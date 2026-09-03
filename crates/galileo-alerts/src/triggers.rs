use chrono::{DateTime, Duration, Utc};
use galileo_core::ProjectId;
use galileo_query::{Query, TimeRange};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::notify::Notification;
use crate::Evaluator;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TriggerRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub query: serde_json::Value,
    pub op: String,
    pub threshold: f64,
    pub frequency_secs: i32,
    pub window_secs: i32,
    pub enabled: bool,
    pub recipients: serde_json::Value,
    pub state: String,
    pub last_value: Option<f64>,
    pub last_evaluated_at: Option<DateTime<Utc>>,
    pub last_triggered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub warn_threshold: Option<f64>,
    pub for_secs: i32,
    pub mute_until: Option<DateTime<Utc>>,
    pub per_group: bool,
    pub mode: String,
    pub baseline_factor: f64,
    pub baseline_min_delta: f64,
    pub breaching_since: Option<DateTime<Utc>>,
    pub severity: String,
    #[sqlx(default)] pub sensitivity: Option<i32>,
    #[sqlx(default)] pub min_value: Option<f64>,
    #[sqlx(default)] pub composite: Option<serde_json::Value>,
    #[sqlx(default)] pub mutes: Option<serde_json::Value>,
    #[sqlx(default)] pub last_baseline: Option<f64>,
    #[sqlx(default)] pub last_band: Option<f64>,
}

/// Severity of one value against the trigger's thresholds (critical > warn > ok).
pub fn severity_of(op: &str, value: f64, critical: f64, warn: Option<f64>) -> &'static str {
    if compare(op, value, critical) { return "critical"; }
    if let Some(w) = warn { if compare(op, value, w) { return "warn"; } }
    "ok"
}

/// Baseline mode: the threshold is derived from last week's value for the same window.
/// `critical = baseline × factor + min_delta`; warn (if enabled) sits halfway.
pub fn baseline_thresholds(baseline: Option<f64>, factor: f64, min_delta: f64, want_warn: bool) -> (f64, Option<f64>) {
    let b = baseline.unwrap_or(0.0);
    let critical = b * factor + min_delta;
    let warn = want_warn.then(|| b + (critical - b) / 2.0);
    (critical, warn)
}

/// Sustained-duration gate: a breach only counts after `for_secs` of continuous breaching.
/// Returns (effective severity, new breaching_since).
pub fn sustain(raw: &str, for_secs: i32, breaching_since: Option<DateTime<Utc>>, now: DateTime<Utc>) -> (&'static str, Option<DateTime<Utc>>) {
    if raw == "ok" { return ("ok", None); }
    let since = breaching_since.unwrap_or(now);
    let held = (now - since).num_seconds() >= for_secs as i64;
    (if held { if raw == "critical" { "critical" } else { "warn" } } else { "ok" }, Some(since))
}

#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    pub value: Option<f64>,
    pub triggered: bool,
    pub state: String,
    /// Per-group values when the query has breakdowns.
    pub groups: Vec<(Vec<String>, Option<f64>)>,
    pub error: Option<String>,
    pub severity: String,
    pub baseline: Option<f64>,
    pub group_severities: Vec<(String, Option<f64>, String)>,
    /// Anomaly band (σ·k) around the baseline, when mode = anomaly.
    pub band: Option<f64>,
}

pub fn compare(op: &str, value: f64, threshold: f64) -> bool {
    match op {
        ">" => value > threshold,
        ">=" => value >= threshold,
        "<" => value < threshold,
        "<=" => value <= threshold,
        "=" => (value - threshold).abs() < f64::EPSILON,
        "!=" => (value - threshold).abs() >= f64::EPSILON,
        _ => false,
    }
}

/// Evaluate one trigger now (used by the loop and by the "test"/"preview" endpoints).
pub async fn evaluate(ev: &Evaluator, t: &TriggerRow) -> Evaluation {
    let mut q: Query = match serde_json::from_value(t.query.clone()) {
        Ok(q) => q,
        Err(e) => return Evaluation { value: None, triggered: false, state: "error".into(), groups: vec![], error: Some(format!("invalid query: {e}")), severity: "ok".into(), baseline: None, group_severities: vec![], band: None },
    };
    let window = t.window_secs.max(10) as i64;
    q.time_range = TimeRange::Relative { last_seconds: window };
    q.calculations.truncate(1);
    if q.calculations.is_empty() {
        return Evaluation { value: None, triggered: false, state: "error".into(), groups: vec![], error: Some("trigger query needs one calculation".into()), severity: "ok".into(), baseline: None, group_severities: vec![], band: None };
    }
    q.granularity = Some(window as u32);
    let res = match galileo_query::run::run(ev.storage.as_ref(), ProjectId(t.project_id), &q).await {
        Ok(r) => r,
        Err(e) => return Evaluation { value: None, triggered: false, state: "error".into(), groups: vec![], error: Some(e.to_string()), severity: "ok".into(), baseline: None, group_severities: vec![], band: None },
    };
    let is_count = q.calculations[0].op == galileo_query::CalcOp::Count;
    let groups: Vec<(Vec<String>, Option<f64>)> = res.groups.iter().map(|g| (g.key.clone(), g.totals.first().copied().flatten().or(is_count.then_some(0.0)))).collect();

    let sev_rank = |s: &str| match s { "critical" => 2, "warn" => 1, _ => 0 };
    let sensitivity = t.sensitivity.unwrap_or(3);
    let mut baseline = None;
    let mut band = None;

    // --- anomaly: the same window at the same time of day over the previous 7 days ----------
    if t.mode == "anomaly" {
        let now = Utc::now();
        let mut history = vec![];
        let mut bq = q.clone();
        bq.breakdowns.clear();
        for d in 1..=7 {
            bq.time_range = TimeRange::Absolute { start: now - Duration::days(d) - Duration::seconds(window), end: now - Duration::days(d) };
            if let Ok(br) = galileo_query::run::run(ev.storage.as_ref(), ProjectId(t.project_id), &bq).await {
                if let Some(v) = br.groups.first().and_then(|g| g.totals.first().copied().flatten()).or(is_count.then_some(0.0)) { history.push(v); }
            }
        }
        let overall = groups.iter().filter_map(|(_, v)| *v).fold(None, |acc: Option<f64>, v| Some(acc.map(|a| a.max(v)).unwrap_or(v))).or(is_count.then_some(0.0));
        let verdict = overall.and_then(|v| crate::stats::anomaly(&history, v, sensitivity, t.min_value.unwrap_or(0.0)));
        let (fired, med, bd) = match verdict { Some((f, m, b)) => (f, Some(m), Some(b)), None => (false, None, None) };
        baseline = med; band = bd;
        let severity = if fired { "critical" } else { "ok" };
        let group_severities: Vec<(String, Option<f64>, String)> = groups.iter().map(|(k, v)| (k.join(","), *v, severity.to_string())).collect();
        return Evaluation { value: overall, triggered: fired, state: if fired { "triggered".into() } else { "ok".into() }, groups, error: None, severity: severity.into(), baseline, group_severities, band };
    }

    // --- outlier: groups whose value sits far from the other groups --------------------------
    if t.mode == "outlier" {
        let vals: Vec<(String, f64)> = groups.iter().filter_map(|(k, v)| v.map(|x| (k.join(","), x))).collect();
        let out = crate::stats::outliers(&vals, sensitivity);
        let med = out.first().map(|o| o.2);
        let group_severities: Vec<(String, Option<f64>, String)> = groups.iter().map(|(k, v)| { let key = k.join(","); (key.clone(), *v, if out.iter().any(|o| o.0 == key) { "critical".to_string() } else { "ok".to_string() }) }).collect();
        let worst = group_severities.iter().max_by_key(|(_, _, s)| sev_rank(s));
        let (value, severity) = worst.map(|(_, v, s)| (*v, s.clone())).unwrap_or((None, "ok".into()));
        let triggered = severity != "ok";
        return Evaluation { value, triggered, state: if triggered { "triggered".into() } else { "ok".into() }, groups, error: None, severity, baseline: med, group_severities, band: None };
    }

    // Baseline: same query, same window, seven days earlier.
    let (critical, warn) = if t.mode == "baseline" {
        let now = Utc::now();
        let mut bq = q.clone();
        bq.time_range = TimeRange::Absolute { start: now - Duration::days(7) - Duration::seconds(window), end: now - Duration::days(7) };
        bq.breakdowns.clear();
        if let Ok(br) = galileo_query::run::run(ev.storage.as_ref(), ProjectId(t.project_id), &bq).await {
            baseline = br.groups.first().and_then(|g| g.totals.first().copied().flatten()).or(is_count.then_some(0.0));
        }
        baseline_thresholds(baseline, t.baseline_factor, t.baseline_min_delta, t.warn_threshold.is_some())
    } else {
        (t.threshold, t.warn_threshold)
    };
    let group_severities: Vec<(String, Option<f64>, String)> = groups.iter().map(|(k, v)| (k.join(","), *v, v.map(|x| severity_of(&t.op, x, critical, warn).to_string()).unwrap_or_else(|| "ok".into()))).collect();
    let worst = group_severities.iter().max_by_key(|(_, _, s)| sev_rank(s));
    let (value, severity) = match worst {
        Some((_, v, sev)) => (*v, sev.clone()),
        None => (is_count.then_some(0.0), "ok".to_string()),
    };
    let triggered = severity != "ok";
    let _ = band;
    Evaluation { value, triggered, state: if triggered { "triggered".into() } else { "ok".into() }, groups, error: None, severity, baseline, group_severities, band: None }
}

pub async fn evaluate_due(ev: &Evaluator) -> anyhow::Result<()> {
    let due: Vec<TriggerRow> = sqlx::query_as(
        "SELECT * FROM triggers WHERE enabled AND (last_evaluated_at IS NULL OR last_evaluated_at < now() - make_interval(secs => frequency_secs))",
    ).fetch_all(&ev.pg).await?;
    let now = Utc::now();
    let windows: Vec<(Uuid, Vec<Uuid>)> = sqlx::query_as("SELECT project_id, trigger_ids FROM maintenance_windows WHERE starts_at <= now() AND ends_at > now()").fetch_all(&ev.pg).await.unwrap_or_default();
    let in_window = |t: &TriggerRow| windows.iter().any(|(p, ids)| *p == t.project_id && (ids.is_empty() || ids.contains(&t.id)));
    let mut composites = vec![];
    for t in due {
        if t.composite.as_ref().map(|c| !c.is_null()).unwrap_or(false) { composites.push(t); continue; }
        let e = evaluate(ev, &t).await;
        let muted = t.mute_until.map(|m| m > now).unwrap_or(false) || in_window(&t);
        let group_muted: Vec<String> = t.mutes.as_ref().and_then(|m| m.as_array()).map(|a| a.iter().filter(|m| m.get("until").and_then(|u| u.as_str()).and_then(|u| u.parse::<DateTime<Utc>>().ok()).map(|u| u > now).unwrap_or(true)).filter_map(|m| m.get("group_key").and_then(|g| g.as_str()).map(str::to_owned)).collect()).unwrap_or_default();
        // Sustained-duration gate on the overall severity.
        let (eff, since) = sustain(&e.severity, t.for_secs, t.breaching_since, now);
        let new_state = if e.error.is_some() { "error" } else if muted { "muted" } else if eff == "ok" { "ok" } else { "triggered" };
        let changed = new_state != t.state || (new_state == "triggered" && eff != t.severity);
        sqlx::query(
            "UPDATE triggers SET state = $2, severity = $3, last_value = $4, breaching_since = $5, last_evaluated_at = now(), last_baseline = $6, last_band = $7, \
             last_triggered_at = CASE WHEN $2 = 'triggered' AND (state != 'triggered' OR severity != $3) THEN now() ELSE last_triggered_at END WHERE id = $1",
        ).bind(t.id).bind(new_state).bind(eff).bind(e.value).bind(since).bind(e.baseline).bind(e.band).execute(&ev.pg).await?;

        // Per-group states (informational + per-group notifications when enabled).
        if t.per_group && !t.query.get("breakdowns").and_then(|b| b.as_array()).map(|a| a.is_empty()).unwrap_or(true) {
            for (key, val, raw) in &e.group_severities {
                let prev: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as("SELECT severity, breaching_since FROM trigger_group_states WHERE trigger_id = $1 AND group_key = $2").bind(t.id).bind(key).fetch_optional(&ev.pg).await?;
                let (geff, gsince) = sustain(raw, t.for_secs, prev.as_ref().and_then(|p| p.1), now);
                sqlx::query("INSERT INTO trigger_group_states (trigger_id, group_key, severity, last_value, breaching_since) VALUES ($1, $2, $3, $4, $5) \
                             ON CONFLICT (trigger_id, group_key) DO UPDATE SET severity = $3, last_value = $4, breaching_since = $5, updated_at = now()")
                    .bind(t.id).bind(key).bind(geff).bind(val).bind(gsince).execute(&ev.pg).await?;
                let prev_sev = prev.map(|p| p.0).unwrap_or_else(|| "ok".into());
                if geff != prev_sev && e.error.is_none() { crate::incidents::transition(ev, &t, key, geff, *val, now).await; }
                if geff != prev_sev && !muted && !group_muted.contains(key) && e.error.is_none() {
                    let msg = format!("{} for {} = {} ({}; threshold {} {})", describe_query(&t.query), key, val.map(|v| format!("{v:.3}")).unwrap_or_else(|| "n/a".into()), geff, t.op, t.threshold);
                    sqlx::query("INSERT INTO trigger_events (id, trigger_id, state, value, message) VALUES ($1, $2, $3, $4, $5)")
                        .bind(Uuid::now_v7()).bind(t.id).bind(if geff == "ok" { "ok" } else { "triggered" }).bind(*val).bind(&msg).execute(&ev.pg).await?;
                    ev.notify(t.project_id, &t.recipients, &Notification { kind: "trigger", state: geff.to_string(), name: t.name.clone(), project_id: t.project_id, title: format!("{} · {}", t.name, key), message: msg, value: *val, threshold: Some(t.threshold), url: Some(format!("{}/p/{}/triggers/{}", ev.public_url, t.project_id, t.id)), at: now }).await;
                }
            }
            continue; // per-group mode notifies per group, not overall
        }

        if changed && e.error.is_none() { crate::incidents::transition(ev, &t, "", eff, e.value, now).await; }
        if changed && new_state != "muted" {
            let message = match &e.error {
                Some(err) => format!("evaluation error: {err}"),
                None => format!(
                    "{} = {} ({}; critical {} {}{}{}) over the last {}s",
                    describe_query(&t.query), e.value.map(|v| format!("{v:.3}")).unwrap_or_else(|| "n/a".into()), eff, t.op, t.threshold,
                    t.warn_threshold.map(|w| format!(", warn {w}")).unwrap_or_default(),
                    e.baseline.map(|b| format!(", baseline last week {b:.3}")).unwrap_or_default(), t.window_secs
                ),
            };
            sqlx::query("INSERT INTO trigger_events (id, trigger_id, state, value, message) VALUES ($1, $2, $3, $4, $5)")
                .bind(Uuid::now_v7()).bind(t.id).bind(new_state).bind(e.value).bind(&message).execute(&ev.pg).await?;
            if new_state != "error" {
                ev.notify(t.project_id, &t.recipients, &Notification { kind: "trigger", state: if new_state == "ok" { "ok".into() } else { eff.to_string() }, name: t.name.clone(), project_id: t.project_id, title: format!("Trigger: {}", t.name), message, value: e.value, threshold: Some(t.threshold), url: Some(format!("{}/p/{}/triggers/{}", ev.public_url, t.project_id, t.id)), at: now }).await;
            }
        }
    }
    for t in composites { crate::incidents::evaluate_composite(ev, &t, now).await; }
    crate::incidents::escalate_due(ev, now).await;
    Ok(())
}

fn describe_query(q: &serde_json::Value) -> String {
    let calc = q
        .get("calculations")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .map(|c| {
            let op = c.get("op").and_then(|o| o.as_str()).unwrap_or("COUNT");
            match c.get("field").and_then(|f| f.as_str()) {
                Some(f) => format!("{op}({f})"),
                None => op.to_string(),
            }
        })
        .unwrap_or_else(|| "COUNT".into());
    let ds = q.get("dataset").and_then(|d| d.as_str()).unwrap_or("spans");
    format!("{calc} on {ds}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn compare_ops() {
        use super::compare;
        assert!(compare(">", 2.0, 1.0));
        assert!(!compare(">", 1.0, 1.0));
        assert!(compare(">=", 1.0, 1.0));
        assert!(compare("<", 0.5, 1.0));
        assert!(compare("=", 1.0, 1.0));
        assert!(compare("!=", 1.1, 1.0));
        assert!(!compare("??", 1.0, 1.0));
    }

    #[test]
    fn severity_and_baseline() {
        use super::{baseline_thresholds, severity_of, sustain};
        assert_eq!(severity_of(">", 900.0, 800.0, Some(500.0)), "critical");
        assert_eq!(severity_of(">", 600.0, 800.0, Some(500.0)), "warn");
        assert_eq!(severity_of(">", 100.0, 800.0, Some(500.0)), "ok");
        assert_eq!(baseline_thresholds(Some(100.0), 2.0, 10.0, true), (210.0, Some(155.0)));
        assert_eq!(baseline_thresholds(None, 2.0, 5.0, false), (5.0, None));
        let now = chrono::Utc::now();
        let (s1, since) = sustain("critical", 300, None, now);
        assert_eq!(s1, "ok");
        assert!(since.is_some());
        let (s2, _) = sustain("critical", 300, Some(now - chrono::Duration::seconds(400)), now);
        assert_eq!(s2, "critical");
        assert_eq!(sustain("ok", 300, Some(now), now), ("ok", None));
    }
}
