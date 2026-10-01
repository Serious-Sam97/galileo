//! Event-data endpoints: run queries, fetch traces, BubbleUp, field/value autocomplete.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::Utc;
use galileo_core::{ProjectId, TraceId};
use galileo_query::bubbleup::BubbleUpRequest;
use galileo_query::{fields, sql, Dataset, Query, TimeRange};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::perms::{self, Perm};

fn qerr(e: galileo_query::QueryError) -> ApiError {
    match e {
        galileo_query::QueryError::Invalid(m) => ApiError::Query(m),
        galileo_query::QueryError::Storage(s) => ApiError::Storage(s),
    }
}

/// Without `ViewSensitive`, a query may not name a sensitive field at all.
fn guard(pa: &ProjectAccess, q: &serde_json::Value) -> ApiResult<()> {
    if !pa.can(Perm::ViewSensitive) && perms::query_mentions_sensitive(q) {
        return Err(ApiError::Coded(axum::http::StatusCode::FORBIDDEN, "sensitive_field", "this query uses fields you are not allowed to see".into()));
    }
    Ok(())
}

/// Without `ViewSensitive`, raw rows lose sensitive columns and attributes.
fn redact_raw(pa: &ProjectAccess, res: &mut galileo_query::run::QueryResponse) {
    if pa.can(Perm::ViewSensitive) { return; }
    if let Some(raw) = res.raw.as_mut() {
        let hidden: Vec<usize> = raw.columns.iter().enumerate().filter(|(_, c)| perms::is_sensitive(c)).map(|(i, _)| i).collect();
        for row in raw.rows.iter_mut() {
            for &i in &hidden { if let Some(v) = row.get_mut(i) { *v = serde_json::Value::Null; } }
            row.iter_mut().for_each(perms::redact);
        }
    }
}

pub async fn run(State(st): State<AppState>, pa: ProjectAccess, Json(q): Json<serde_json::Value>) -> ApiResult<Json<galileo_query::run::QueryResponse>> {
    guard(&pa, &q)?;
    let q = Query::from_json(q).map_err(qerr)?;
    let mut res = galileo_query::run::run(st.storage.as_ref(), ProjectId(pa.project.id), &q).await.map_err(qerr)?;
    redact_raw(&pa, &mut res);
    // query history for the History drawer (best effort, deduped per minute)
    let (pg, uid, pid, qq) = (st.pg.clone(), pa.user.id, pa.project.id, q.clone());
    tokio::spawn(async move { crate::routes::boards::record_history(&pg, uid, pid, &qq).await; });
    Ok(Json(res))
}

#[derive(Deserialize)]
pub struct TracePath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub trace_id: String,
}

pub async fn trace(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<TracePath>) -> ApiResult<Json<serde_json::Value>> {
    let tid = TraceId::from_hex(p.trace_id.trim()).ok_or_else(|| ApiError::BadRequest("invalid trace id".into()))?;
    let spans = st.storage.fetch_trace(ProjectId(pa.project.id), tid).await?;
    if spans.is_empty() {
        return Err(ApiError::NotFound("trace"));
    }
    let mut view = serde_json::to_value(galileo_query::trace::assemble(tid, spans)).map_err(|e| ApiError::Internal(e.to_string()))?;
    if !pa.can(Perm::ViewSensitive) { perms::redact(&mut view); }
    Ok(Json(view))
}

pub async fn bubbleup(State(st): State<AppState>, pa: ProjectAccess, Json(req): Json<BubbleUpRequest>) -> ApiResult<Json<serde_json::Value>> {
    guard(&pa, &serde_json::to_value(&req.query).unwrap_or_default())?;
    let res = galileo_query::bubbleup::run(st.storage.as_ref(), ProjectId(pa.project.id), &req).await.map_err(qerr)?;
    let mut v = serde_json::to_value(res).map_err(|e| ApiError::Internal(e.to_string()))?;
    // BubbleUp compares every attribute; sensitive keys drop out of its answer.
    if !pa.can(Perm::ViewSensitive) {
        if let Some(keys) = v.get_mut("keys").and_then(|k| k.as_array_mut()) {
            keys.retain(|k| !k.get("key").and_then(|x| x.as_str()).is_some_and(perms::is_sensitive));
        }
    }
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct FieldsParams {
    #[serde(default)]
    pub dataset: Dataset,
    #[serde(default = "default_last")]
    pub last_seconds: i64,
}
fn default_last() -> i64 {
    86400
}

/// Static columns plus attribute keys seen in the window, with counts.
pub async fn fields(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<FieldsParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let res = st.storage.query(&sql::keys_query(p.dataset, ProjectId(pa.project.id), start, end)).await?;
    let statics = fields::static_fields(p.dataset);
    let known: std::collections::HashSet<String> = statics.iter().map(|f| f.name.clone()).collect();
    let mut out: Vec<serde_json::Value> = statics
        .iter()
        .map(|f| json!({ "name": f.name, "type": f.ty, "column": true, "count": null }))
        .collect();
    for r in res.rows {
        // attribute keys that are also hot columns are listed once, as the column
        let name = r[0].as_str().unwrap_or("").to_string();
        if known.contains(&name) || known.contains(name.trim_start_matches("resource.")) {
            continue;
        }
        out.push(json!({ "name": name, "type": "string", "column": false, "count": r[1] }));
    }
    Ok(Json(json!({ "fields": out })))
}

#[derive(Deserialize)]
pub struct ValuesParams {
    #[serde(default)]
    pub dataset: Dataset,
    pub field: String,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default = "default_last")]
    pub last_seconds: i64,
}

pub async fn values(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<ValuesParams>) -> ApiResult<Json<serde_json::Value>> {
    guard(&pa, &json!({ "field": p.field }))?;
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let sq = sql::values_query(p.dataset, &p.field, p.q.as_deref(), ProjectId(pa.project.id), start, end).map_err(qerr)?;
    let res = st.storage.query(&sq).await?;
    let values: Vec<serde_json::Value> = res.rows.into_iter().map(|r| json!({ "value": r[0], "count": r[1] })).collect();
    Ok(Json(json!({ "values": values })))
}

#[derive(Deserialize)]
pub struct MetricNamesParams {
    #[serde(default = "default_last")]
    pub last_seconds: i64,
}

/// Metric catalog for the metrics explorer.
pub async fn metric_names(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<MetricNamesParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let sq = galileo_storage::SqlQuery {
        sql: "SELECT name, any(kind) AS kind, any(unit) AS unit, any(description) AS description, uniq(service_name) AS services, count() AS points \
              FROM metrics WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) \
              GROUP BY name ORDER BY name LIMIT 2000"
            .into(),
        params: vec![ProjectId(pa.project.id).into(), start.into(), end.into()],
    };
    let res = st.storage.query(&sq).await?;
    Ok(Json(json!({ "metrics": res.to_objects() })))
}

/// Recent traces list: one row per trace (root span), newest first. Built on the raw query
/// shape with an `is_root` filter so it shares all the filter machinery.
pub async fn recent_traces(State(st): State<AppState>, pa: ProjectAccess, Json(mut q): Json<Query>) -> ApiResult<Json<galileo_query::run::QueryResponse>> {
    guard(&pa, &serde_json::to_value(&q).unwrap_or_default())?;
    q.dataset = Dataset::Spans;
    q.calculations.clear();
    q.filters.push(galileo_query::Filter::new("is_root", galileo_query::FilterOp::Eq, 1));
    let mut res = galileo_query::run::run(st.storage.as_ref(), ProjectId(pa.project.id), &q).await.map_err(qerr)?;
    redact_raw(&pa, &mut res);
    Ok(Json(res))
}

/// Service overview: request count, error rate, p50/p95/p99 per service over the window.
pub async fn services(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<MetricNamesParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let sq = galileo_storage::SqlQuery {
        sql: "SELECT service_name, count() AS spans, countIf(parent_span_id = '') AS requests, \
              countIf(status_code = 'error') AS errors, \
              quantileTDigest(0.5)(duration_ns) / 1e6 AS p50_ms, quantileTDigest(0.95)(duration_ns) / 1e6 AS p95_ms, \
              quantileTDigest(0.99)(duration_ns) / 1e6 AS p99_ms, max(timestamp) AS last_seen, \
              countIf(gen_ai_system != '') AS llm_calls, sum(gen_ai_cost_usd) AS llm_cost_usd \
              FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) \
              GROUP BY service_name ORDER BY spans DESC LIMIT 500"
            .into(),
        params: vec![ProjectId(pa.project.id).into(), start.into(), end.into()],
    };
    let res = st.storage.query(&sq).await?;
    Ok(Json(json!({ "services": res.to_objects(), "start": start, "end": end })))
}

/// Text form of the DSL → Query JSON (and back), for the keyboard editor.
#[derive(Deserialize)]
pub struct ParseBody { pub text: String }

pub async fn parse_dsl(State(_st): State<AppState>, _pa: ProjectAccess, Json(b): Json<ParseBody>) -> ApiResult<Json<serde_json::Value>> {
    match galileo_query::text::parse(&b.text) {
        Ok(q) => Ok(Json(json!({ "ok": true, "query": q, "text": galileo_query::text::stringify(&q) }))),
        Err(e) => Ok(Json(json!({ "ok": false, "error": e.to_string() }))),
    }
}

#[derive(Deserialize)]
pub struct StringifyBody { pub query: serde_json::Value }

pub async fn stringify_dsl(State(_st): State<AppState>, _pa: ProjectAccess, Json(b): Json<StringifyBody>) -> ApiResult<Json<serde_json::Value>> {
    let q = Query::from_json(b.query).map_err(qerr)?;
    Ok(Json(json!({ "text": galileo_query::text::stringify(&q) })))
}

/// Run the query and stream the result as CSV (grouped totals, or raw rows).
pub async fn export_csv(State(st): State<AppState>, pa: ProjectAccess, Json(q): Json<serde_json::Value>) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    guard(&pa, &q)?;
    let q = Query::from_json(q).map_err(qerr)?;
    let mut res = galileo_query::run::run(st.storage.as_ref(), ProjectId(pa.project.id), &q).await.map_err(qerr)?;
    redact_raw(&pa, &mut res);
    let mut out = String::new();
    let esc = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    if let Some(raw) = &res.raw {
        out.push_str(&raw.columns.iter().map(|c| esc(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
        for row in raw.rows.iter().take(50_000) {
            let cells: Vec<String> = row.iter().map(|v| esc(&match v { serde_json::Value::String(s) => s.clone(), serde_json::Value::Null => String::new(), o => o.to_string() })).collect();
            out.push_str(&cells.join(",")); out.push('\n');
        }
    } else {
        let mut header: Vec<String> = res.breakdowns.clone();
        header.extend(res.calculations.clone());
        out.push_str(&header.iter().map(|c| esc(c)).collect::<Vec<_>>().join(",")); out.push('\n');
        for g in &res.groups {
            let mut cells: Vec<String> = g.key.iter().map(|k| esc(k)).collect();
            cells.extend(g.totals.iter().map(|t| t.map(|v| format!("{v}")).unwrap_or_default()));
            out.push_str(&cells.join(",")); out.push('\n');
        }
    }
    let fname = format!("galileo-{}-{}.csv", q.dataset_name(), Utc::now().format("%Y%m%d-%H%M"));
    Ok(([(axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{fname}\""))], out).into_response())
}

/// Service map: nodes (services + db/llm/http leaves) and edges (caller → callee).
pub async fn service_map(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<RangeParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let pid = ProjectId(pa.project.id);
    // service nodes
    let nodes_q = galileo_storage::SqlQuery {
        sql: "SELECT service_name AS id, count() AS spans, countIf(status_code = 'error') AS errors, quantileTDigest(0.95)(duration_ns) / 1e6 AS p95_ms, countIf(parent_span_id = '') AS entries               FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND service_name != '' GROUP BY service_name ORDER BY spans DESC LIMIT 200".into(),
        params: vec![pid.into(), start.into(), end.into()],
    };
    // cross-service edges via parent/child join
    let edges_q = galileo_storage::SqlQuery {
        sql: "SELECT p.service_name AS src, c.service_name AS dst, count() AS calls, countIf(c.status_code = 'error') AS errors, quantileTDigest(0.95)(c.duration_ns) / 1e6 AS p95_ms               FROM spans AS c INNER JOIN spans AS p ON c.project_id = p.project_id AND c.parent_span_id = p.span_id               WHERE c.project_id = ? AND c.timestamp >= fromUnixTimestamp64Nano(?) AND c.timestamp < fromUnixTimestamp64Nano(?)                 AND p.timestamp >= fromUnixTimestamp64Nano(?) - INTERVAL 1 MINUTE AND p.timestamp < fromUnixTimestamp64Nano(?) + INTERVAL 1 MINUTE                 AND c.service_name != p.service_name AND c.service_name != '' AND p.service_name != ''               GROUP BY src, dst ORDER BY calls DESC LIMIT 500".into(),
        params: vec![pid.into(), start.into(), end.into(), start.into(), end.into()],
    };
    // db / llm leaves: client calls grouped by their target system
    let leaves_q = galileo_storage::SqlQuery {
        sql: "SELECT service_name AS src, multiIf(db_system != '', db_system, gen_ai_system != '', gen_ai_system, 'http') AS dst,                      multiIf(db_system != '', 'db', gen_ai_system != '', 'llm', 'http') AS kind,                      count() AS calls, countIf(status_code = 'error') AS errors, quantileTDigest(0.95)(duration_ns) / 1e6 AS p95_ms               FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?)                 AND (db_system != '' OR gen_ai_system != '' OR kind = 'client') AND service_name != ''               GROUP BY src, dst, kind ORDER BY calls DESC LIMIT 500".into(),
        params: vec![pid.into(), start.into(), end.into()],
    };
    let (nodes, edges, leaves) = tokio::try_join!(st.storage.query(&nodes_q), st.storage.query(&edges_q), st.storage.query(&leaves_q))?;
    Ok(Json(json!({ "nodes": nodes.to_objects(), "edges": edges.to_objects(), "leaves": leaves.to_objects(), "start": start, "end": end })))
}

#[derive(Deserialize)]
pub struct RangeParams {
    #[serde(default = "default_last")]
    pub last_seconds: i64,
}

/// N+1 candidates: statements issued many times from the same call site within one trace,
/// aggregated over the window. One row per (statement, function).
pub async fn nplusone(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<RangeParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let norm = "replaceRegexpAll(replaceRegexpAll(replaceRegexpAll(attrs['db.statement'], '\\'[^\\']*\\'', char(63)), '\\b[0-9]+(\\.[0-9]+){0,1}\\b', char(63)), '\\s+', ' ')";
    let sq = galileo_storage::SqlQuery {
        sql: format!(
            "WITH roots AS (SELECT trace_id, any(http_route) AS route FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) - INTERVAL 10 MINUTE AND timestamp < fromUnixTimestamp64Nano(?) AND parent_span_id = '' GROUP BY trace_id), \
             per_trace AS (SELECT trace_id, code_function, code_namespace, code_file, db_table, {norm} AS statement, count() AS repeats, sum(duration_ns) / 1e6 AS total_ms \
                FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND db_system != '' \
                GROUP BY trace_id, code_function, code_namespace, code_file, db_table, statement HAVING repeats >= 5) \
             SELECT statement, db_table AS table, code_function AS function, code_namespace AS namespace, code_file AS file, \
                    count() AS traces, round(avg(repeats), 1) AS avg_repeats, max(repeats) AS max_repeats, round(avg(total_ms), 1) AS avg_ms_per_trace, \
                    any(per_trace.trace_id) AS sample_trace, uniq(roots.route) AS routes, anyIf(roots.route, roots.route != '') AS sample_route \
             FROM per_trace LEFT JOIN roots ON roots.trace_id = per_trace.trace_id \
             GROUP BY statement, table, function, namespace, file ORDER BY traces DESC, avg_repeats DESC LIMIT 50"
        ),
        params: vec![ProjectId(pa.project.id).into(), start.into(), end.into(), ProjectId(pa.project.id).into(), start.into(), end.into()],
    };
    let res = st.storage.query(&sq).await?;
    Ok(Json(json!({ "start": start, "end": end, "candidates": res.to_objects() })))
}

#[derive(Deserialize)]
pub struct CallersParams {
    pub statement: String,
    #[serde(default)]
    pub table: Option<String>,
    #[serde(default = "default_last")]
    pub last_seconds: i64,
}

/// Which application functions issue a given statement (normalised), with counts and latency.
pub async fn callers(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<CallersParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let norm = galileo_query::trace::normalize_sql(&p.statement);
    let mut params: Vec<galileo_storage::SqlValue> = vec![ProjectId(pa.project.id).into(), start.into(), end.into(), norm.clone().into()];
    let mut extra = String::new();
    if let Some(t) = p.table.filter(|t| !t.is_empty()) {
        extra = " AND db_table = ?".into();
        params.push(t.into());
    }
    let sq = galileo_storage::SqlQuery {
        sql: format!(
            "SELECT code_function AS function, code_namespace AS namespace, code_file AS file, any(code_line) AS line, count() AS calls, uniq(trace_id) AS traces, \
             round(quantileTDigest(0.5)(duration_ns) / 1e6, 2) AS p50_ms, round(quantileTDigest(0.95)(duration_ns) / 1e6, 2) AS p95_ms, round(sum(duration_ns) / 1e6) AS total_ms \
             FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND db_system != '' \
             AND replaceRegexpAll(replaceRegexpAll(replaceRegexpAll(attrs['db.statement'], '\\'[^\\']*\\'', char(63)), '\\b[0-9]+(\\.[0-9]+){{0,1}}\\b', char(63)), '\\s+', ' ') = ?{extra} \
             GROUP BY function, namespace, file ORDER BY calls DESC LIMIT 30"
        ),
        params,
    };
    let res = st.storage.query(&sq).await?;
    Ok(Json(json!({ "statement": norm, "callers": res.to_objects() })))
}

#[derive(Deserialize)]
pub struct UserPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub user_id: String,
}

/// Everything one user did: requests, log lines and LLM calls, merged in time order.
pub async fn user_timeline(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<UserPath>, QueryParams(r): QueryParams<RangeParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: r.last_seconds }.resolve(Utc::now());
    let pid = ProjectId(pa.project.id);
    let spans = st.storage.query(&galileo_storage::SqlQuery {
        sql: "SELECT timestamp, 'request' AS kind, name, http_route AS route, http_status_code AS status, status_code, duration_ns / 1e6 AS duration_ms, trace_id, tenant_id, service_name, '' AS body, '' AS severity, gen_ai_model AS model, gen_ai_cost_usd AS cost_usd \
              FROM spans WHERE project_id = ? AND user_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND (parent_span_id = '' OR gen_ai_system != '') ORDER BY timestamp DESC LIMIT 500".into(),
        params: vec![pid.into(), p.user_id.clone().into(), start.into(), end.into()],
    }).await?;
    let logs = st.storage.query(&galileo_storage::SqlQuery {
        sql: "SELECT timestamp, 'log' AS kind, '' AS name, attrs['http.route'] AS route, 0 AS status, '' AS status_code, 0 AS duration_ms, trace_id, tenant_id, service_name, body, severity, '' AS model, 0 AS cost_usd \
              FROM logs WHERE project_id = ? AND user_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) ORDER BY timestamp DESC LIMIT 500".into(),
        params: vec![pid.into(), p.user_id.clone().into(), start.into(), end.into()],
    }).await?;
    let mut events: Vec<serde_json::Map<String, serde_json::Value>> = spans.to_objects();
    events.extend(logs.to_objects());
    events.sort_by_key(|e| std::cmp::Reverse(e.get("timestamp").and_then(|v| v.as_str()).map(str::to_owned)));
    let summary = st.storage.query(&galileo_storage::SqlQuery {
        sql: "SELECT countIf(parent_span_id = '') AS requests, countIf(parent_span_id = '' AND status_code = 'error') AS errors, countIf(gen_ai_system != '') AS llm_calls, sum(gen_ai_cost_usd) AS llm_cost_usd, \
              uniq(tenant_id) AS tenants, anyIf(tenant_id, tenant_id != '') AS main_tenant, min(timestamp) AS first_seen, max(timestamp) AS last_seen, uniq(http_route) AS routes \
              FROM spans WHERE project_id = ? AND user_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?)".into(),
        params: vec![pid.into(), p.user_id.clone().into(), start.into(), end.into()],
    }).await?;
    Ok(Json(json!({ "user_id": p.user_id, "start": start, "end": end, "summary": summary.to_objects().into_iter().next(), "events": events })))
}
