use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::state::AppState;

pub async fn health(State(st): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let ch = st.storage.ping().await.is_ok();
    let pg = sqlx::query("SELECT 1").execute(&st.pg).await.is_ok();
    let ok = ch && pg;
    (
        if ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
        Json(json!({ "ok": ok, "clickhouse": ch, "postgres": pg, "version": env!("CARGO_PKG_VERSION") })),
    )
}

pub async fn stats(State(st): State<AppState>, _cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "ingest": st.ingest_stats.snapshot() })))
}

/// Detailed self-health for org owners: ingest, sampling, ClickHouse parts, Postgres pool,
/// gateway latency, evaluator age. Cheap enough to poll every 10 s.
pub async fn detail(State(st): State<AppState>, _cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    let ingest = st.ingest_stats.snapshot();
    let ch_parts = st.storage.query(&galileo_storage::SqlQuery {
        sql: "SELECT table, count() AS parts, sum(rows) AS rows, sum(bytes_on_disk) AS bytes, min(partition) AS oldest_partition FROM system.parts WHERE database = currentDatabase() AND active AND table IN ('spans', 'logs', 'metrics') GROUP BY table".into(), params: vec![] }).await.ok();
    let ch_merges = st.storage.query(&galileo_storage::SqlQuery { sql: "SELECT count() FROM system.merges".into(), params: vec![] }).await.ok();
    let ch_disk = st.storage.query(&galileo_storage::SqlQuery { sql: "SELECT name, free_space, total_space FROM system.disks".into(), params: vec![] }).await.ok();
    let ch_last = st.storage.query(&galileo_storage::SqlQuery { sql: "SELECT max(timestamp) FROM spans WHERE timestamp > now() - INTERVAL 1 DAY".into(), params: vec![] }).await.ok();
    let gw = st.storage.query(&galileo_storage::SqlQuery {
        sql: "SELECT count(), countIf(status_code = 'error'), quantileTDigest(0.95)(duration_ns) / 1e6 FROM spans WHERE service_name = 'galileo-gateway' AND timestamp > now() - INTERVAL 5 MINUTE".into(), params: vec![] }).await.ok();
    let eval_age: Option<(Option<f64>,)> = sqlx::query_as("SELECT EXTRACT(EPOCH FROM now() - max(issues_last_run))::float8 FROM project_settings").fetch_optional(&st.pg).await.unwrap_or(None);
    let pool = json!({ "size": st.pg.size(), "idle": st.pg.num_idle() });
    let v = |r: &Option<galileo_storage::QueryResult>, i: usize| r.as_ref().and_then(|x| x.rows.first()).and_then(|row| row.get(i)).cloned().unwrap_or(serde_json::Value::Null);
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": st.started_at.elapsed().as_secs(),
        "ingest": ingest,
        "queue_capacity": st.config.ingest.max_queued_rows,
        "clickhouse": {
            "parts": ch_parts.as_ref().map(|r| r.to_objects()).unwrap_or_default(),
            "merges": v(&ch_merges, 0),
            "disks": ch_disk.as_ref().map(|r| r.to_objects()).unwrap_or_default(),
            "last_span_at": v(&ch_last, 0),
        },
        "postgres": pool,
        "gateway_5m": { "calls": v(&gw, 0), "errors": v(&gw, 1), "p95_ms": v(&gw, 2) },
        "evaluator_age_secs": eval_age.and_then(|x| x.0),
    })))
}

/// `galileo.health.*` gauges about Galileo itself, written into the Default project every 30 s so
/// triggers and boards can watch the server. Returns None when no project is named Default.
pub async fn health_points(st: &AppState, project: galileo_core::ProjectId) -> Vec<galileo_core::MetricPoint> {
    use galileo_core::{metric::{AggregationTemporality, MetricKind}, Attributes, MetricPoint};
    let now = chrono::Utc::now();
    let ingest = st.ingest_stats.snapshot();
    let g = |k: &str| ingest.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let queue_fill = if st.config.ingest.max_queued_rows > 0 { g("queued_rows") / st.config.ingest.max_queued_rows as f64 } else { 0.0 };
    let parts = st.storage.query(&galileo_storage::SqlQuery { sql: "SELECT count() FROM system.parts WHERE database = currentDatabase() AND active AND table IN ('spans', 'logs', 'metrics')".into(), params: vec![] }).await.ok()
        .and_then(|r| r.rows.first().and_then(|row| row.first()).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))).unwrap_or(0.0);
    let gw = st.storage.query(&galileo_storage::SqlQuery { sql: "SELECT quantileTDigest(0.95)(duration_ns) / 1e6, countIf(status_code = 'error') / greatest(count(), 1) FROM spans WHERE service_name = 'galileo-gateway' AND timestamp > now() - INTERVAL 5 MINUTE".into(), params: vec![] }).await.ok();
    let gwv = |i: usize| gw.as_ref().and_then(|r| r.rows.first()).and_then(|row| row.get(i)).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0);
    let eval_age: Option<(Option<f64>,)> = sqlx::query_as("SELECT EXTRACT(EPOCH FROM now() - max(issues_last_run))::float8 FROM project_settings").fetch_optional(&st.pg).await.unwrap_or(None);
    let last_write_age = { let t = g("last_write_at"); if t > 0.0 { (now.timestamp() as f64 - t).max(0.0) } else { 0.0 } };
    let mk = |name: &str, unit: &str, value: f64| MetricPoint {
        project_id: project, name: format!("galileo.health.{name}"), description: String::new(), unit: unit.into(), kind: MetricKind::Gauge,
        temporality: AggregationTemporality::Unspecified, is_monotonic: false, timestamp: now, start_timestamp: None,
        service_name: "galileo-server".into(), scope_name: "galileo-health".into(), resource: Attributes::default(), attributes: Attributes::default(),
        value, count: 0, sum: 0.0, min: None, max: None, bucket_counts: vec![], explicit_bounds: vec![],
        exp_scale: 0, exp_zero_count: 0, exp_pos_offset: 0, exp_pos_counts: vec![], exp_neg_offset: 0, exp_neg_counts: vec![],
    };
    vec![
        mk("queue_fill", "1", queue_fill),
        mk("rows_written_total", "1", g("rows_written")),
        mk("rows_dropped_total", "1", g("rows_dropped")),
        mk("rejected_backpressure_total", "1", g("rejected_backpressure")),
        mk("sampled_buffered", "1", g("sampled_buffered")),
        mk("last_write_age_s", "s", last_write_age),
        mk("parts", "1", parts),
        mk("gateway_p95_ms", "ms", gwv(0)),
        mk("gateway_error_rate", "1", gwv(1)),
        mk("evaluator_age_s", "s", eval_age.and_then(|x| x.0).unwrap_or(0.0)),
    ]
}
