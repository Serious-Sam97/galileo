//! Turn gateway budget events (80% / exhausted) into notifications, once each.

use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::notify::Notification;
use crate::Evaluator;

pub async fn run_due(ev: &Evaluator) -> anyhow::Result<()> {
    let rows: Vec<(Uuid, Uuid, String, String, String, f64, f64)> = sqlx::query_as(
        "SELECT id, project_id, route_alias, kind, period, spent_usd, cap_usd FROM budget_events WHERE NOT notified ORDER BY created_at LIMIT 100",
    ).fetch_all(&ev.pg).await?;
    for (id, project, alias, kind, period, spent, cap) in rows {
        // recipients: the route's budget.alert_recipients, else the project's issue recipients
        let route: Option<(Value,)> = sqlx::query_as("SELECT budget FROM gateway_routes WHERE project_id = $1 AND alias = $2").bind(project).bind(&alias).fetch_optional(&ev.pg).await?;
        let mut recipients = route.and_then(|(b,)| b.get("alert_recipients").cloned()).unwrap_or(Value::Null);
        if !recipients.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            let ps: Option<(Value,)> = sqlx::query_as("SELECT issue_recipients FROM project_settings WHERE project_id = $1").bind(project).fetch_optional(&ev.pg).await?;
            recipients = ps.map(|p| p.0).unwrap_or(json!([]));
        }
        let (state, title) = match kind.as_str() {
            "exhausted" => ("exhausted", format!("LLM budget exhausted: {alias}")),
            "anomaly" => ("critical", format!("LLM cost anomaly: {alias}")),
            "ingest_quota" => ("warn", format!("Ingest quota exceeded: {alias}")),
            _ => ("warn", format!("LLM budget at 80%: {alias}")),
        };
        let period_label = period.replace("day:", "today ").replace("month:", "this month ").replace("hour:", "hour ");
        ev.notify(project, &recipients, &Notification { kind: "budget", state: state.into(), name: alias.clone(), project_id: project, title, message: if kind == "ingest_quota" { format!("{spent:.0} {alias} rows today vs a quota of {cap:.0}. Consider sampling, a log pipeline drop rule, or raising the quota (Settings → Project → Quotas).") } else if kind == "anomaly" { format!("${spent:.4} spent this hour ({period_label}) vs a same-hour baseline that predicts at most ${cap:.4}. Check for loops, retries or a prompt that grew.") } else { format!("${spent:.4} of ${cap:.2} spent ({period_label}). {}", if kind == "exhausted" { "Calls on this route are being rejected with 429." } else { "Calls continue until the cap." }) }, value: Some(spent), threshold: Some(cap), url: Some(format!("{}/p/{}/ai", ev.public_url, project)), at: Utc::now() }).await;
        sqlx::query("UPDATE budget_events SET notified = true WHERE id = $1").bind(id).execute(&ev.pg).await?;
    }
    Ok(())
}
