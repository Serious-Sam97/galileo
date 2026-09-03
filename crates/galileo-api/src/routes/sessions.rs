//! Browser sessions (galileo-rum): list, one session's timeline, Web Vitals by page.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::Utc;
use galileo_core::ProjectId;
use galileo_query::TimeRange;
use galileo_storage::{SqlQuery, SqlValue};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::ProjectAccess;
use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ListParams { #[serde(default = "d_last")] pub last_seconds: i64, #[serde(default)] pub user_id: String, #[serde(default = "d_limit")] pub limit: u64 }
fn d_last() -> i64 { 86_400 }
fn d_limit() -> u64 { 100 }

const RUM: &str = "mapContains(attrs, 'session.id') AND attrs['rum.type'] != ''";

fn s(v: Option<&Value>) -> String { v.and_then(|x| x.as_str()).map(str::to_owned).unwrap_or_default() }
fn f(v: Option<&Value>) -> f64 { v.and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0) }

/// Sessions seen in the window, newest first.
pub async fn list(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<ListParams>) -> ApiResult<Json<Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let mut params: Vec<SqlValue> = vec![ProjectId(pa.project.id).into(), start.into(), end.into()];
    let mut extra = String::new();
    if !p.user_id.is_empty() { extra.push_str(" AND user_id = ?"); params.push(p.user_id.clone().into()); }
    let limit = p.limit.clamp(1, 500);
    let res = st.storage.query(&SqlQuery { sql: format!(
        "SELECT attrs['session.id'] AS sid, anyLast(user_id) AS uid, any(service_name) AS svc, min(timestamp) AS first_seen, max(timestamp) AS last_seen, \
                countIf(attrs['rum.type'] IN ('pageload', 'navigation')) AS pages, countIf(attrs['rum.type'] = 'fetch') AS fetches, \
                countIf(attrs['rum.type'] = 'error') AS errors, countIf(attrs['rum.type'] = 'fetch' AND status_code = 'error') AS failed_fetches, \
                quantileIf(0.75)(toFloat64OrZero(attrs['web_vital.lcp']), attrs['web_vital.lcp'] != '') AS p75_lcp, \
                any(attrs['browser.name']) AS browser, anyLast(attrs['url.path']) AS last_path, any(attrs['url.path']) AS first_path \
         FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND {RUM}{extra} \
         GROUP BY sid ORDER BY last_seen DESC LIMIT {limit}"), params }).await?;
    let sessions: Vec<Value> = res.rows.iter().map(|r| json!({
        "session_id": s(r.first()), "user_id": s(r.get(1)), "service_name": s(r.get(2)), "first_seen": s(r.get(3)), "last_seen": s(r.get(4)),
        "pages": f(r.get(5)), "fetches": f(r.get(6)), "errors": f(r.get(7)), "failed_fetches": f(r.get(8)), "p75_lcp": f(r.get(9)),
        "browser": s(r.get(10)), "last_path": s(r.get(11)), "first_path": s(r.get(12)),
    })).collect();
    Ok(Json(json!({ "sessions": sessions, "start": start, "end": end })))
}

/// One session: every RUM span in order, each fetch carrying the backend root span of its trace.
pub async fn get(State(st): State<AppState>, pa: ProjectAccess, Path((_, sid)): Path<(uuid::Uuid, String)>) -> ApiResult<Json<Value>> {
    let pid = ProjectId(pa.project.id);
    let res = st.storage.query(&SqlQuery { sql: format!(
        "SELECT timestamp, trace_id, span_id, parent_span_id, name, duration_ns / 1000000 AS duration_ms, status_code, user_id, service_name, toJSONString(attrs) \
         FROM spans WHERE project_id = ? AND attrs['session.id'] = ? AND {RUM} ORDER BY timestamp LIMIT 2000"),
        params: vec![pid.into(), sid.clone().into()] }).await?;
    let mut events: Vec<Value> = res.rows.iter().map(|r| json!({
        "timestamp": s(r.first()), "trace_id": s(r.get(1)), "span_id": s(r.get(2)), "parent_span_id": s(r.get(3)), "name": s(r.get(4)),
        "duration_ms": f(r.get(5)), "status_code": s(r.get(6)), "user_id": s(r.get(7)), "service_name": s(r.get(8)), "attrs": serde_json::from_str::<Value>(&s(r.get(9))).unwrap_or(Value::Null),
    })).collect();
    // backend children of fetch spans: the server-side span whose parent is the browser fetch span
    let fetch_ids: Vec<String> = events.iter().filter(|e| e["attrs"]["rum.type"] == "fetch").map(|e| e["span_id"].as_str().unwrap_or("").to_string()).collect();
    if !fetch_ids.is_empty() {
        let placeholders = fetch_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut params: Vec<SqlValue> = vec![pid.into()];
        params.extend(fetch_ids.iter().map(|x| SqlValue::from(x.clone())));
        let be = st.storage.query(&SqlQuery { sql: format!(
            "SELECT parent_span_id, span_id, service_name, name, http_route, http_status_code, duration_ns / 1000000 AS duration_ms, status_code, trace_id, attrs['exception.type'] AS exc \
             FROM spans WHERE project_id = ? AND parent_span_id IN ({placeholders}) AND NOT mapContains(attrs, 'session.id') LIMIT 2000"), params }).await?;
        // DB calls under each backend span: DB spans of the same trace inside the backend span's time window
        // (a page view shares one trace across its fetches, so "per trace" would over-count).
        let be_ids: Vec<String> = be.rows.iter().map(|r| s(r.get(1))).filter(|t| !t.is_empty()).collect();
        let mut db_by_span: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        if !be_ids.is_empty() {
            let ph = be_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let mut params: Vec<SqlValue> = vec![pid.into()];
            params.extend(be_ids.iter().map(|x| SqlValue::from(x.clone())));
            params.push(pid.into());
            let db = st.storage.query(&SqlQuery { sql: format!(
                "SELECT b.span_id, count() FROM spans AS d INNER JOIN \
                   (SELECT span_id, trace_id, timestamp AS st, timestamp + toIntervalNanosecond(duration_ns) AS en FROM spans WHERE project_id = ? AND span_id IN ({ph})) AS b \
                 ON d.trace_id = b.trace_id WHERE d.project_id = ? AND d.db_table != '' AND d.timestamp >= b.st AND d.timestamp <= b.en GROUP BY b.span_id"), params }).await?;
            for r in &db.rows { db_by_span.insert(s(r.first()), f(r.get(1))); }
        }
        for r in &be.rows {
            let parent = s(r.first());
            if let Some(e) = events.iter_mut().find(|e| e["span_id"] == parent) {
                e["backend"] = json!({ "span_id": s(r.get(1)), "service_name": s(r.get(2)), "name": s(r.get(3)), "http_route": s(r.get(4)), "http_status_code": f(r.get(5)),
                                       "duration_ms": f(r.get(6)), "status_code": s(r.get(7)), "db_calls": db_by_span.get(&s(r.get(1))).copied().unwrap_or(0.0), "exception_type": s(r.get(9)) });
            }
        }
    }
    let user_id = events.iter().rev().find_map(|e| e["user_id"].as_str().filter(|u| !u.is_empty()).map(str::to_owned)).unwrap_or_default();
    let (first, last) = (events.first().map(|e| e["timestamp"].clone()).unwrap_or(Value::Null), events.last().map(|e| e["timestamp"].clone()).unwrap_or(Value::Null));
    Ok(Json(json!({ "session_id": sid, "user_id": user_id, "first_seen": first, "last_seen": last, "events": events })))
}

#[derive(Deserialize)]
pub struct VitalsParams { #[serde(default = "d_last")] pub last_seconds: i64 }

/// Web Vitals p75 overall and by page (from the `browser.web_vital.*` gauges).
pub async fn vitals(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<VitalsParams>) -> ApiResult<Json<Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let params = |extra: Vec<SqlValue>| { let mut v: Vec<SqlValue> = vec![ProjectId(pa.project.id).into(), start.into(), end.into()]; v.extend(extra); v };
    let by_page = st.storage.query(&SqlQuery { sql:
        "SELECT attrs['url.path'] AS path, name, quantile(0.75)(value) AS p75, count() AS n \
         FROM metrics WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND name LIKE 'browser.web_vital.%' \
         GROUP BY path, name ORDER BY n DESC LIMIT 400".into(), params: params(vec![]) }).await?;
    let overall = st.storage.query(&SqlQuery { sql:
        "SELECT name, quantile(0.75)(value) AS p75, count() AS n, uniq(attrs['session.id']) AS sessions \
         FROM metrics WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND name LIKE 'browser.web_vital.%' GROUP BY name".into(), params: params(vec![]) }).await?;
    let counts = st.storage.query(&SqlQuery { sql: format!(
        "SELECT uniq(attrs['session.id']) AS sessions, countIf(attrs['rum.type'] IN ('pageload','navigation')) AS pages, countIf(attrs['rum.type'] = 'error') AS errors, \
                countIf(attrs['rum.type'] = 'fetch') AS fetches, countIf(attrs['rum.type'] = 'fetch' AND status_code = 'error') AS failed_fetches, uniq(user_id) AS users \
         FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND {RUM}"), params: params(vec![]) }).await?;
    let strip = |n: &str| n.trim_start_matches("browser.web_vital.").to_string();
    let mut pages: serde_json::Map<String, Value> = serde_json::Map::new();
    for r in &by_page.rows {
        let e = pages.entry(s(r.first())).or_insert_with(|| json!({ "path": s(r.first()), "vitals": {}, "n": 0.0 }));
        e["vitals"][strip(&s(r.get(1)))] = json!(f(r.get(2)));
        if strip(&s(r.get(1))) == "lcp" || e["n"] == 0.0 { e["n"] = json!(f(r.get(3))); }
    }
    let c = counts.rows.first();
    Ok(Json(json!({
        "overall": overall.rows.iter().map(|r| json!({ "name": strip(&s(r.first())), "p75": f(r.get(1)), "n": f(r.get(2)), "sessions": f(r.get(3)) })).collect::<Vec<_>>(),
        "by_page": pages.values().cloned().collect::<Vec<_>>(),
        "counts": { "sessions": c.map(|r| f(r.first())).unwrap_or(0.0), "pages": c.map(|r| f(r.get(1))).unwrap_or(0.0), "errors": c.map(|r| f(r.get(2))).unwrap_or(0.0), "fetches": c.map(|r| f(r.get(3))).unwrap_or(0.0), "failed_fetches": c.map(|r| f(r.get(4))).unwrap_or(0.0), "users": c.map(|r| f(r.get(5))).unwrap_or(0.0) },
        "start": start, "end": end
    })))
}

/// The browser script, served by the API so one origin gives both the script and the UI.
pub async fn rum_js() -> axum::response::Response {
    use axum::response::IntoResponse;
    ([(axum::http::header::CONTENT_TYPE, "application/javascript; charset=utf-8"), (axum::http::header::CACHE_CONTROL, "public, max-age=300")], include_str!("../../../../sdk/browser/galileo-rum.js")).into_response()
}
