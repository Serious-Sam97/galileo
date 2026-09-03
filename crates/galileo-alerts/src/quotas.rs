//! Ingest quotas (warn mode): count today's rows per project from ClickHouse and write a
//! `budget_events` row of kind `ingest_quota` once per day per signal when exceeded.

use galileo_storage::SqlQuery;
use uuid::Uuid;

use crate::Evaluator;

pub async fn run_due(ev: &Evaluator) -> anyhow::Result<()> {
    let rows: Vec<(Uuid, serde_json::Value)> = sqlx::query_as("SELECT project_id, quotas FROM project_settings WHERE quotas <> '{}'::jsonb").fetch_all(&ev.pg).await?;
    for (project, q) in rows {
        let quotas: galileo_core::Quotas = match serde_json::from_value(q) { Ok(q) => q, Err(_) => continue };
        if quotas.is_empty() { continue; }
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        for signal in ["spans", "logs", "metrics"] {
            let Some(limit) = quotas.limit(signal) else { continue };
            let n = ev.storage.query(&SqlQuery { sql: format!("SELECT count() FROM {signal} WHERE project_id = ? AND timestamp >= toStartOfDay(now())"), params: vec![galileo_core::ProjectId(project).into()] }).await?
                .rows.first().and_then(|r| r.first()).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
            if n as u64 > limit {
                let _ = sqlx::query("INSERT INTO budget_events (id, project_id, route_alias, kind, period, spent_usd, cap_usd) VALUES ($1, $2, $3, 'ingest_quota', $4, $5, $6) ON CONFLICT DO NOTHING")
                    .bind(Uuid::now_v7()).bind(project).bind(signal).bind(format!("day:{day}")).bind(n).bind(limit as f64).execute(&ev.pg).await;
            }
        }
    }
    Ok(())
}
