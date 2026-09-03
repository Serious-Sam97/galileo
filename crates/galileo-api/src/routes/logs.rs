//! Log pipelines, log-based metrics, usage and quotas.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::Utc;
use galileo_core::{LogMetric, LogPipeline, ProjectId, Quotas};
use galileo_query::TimeRange;
use galileo_storage::SqlQuery;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub async fn get_pipeline(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<Value>> {
    let row: Option<(Value,)> = sqlx::query_as("SELECT pipeline FROM log_pipelines WHERE project_id = $1").bind(pa.project.id).fetch_optional(&st.pg).await?;
    let p: LogPipeline = row.and_then(|(v,)| serde_json::from_value(v).ok()).unwrap_or_default();
    Ok(Json(json!({ "pipeline": p })))
}

pub async fn put_pipeline(State(st): State<AppState>, pa: ProjectAccess, Json(p): Json<LogPipeline>) -> ApiResult<Json<Value>> {
    pa.require_write()?;
    if p.processors.len() > 50 { return Err(ApiError::BadRequest("at most 50 processors".into())); }
    for pr in &p.processors {
        if let galileo_core::Processor::RegexExtract { pattern, .. } = pr { regex::Regex::new(pattern).map_err(|e| ApiError::BadRequest(format!("bad regex '{pattern}': {e}")))?; }
    }
    sqlx::query("INSERT INTO log_pipelines (project_id, pipeline) VALUES ($1, $2) ON CONFLICT (project_id) DO UPDATE SET pipeline = $2, updated_at = now()").bind(pa.project.id).bind(serde_json::to_value(&p).unwrap_or_default()).execute(&st.pg).await?;
    st.resolver.invalidate_all();
    audit::project(&st.pg, &pa, "logs.pipeline", "project", pa.project.id, json!({ "processors": p.processors.len(), "enabled": p.enabled })).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PreviewBody { pub pipeline: LogPipeline, #[serde(default = "d_n")] pub sample: u32, #[serde(default)] pub metrics: Vec<LogMetric> }
fn d_n() -> u32 { 20 }

/// Run a pipeline over recent logs: before/after per record, so the editor shows what it does.
pub async fn preview(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<PreviewBody>) -> ApiResult<Json<Value>> {
    let n = b.sample.clamp(1, 100);
    let (start, end) = TimeRange::Relative { last_seconds: 7 * 86_400 }.resolve(Utc::now());
    let rows = st.storage.query(&SqlQuery { sql: format!("SELECT timestamp, severity, body, service_name, toJSONString(attrs), toJSONString(resource) FROM logs WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) ORDER BY timestamp DESC LIMIT {n}"), params: vec![ProjectId(pa.project.id).into(), start.into(), end.into()] }).await?;
    let mut out = vec![];
    for r in &rows.rows {
        let g = |i: usize| r.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut rec = galileo_core::LogRecord { project_id: ProjectId(pa.project.id), timestamp: Utc::now(), observed_timestamp: Utc::now(), severity: galileo_core::log::Severity::from_text(&g(1)), severity_text: g(1).to_uppercase(), body: g(2), body_value: None, trace_id: None, span_id: None, service_name: g(3), scope_name: String::new(), resource: Default::default(), attributes: Default::default() };
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&g(4)) { for (k, v) in m { rec.attributes.insert(k, galileo_core::AttributeValue::Str(v.as_str().unwrap_or("").to_string())); } }
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&g(5)) { for (k, v) in m { rec.resource.insert(k, galileo_core::AttributeValue::Str(v.as_str().unwrap_or("").to_string())); } }
        let before = json!({ "severity": rec.severity_text, "body": rec.body, "attrs": rec.attributes.iter().map(|(k, v)| (k.clone(), Value::String(galileo_core::pipeline::attr_str(v)))).collect::<serde_json::Map<_, _>>() });
        let kept = b.pipeline.apply(&mut rec);
        let metrics: Vec<Value> = b.metrics.iter().filter_map(|m| m.value(&rec).map(|v| json!({ "name": m.name, "value": v }))).collect();
        let after = json!({ "severity": rec.severity_text, "body": rec.body, "attrs": rec.attributes.iter().map(|(k, v)| (k.clone(), Value::String(galileo_core::pipeline::attr_str(v)))).collect::<serde_json::Map<_, _>>() });
        out.push(json!({ "timestamp": g(0), "kept": kept, "before": before, "after": after, "metrics": metrics }));
    }
    Ok(Json(json!({ "records": out })))
}

// ---- log metrics
pub async fn list_metrics(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<Value>> {
    let rows: Vec<(Uuid, String, Value, chrono::DateTime<Utc>)> = sqlx::query_as("SELECT id, name, rule, created_at FROM log_metrics WHERE project_id = $1 ORDER BY name").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "metrics": rows.into_iter().map(|(id, name, rule, at)| json!({ "id": id, "name": name, "rule": rule, "created_at": at })).collect::<Vec<_>>() })))
}
pub async fn create_metric(State(st): State<AppState>, pa: ProjectAccess, Json(m): Json<LogMetric>) -> ApiResult<Json<Value>> {
    pa.require_write()?;
    let name = m.name.trim().to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') { return Err(ApiError::BadRequest("name must be alphanumeric with _ or .".into())); }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO log_metrics (id, project_id, name, rule) VALUES ($1, $2, $3, $4) ON CONFLICT (project_id, name) DO UPDATE SET rule = $4").bind(id).bind(pa.project.id).bind(&name).bind(serde_json::to_value(&LogMetric { name: name.clone(), ..m }).unwrap_or_default()).execute(&st.pg).await?;
    st.resolver.invalidate_all();
    Ok(Json(json!({ "id": id, "name": format!("log.{name}") })))
}
#[derive(Deserialize)]
pub struct MetricPath { #[allow(dead_code)] pub project_id: Uuid, pub metric_id: Uuid }
pub async fn delete_metric(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<MetricPath>) -> ApiResult<Json<Value>> {
    pa.require_write()?;
    sqlx::query("DELETE FROM log_metrics WHERE id = $1 AND project_id = $2").bind(p.metric_id).bind(pa.project.id).execute(&st.pg).await?;
    st.resolver.invalidate_all();
    Ok(Json(json!({ "ok": true })))
}

// ---- usage + cardinality
#[derive(Deserialize)]
pub struct UsageParams { #[serde(default = "d_last")] pub last_seconds: i64 }
fn d_last() -> i64 { 7 * 86_400 }

pub async fn usage(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<UsageParams>) -> ApiResult<Json<Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let pid = ProjectId(pa.project.id);
    let mut per_day = vec![];
    let mut totals = serde_json::Map::new();
    for table in ["spans", "logs", "metrics"] {
        let r = st.storage.query(&SqlQuery { sql: format!("SELECT toDate(timestamp) AS d, count() FROM {table} WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) GROUP BY d ORDER BY d"), params: vec![pid.into(), start.into(), end.into()] }).await?;
        let mut total = 0f64;
        for row in &r.rows { let n = row.get(1).and_then(num).unwrap_or(0.0); total += n; per_day.push(json!({ "day": row.first().and_then(|v| v.as_str()).unwrap_or(""), "signal": table, "rows": n })); }
        // bytes: table-level average bytes per row from system.parts × this project's rows
        let bpr = st.storage.query(&SqlQuery { sql: format!("SELECT sum(bytes_on_disk) / greatest(sum(rows), 1) FROM system.parts WHERE database = currentDatabase() AND active AND table = '{table}'"), params: vec![] }).await.ok().and_then(|x| x.rows.first().and_then(|r| r.first()).and_then(num)).unwrap_or(0.0);
        totals.insert(table.into(), json!({ "rows": total, "est_bytes": total * bpr, "bytes_per_row": bpr }));
    }
    let today = st.storage.query(&SqlQuery { sql: "SELECT (SELECT count() FROM spans WHERE project_id = ? AND timestamp >= toStartOfDay(now())), (SELECT count() FROM logs WHERE project_id = ? AND timestamp >= toStartOfDay(now())), (SELECT count() FROM metrics WHERE project_id = ? AND timestamp >= toStartOfDay(now()))".into(), params: vec![pid.into(), pid.into(), pid.into()] }).await?;
    let t = today.rows.first();
    let card = st.storage.query(&SqlQuery { sql: "SELECT k, uniq(v) AS distinct_values, count() AS rows FROM spans ARRAY JOIN mapKeys(attrs) AS k, mapValues(attrs) AS v WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) GROUP BY k ORDER BY distinct_values DESC LIMIT 25".into(), params: vec![pid.into(), start.into(), end.into()] }).await?;
    let quotas: Option<(Value,)> = sqlx::query_as("SELECT quotas FROM project_settings WHERE project_id = $1").bind(pa.project.id).fetch_optional(&st.pg).await?;
    Ok(Json(json!({
        "start": start, "end": end, "per_day": per_day, "totals": totals,
        "today": { "spans": t.and_then(|r| r.first()).and_then(num).unwrap_or(0.0), "logs": t.and_then(|r| r.get(1)).and_then(num).unwrap_or(0.0), "metrics": t.and_then(|r| r.get(2)).and_then(num).unwrap_or(0.0) },
        "cardinality": card.to_objects(),
        "quotas": quotas.map(|q| q.0).unwrap_or(json!({})),
        "ingest": st.ingest_stats.snapshot(),
    })))
}
fn num(v: &Value) -> Option<f64> { v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())) }

pub async fn put_quotas(State(st): State<AppState>, pa: ProjectAccess, Json(q): Json<Quotas>) -> ApiResult<Json<Value>> {
    pa.require_admin()?;
    if !matches!(q.mode.as_str(), "warn" | "hard") { return Err(ApiError::BadRequest("mode must be warn or hard".into())); }
    sqlx::query("INSERT INTO project_settings (project_id, quotas) VALUES ($1, $2) ON CONFLICT (project_id) DO UPDATE SET quotas = $2, updated_at = now()").bind(pa.project.id).bind(serde_json::to_value(&q).unwrap_or_default()).execute(&st.pg).await?;
    st.resolver.invalidate_all();
    audit::project(&st.pg, &pa, "project.quotas", "project", pa.project.id, serde_json::to_value(&q).unwrap_or_default()).await;
    Ok(Json(json!({ "ok": true })))
}
