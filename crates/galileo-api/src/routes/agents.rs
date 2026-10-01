//! Agent runs: gateway calls grouped by `gen_ai.conversation.id`, with the app's tool spans.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::Utc;
use galileo_core::ProjectId;
use galileo_query::TimeRange;
use galileo_storage::{SqlQuery, SqlValue};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::ProjectAccess;
use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ListParams { #[serde(default = "d_last")] pub last_seconds: i64, #[serde(default)] pub user_id: String, #[serde(default = "d_limit")] pub limit: u64 }
fn d_last() -> i64 { 86_400 }
fn d_limit() -> u64 { 100 }

fn s(v: Option<&Value>) -> String { v.and_then(|x| x.as_str()).map(str::to_owned).unwrap_or_default() }
fn f(v: Option<&Value>) -> f64 { v.and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0) }

const CONV: &str = "attrs['gen_ai.conversation.id']";

/// `gen_ai.tool_calls` is a comma-joined list of tool names (or JSON from newer SDKs).
fn parse_tools(raw: &str) -> Value {
    if raw.is_empty() { return Value::Null; }
    serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!(raw.split(',').map(|x| json!({ "name": x.trim() })).collect::<Vec<_>>()))
}

pub async fn list(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<ListParams>) -> ApiResult<Json<Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let mut params: Vec<SqlValue> = vec![ProjectId(pa.project.id).into(), start.into(), end.into()];
    let mut extra = String::new();
    if !p.user_id.is_empty() { extra.push_str(" AND user_id = ?"); params.push(p.user_id.clone().into()); }
    let limit = p.limit.clamp(1, 500);
    let res = st.storage.query(&SqlQuery { sql: format!(
        "SELECT {CONV} AS cid, anyLast(user_id) AS uid, count() AS turns, sum(attrs['gen_ai.tool_calls'] != '' AND attrs['gen_ai.tool_calls'] != '[]') AS tool_turns, \
                sum(gen_ai_input_tokens) AS in_tok, sum(gen_ai_output_tokens) AS out_tok, sum(gen_ai_cost_usd) AS cost, countIf(status_code = 'error') AS errors, \
                min(timestamp) AS first_seen, max(timestamp) AS last_seen, any(gen_ai_model) AS model, anyLast(attrs['gen_ai.galileo.route']) AS route, \
                sum(duration_ns) / 1e6 AS model_ms, uniq(trace_id) AS traces \
         FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND gen_ai_system != '' AND {CONV} != ''{extra} \
         GROUP BY cid ORDER BY last_seen DESC LIMIT {limit}"), params }).await?;
    let runs: Vec<Value> = res.rows.iter().map(|r| json!({
        "conversation_id": s(r.first()), "user_id": s(r.get(1)), "turns": f(r.get(2)), "tool_turns": f(r.get(3)), "input_tokens": f(r.get(4)), "output_tokens": f(r.get(5)), "cost_usd": f(r.get(6)),
        "errors": f(r.get(7)), "first_seen": s(r.get(8)), "last_seen": s(r.get(9)), "model": s(r.get(10)), "route": s(r.get(11)), "model_ms": f(r.get(12)), "traces": f(r.get(13)),
    })).collect();
    Ok(Json(json!({ "runs": runs, "start": start, "end": end })))
}

pub async fn get(State(st): State<AppState>, pa: ProjectAccess, Path((_, cid)): Path<(Uuid, String)>) -> ApiResult<Json<Value>> {
    let pid = ProjectId(pa.project.id);
    let turns = st.storage.query(&SqlQuery { sql: format!(
        "SELECT timestamp, trace_id, span_id, parent_span_id, duration_ns / 1e6, status_code, user_id, gen_ai_model, gen_ai_input_tokens, gen_ai_output_tokens, gen_ai_cost_usd, \
                attrs['gen_ai.galileo.route'], attrs['gen_ai.tool_calls'], attrs['gen_ai.prompt'], attrs['gen_ai.completion'], attrs['gen_ai.response.finish_reasons'], \
                attrs['gen_ai.galileo.cache_hit'], attrs['gen_ai.galileo.guardrail'], attrs['gen_ai.galileo.time_to_first_token_ms'], attrs['gen_ai.galileo.fallback_index'] \
         FROM spans WHERE project_id = ? AND {CONV} = ? AND gen_ai_system != '' ORDER BY timestamp LIMIT 500"), params: vec![pid.into(), cid.clone().into()] }).await?;
    let mut out: Vec<Value> = turns.rows.iter().map(|r| json!({
        "timestamp": s(r.first()), "trace_id": s(r.get(1)), "span_id": s(r.get(2)), "parent_span_id": s(r.get(3)), "duration_ms": f(r.get(4)), "status_code": s(r.get(5)), "user_id": s(r.get(6)),
        "model": s(r.get(7)), "input_tokens": f(r.get(8)), "output_tokens": f(r.get(9)), "cost_usd": f(r.get(10)), "route": s(r.get(11)),
        "tool_calls": parse_tools(&s(r.get(12))), "prompt": s(r.get(13)).chars().take(1500).collect::<String>(), "completion": s(r.get(14)).chars().take(1500).collect::<String>(),
        "finish_reason": s(r.get(15)), "cache_hit": s(r.get(16)), "guardrail": s(r.get(17)), "ttft_ms": f(r.get(18)), "fallback_index": f(r.get(19)), "app_spans": [],
    })).collect();
    // Prompts, completions and tool arguments are sensitive.
    if !pa.can(crate::perms::Perm::ViewSensitive) {
        for t in out.iter_mut() {
            t["prompt"] = Value::Null;
            t["completion"] = Value::Null;
            t["tool_calls"] = Value::Null;
            t["redacted"] = Value::Bool(true);
        }
    }
    // tool executions and other app spans from the same traces (anything not the gateway itself)
    let trace_ids: Vec<String> = { let mut v: Vec<String> = out.iter().map(|t| t["trace_id"].as_str().unwrap_or("").to_string()).filter(|t| !t.is_empty()).collect(); v.sort(); v.dedup(); v };
    if !trace_ids.is_empty() {
        let ph = trace_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut params: Vec<SqlValue> = vec![pid.into()];
        params.extend(trace_ids.iter().map(|t| SqlValue::from(t.clone())));
        let app = st.storage.query(&SqlQuery { sql: format!(
            "SELECT timestamp, trace_id, span_id, parent_span_id, name, kind, service_name, duration_ns / 1e6, status_code, db_table, code_function, attrs['exception.type'] \
             FROM spans WHERE project_id = ? AND trace_id IN ({ph}) AND gen_ai_system = '' AND service_name != 'galileo-gateway' ORDER BY timestamp LIMIT 2000"), params }).await?;
        for r in &app.rows {
            let span = json!({ "timestamp": s(r.first()), "trace_id": s(r.get(1)), "span_id": s(r.get(2)), "parent_span_id": s(r.get(3)), "name": s(r.get(4)), "kind": s(r.get(5)), "service": s(r.get(6)), "duration_ms": f(r.get(7)), "status_code": s(r.get(8)), "table": s(r.get(9)), "function": s(r.get(10)), "exception": s(r.get(11)) });
            // attach to the last turn in the same trace that started before this span
            let ts = s(r.first());
            if let Some(t) = out.iter_mut().rev().find(|t| t["trace_id"] == span["trace_id"] && t["timestamp"].as_str().unwrap_or("") <= ts.as_str()) {
                if let Some(arr) = t["app_spans"].as_array_mut() { if arr.len() < 200 { arr.push(span); } }
            } else if let Some(t) = out.iter_mut().find(|t| t["trace_id"] == span["trace_id"]) {
                if let Some(arr) = t["app_spans"].as_array_mut() { if arr.len() < 200 { arr.push(span); } }
            }
        }
    }
    let totals = json!({ "turns": out.len(), "cost_usd": out.iter().map(|t| t["cost_usd"].as_f64().unwrap_or(0.0)).sum::<f64>(), "input_tokens": out.iter().map(|t| t["input_tokens"].as_f64().unwrap_or(0.0)).sum::<f64>(), "output_tokens": out.iter().map(|t| t["output_tokens"].as_f64().unwrap_or(0.0)).sum::<f64>(), "errors": out.iter().filter(|t| t["status_code"] == "error").count(), "tool_calls": out.iter().filter(|t| t["tool_calls"].as_array().map(|a| !a.is_empty()).unwrap_or(false)).count() });
    let user_id = out.iter().rev().find_map(|t| t["user_id"].as_str().filter(|u| !u.is_empty()).map(str::to_owned)).unwrap_or_default();
    Ok(Json(json!({ "conversation_id": cid, "user_id": user_id, "totals": totals, "turns": out })))
}
