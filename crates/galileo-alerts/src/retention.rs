//! Per-project retention: the ClickHouse TTL is global; projects that want less keep it by
//! deleting their own older rows hourly.

use chrono::{Duration, Utc};
use galileo_core::ProjectId;
use galileo_storage::SqlQuery;
use tracing::info;
use uuid::Uuid;

use crate::Evaluator;

pub async fn run_due(ev: &Evaluator) -> anyhow::Result<()> {
    type Row = (Uuid, Option<i32>, Option<i32>, Option<i32>);
    let due: Vec<Row> = sqlx::query_as(
        "SELECT project_id, retention_spans_days, retention_logs_days, retention_metrics_days FROM project_settings \
         WHERE (retention_spans_days IS NOT NULL OR retention_logs_days IS NOT NULL OR retention_metrics_days IS NOT NULL) \
         AND (retention_last_run IS NULL OR retention_last_run < now() - interval '1 hour')",
    ).fetch_all(&ev.pg).await?;
    for (project, spans, logs, metrics) in due {
        for (table, days) in [("spans", spans), ("logs", logs), ("metrics", metrics)] {
            let Some(d) = days else { continue };
            let cutoff = Utc::now() - Duration::days(d as i64);
            ev.storage.execute(&SqlQuery {
                sql: format!("DELETE FROM {table} WHERE project_id = ? AND timestamp < fromUnixTimestamp64Nano(?)"),
                params: vec![ProjectId(project).into(), cutoff.into()],
            }).await?;
            info!(%project, table, days = d, "retention applied");
        }
        sqlx::query("UPDATE project_settings SET retention_last_run = now() WHERE project_id = $1").bind(project).execute(&ev.pg).await?;
    }
    Ok(())
}
