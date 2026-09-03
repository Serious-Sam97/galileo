//! SLOs: an SLI defined as good/total events (each a filter set on a dataset), a target, a
//! rolling window, and multi-window burn-rate alerts.

use chrono::{DateTime, Duration, Utc};
use galileo_core::ProjectId;
use galileo_query::sql::{filter_sql, where_clause};
use galileo_query::{Dataset, Filter, Query, TimeRange};
use galileo_storage::SqlQuery;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::notify::Notification;
use crate::Evaluator;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SloRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub dataset: String,
    pub total_filters: serde_json::Value,
    pub good_filters: serde_json::Value,
    pub target_pct: f64,
    pub window_days: i32,
    pub burn_alerts: serde_json::Value,
    pub state: String,
    pub last_evaluated_at: Option<DateTime<Utc>>,
    pub last_result: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnAlert {
    pub name: String,
    #[serde(default = "d_long")]
    pub long_window_mins: i64,
    #[serde(default = "d_short")]
    pub short_window_mins: i64,
    #[serde(default = "d_rate")]
    pub burn_rate: f64,
    #[serde(default)]
    pub recipients: serde_json::Value,
}
fn d_long() -> i64 {
    60
}
fn d_short() -> i64 {
    5
}
fn d_rate() -> f64 {
    14.4
}

#[derive(Debug, Clone, Serialize)]
pub struct Counts {
    pub total: u64,
    pub good: u64,
}

impl Counts {
    pub fn sli(&self) -> Option<f64> {
        (self.total > 0).then(|| self.good as f64 / self.total as f64)
    }
}

/// good/total over a time range.
pub async fn counts(ev: &Evaluator, project: Uuid, dataset: Dataset, total: &[Filter], good: &[Filter], start: DateTime<Utc>, end: DateTime<Utc>) -> anyhow::Result<Counts> {
    let base = Query { dataset, filters: total.to_vec(), calculations: vec![], ..Default::default() };
    let w = where_clause(&base, ProjectId(project), start, end)?;
    // The "good" expression sits in the SELECT list before the WHERE, so its params bind first.
    let mut good_params = Vec::new();
    let good_sql = if good.is_empty() {
        "1".to_string()
    } else {
        let mut parts = Vec::new();
        for f in good {
            parts.push(filter_sql(dataset, f, &mut good_params)?);
        }
        parts.join(" AND ")
    };
    let mut params = good_params;
    params.extend(w.params);
    let q = SqlQuery {
        sql: format!("SELECT count() AS total, countIf({good_sql}) AS good FROM {} WHERE {}", dataset.table(), w.sql),
        params,
    };
    let r = ev.storage.query(&q).await?;
    let n = |v: &serde_json::Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0) as u64;
    Ok(r.rows.first().map(|row| Counts { total: n(&row[0]), good: n(&row[1]) }).unwrap_or(Counts { total: 0, good: 0 }))
}

fn parse_filters(v: &serde_json::Value) -> Vec<Filter> {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

fn dataset_of(s: &str) -> Dataset {
    match s {
        "logs" => Dataset::Logs,
        "metrics" => Dataset::Metrics,
        _ => Dataset::Spans,
    }
}

/// Full evaluation: window SLI, budget, burn alerts, and a daily SLI series for the chart.
pub async fn evaluate(ev: &Evaluator, s: &SloRow, with_series: bool) -> anyhow::Result<serde_json::Value> {
    let ds = dataset_of(&s.dataset);
    let total_f = parse_filters(&s.total_filters);
    let good_f = parse_filters(&s.good_filters);
    let now = Utc::now();
    let (start, end) = TimeRange::Relative { last_seconds: s.window_days.max(1) as i64 * 86400 }.resolve(now);
    let window = counts(ev, s.project_id, ds, &total_f, &good_f, start, end).await?;
    let target = s.target_pct / 100.0;
    let budget = 1.0 - target;
    let sli = window.sli();
    let error_rate = sli.map(|v| 1.0 - v);
    let budget_consumed = error_rate.map(|e| if budget > 0.0 { e / budget } else { 0.0 });
    let budget_remaining_pct = budget_consumed.map(|c| (1.0 - c) * 100.0);
    // Allowed bad events in the window, and how many are left.
    let allowed_bad = (window.total as f64 * budget).floor() as i64;
    let bad = (window.total - window.good) as i64;

    let alerts: Vec<BurnAlert> = serde_json::from_value(s.burn_alerts.clone()).unwrap_or_default();
    let mut alert_results = Vec::new();
    let mut any_burning = false;
    for a in &alerts {
        let long = counts(ev, s.project_id, ds, &total_f, &good_f, now - Duration::minutes(a.long_window_mins.max(1)), now).await?;
        let short = counts(ev, s.project_id, ds, &total_f, &good_f, now - Duration::minutes(a.short_window_mins.max(1)), now).await?;
        let burn = |c: &Counts| c.sli().map(|v| if budget > 0.0 { (1.0 - v) / budget } else { 0.0 });
        let lb = burn(&long);
        let sb = burn(&short);
        let triggered = matches!((lb, sb), (Some(l), Some(sh)) if l >= a.burn_rate && sh >= a.burn_rate);
        any_burning |= triggered;
        alert_results.push(json!({
            "name": a.name, "burn_rate": a.burn_rate, "long_window_mins": a.long_window_mins, "short_window_mins": a.short_window_mins,
            "long_burn": lb, "short_burn": sb, "long": long, "short": short, "triggered": triggered,
        }));
    }

    let mut series = Vec::new();
    if with_series {
        let days = s.window_days.clamp(1, 90);
        for i in (0..days).rev() {
            let day_end = now - Duration::days(i as i64);
            let day_start = day_end - Duration::days(1);
            let c = counts(ev, s.project_id, ds, &total_f, &good_f, day_start, day_end).await?;
            series.push(json!({ "ts": day_end.timestamp(), "sli": c.sli(), "total": c.total, "good": c.good }));
        }
    }

    Ok(json!({
        "evaluated_at": now,
        "window_days": s.window_days,
        "target_pct": s.target_pct,
        "sli": sli,
        "sli_pct": sli.map(|v| v * 100.0),
        "total": window.total,
        "good": window.good,
        "bad": bad,
        "allowed_bad": allowed_bad,
        "budget_remaining_pct": budget_remaining_pct,
        "budget_consumed": budget_consumed,
        "state": if any_burning { "burning" } else if budget_remaining_pct.map(|p| p < 0.0).unwrap_or(false) { "exhausted" } else { "ok" },
        "burn_alerts": alert_results,
        "series": series,
    }))
}

pub async fn evaluate_due(ev: &Evaluator) -> anyhow::Result<()> {
    let due: Vec<SloRow> = sqlx::query_as("SELECT * FROM slos WHERE last_evaluated_at IS NULL OR last_evaluated_at < now() - interval '60 seconds'")
        .fetch_all(&ev.pg)
        .await?;
    for s in due {
        let result = match evaluate(ev, &s, false).await {
            Ok(r) => r,
            Err(e) => json!({ "error": e.to_string(), "state": "error" }),
        };
        let state = result.get("state").and_then(|v| v.as_str()).unwrap_or("error").to_string();
        sqlx::query("UPDATE slos SET state = $2, last_result = $3, last_evaluated_at = now() WHERE id = $1")
            .bind(s.id)
            .bind(&state)
            .bind(&result)
            .execute(&ev.pg)
            .await?;
        if state != s.state && state != "error" {
            let alerts: Vec<BurnAlert> = serde_json::from_value(s.burn_alerts.clone()).unwrap_or_default();
            let mut recipients: Vec<serde_json::Value> = Vec::new();
            for a in &alerts {
                if let Some(arr) = a.recipients.as_array() { recipients.extend(arr.iter().cloned()); }
            }
            if !recipients.is_empty() {
                let msg = format!(
                    "SLI {} vs target {}% over {} days; error budget remaining {}",
                    result.get("sli_pct").and_then(|v| v.as_f64()).map(|v| format!("{v:.3}%")).unwrap_or_else(|| "n/a".into()),
                    s.target_pct,
                    s.window_days,
                    result.get("budget_remaining_pct").and_then(|v| v.as_f64()).map(|v| format!("{v:.1}%")).unwrap_or_else(|| "n/a".into())
                );
                ev.notify(
                    s.project_id,
                    &serde_json::Value::Array(recipients),
                    &Notification {
                        kind: "slo",
                        state: if state == "ok" { "ok".into() } else { "triggered".into() },
                        name: s.name.clone(),
                        project_id: s.project_id,
                        title: format!("SLO: {} is {}", s.name, state),
                        message: msg,
                        value: result.get("sli_pct").and_then(|v| v.as_f64()),
                        threshold: Some(s.target_pct),
                        url: Some(format!("{}/p/{}/slos/{}", ev.public_url, s.project_id, s.id)),
                        at: Utc::now(),
                    },
                )
                .await;
            }
        }
    }
    Ok(())
}
