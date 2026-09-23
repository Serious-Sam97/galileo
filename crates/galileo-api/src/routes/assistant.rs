//! Ask Galileo: an assistant that turns questions into DSL queries, explains traces and writes
//! root-cause notes. It talks to a gateway route chosen at org level and runs read-only tools
//! under the caller's project access, so it can never see more than the user can.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::Utc;
use galileo_core::ProjectId;
use galileo_query::TimeRange;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::routes::{org, query};
use crate::state::AppState;

const TOOL_RESULT_CAP: usize = 4_000;
const DSL_CHEATSHEET: &str = r#"Query text DSL (pipe form). First stage is the dataset: spans | logs | metrics.
Stages: `where <field> <op> <value> [and|or ...]` (ops: = != > >= < <= ~ !~ ^ exists not_exists; quote values with spaces),
`<agg>(field), ... [by field, ...]` (aggs: count() count_distinct(f) sum(f) avg(f) min(f) max(f) p50(f) p75(f) p90(f) p95(f) p99(f) heatmap(f) rate()),
`having <agg> <op> <number>`, `order <agg|field> asc|desc`, `limit N`, `search "text"`, `compare previous`, `derive name = <arithmetic on aggs>`.
Time range is passed separately (last_seconds). Omitting aggregations returns raw rows (newest first).
Useful span fields: service_name, name, kind, duration_ms, status_code (error|ok|unset), http_route, http_method, http_status_code, url_path, user_id, tenant_id, request_id,
db_system, db_table, db_operation, code_function, code_namespace, code_file, exception_type, exception_message, exception_culprit, gen_ai_model, gen_ai_system, gen_ai.usage.cost_usd, gen_ai.galileo.route, trace_id, span_id, parent_span_id (empty = root), is_error, session.id, rum.type, url.path.
Log fields: severity, body, service_name, user_id, trace_id. Metric fields: name, value, mean, hist_p95.
Examples:
- spans | where parent_span_id = "" and service_name = melea-api | p95(duration_ms), count() by http_route | order p95 desc | limit 10
- spans | where status_code = error | count() by exception_type, http_route | limit 20
- spans | where db_system exists | count() by code_function, db_table | order count desc | limit 15
- logs | where severity = ERROR | count() by service_name
- spans | where gen_ai_system exists | sum(gen_ai.usage.cost_usd), count() by gen_ai_model"#;

#[derive(Deserialize)]
pub struct ChatMsg { pub role: String, pub content: String }

#[derive(Deserialize, Default)]
pub struct PageContext {
    #[serde(default)] pub page: String,
    #[serde(default)] pub trace_id: Option<String>,
    #[serde(default)] pub issue_id: Option<Uuid>,
    #[serde(default)] pub trigger_id: Option<Uuid>,
    #[serde(default)] pub session_id: Option<String>,
    #[serde(default)] pub query: Option<Value>,
    #[serde(default = "d_last")] pub last_seconds: i64,
}
fn d_last() -> i64 { 86_400 }

#[derive(Deserialize)]
pub struct ChatBody { pub messages: Vec<ChatMsg>, #[serde(default)] pub context: PageContext }

#[derive(Serialize, Clone)]
pub struct Step { pub action: String, pub summary: String, #[serde(skip_serializing_if = "Option::is_none")] pub query_text: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub query: Option<Value>, #[serde(skip_serializing_if = "Option::is_none")] pub link: Option<String> }

/// The model's reply is a single JSON object; accept fences and surrounding prose.
pub fn parse_action(text: &str) -> Option<Value> {
    let t = text.trim();
    let candidates: Vec<&str> = {
        let mut v = vec![t];
        if let Some(i) = t.find("```") {
            let rest = &t[i + 3..];
            let rest = rest.strip_prefix("json").unwrap_or(rest);
            if let Some(j) = rest.find("```") { v.insert(0, rest[..j].trim()); }
        }
        v
    };
    for c in &candidates {
        if let Ok(v) = serde_json::from_str::<Value>(c) { if v.get("action").is_some() { return Some(v); } }
        if let (Some(i), Some(j)) = (c.find('{'), c.rfind('}')) {
            if let Ok(v) = serde_json::from_str::<Value>(&c[i..=j]) { if v.get("action").is_some() { return Some(v); } }
        }
    }
    // Models often emit raw newlines inside the markdown string, which is invalid JSON. Recover the
    // answer text by hand so the user does not see a JSON blob.
    for c in &candidates {
        if c.contains("\"action\"") && c.contains("answer") {
            if let Some(md) = lenient_markdown(c) { return Some(json!({ "action": "answer", "markdown": md })); }
        }
    }
    None
}

fn lenient_markdown(c: &str) -> Option<String> {
    let key = c.find("\"markdown\"")?;
    let colon = c[key..].find(':')? + key;
    let start = c[colon..].find('"')? + colon + 1;
    // the value ends at the last quote that is followed by optional whitespace and a closing brace
    let end = c.rfind('}').and_then(|b| c[..b].rfind('"'))?;
    if end <= start { return None; }
    let raw = &c[start..end];
    Some(raw.replace("\\n", "\n").replace("\\\"", "\"").replace("\\t", "\t"))
}

pub fn cap(s: String, n: usize) -> String {
    if s.chars().count() <= n { s } else { let mut t: String = s.chars().take(n).collect(); t.push_str("\n…(truncated)"); t }
}

/// Strip content that must not reach the model regardless of the query.
fn scrub(mut v: Value) -> Value {
    fn walk(v: &mut Value) {
        match v {
            Value::Object(m) => { for k in ["gen_ai.prompt", "gen_ai.completion", "db.query.parameters", "exception.stacktrace", "sql"] { m.remove(k); } for x in m.values_mut() { walk(x); } }
            Value::Array(a) => for x in a { walk(x); },
            _ => {}
        }
    }
    walk(&mut v);
    v
}

async fn grounding(st: &AppState, pa: &ProjectAccess, ctx: &PageContext) -> String {
    let pid = ProjectId(pa.project.id);
    let (start, end) = TimeRange::Relative { last_seconds: 86_400 }.resolve(Utc::now());
    let services = st.storage.query(&galileo_storage::SqlQuery { sql: "SELECT service_name, count() FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) GROUP BY service_name ORDER BY 2 DESC LIMIT 15".into(), params: vec![pid.into(), start.into(), end.into()] }).await.ok();
    let routes = st.storage.query(&galileo_storage::SqlQuery { sql: "SELECT http_route, count(), quantileTDigest(0.95)(duration_ns)/1e6 FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND http_route != '' AND parent_span_id = '' GROUP BY http_route ORDER BY 2 DESC LIMIT 25".into(), params: vec![pid.into(), start.into(), end.into()] }).await.ok();
    let attrs = st.storage.query(&galileo_query::sql::keys_query(galileo_query::Dataset::Spans, pid, start, end)).await.ok();
    let fmt = |r: &Option<galileo_storage::QueryResult>, f: &dyn Fn(&Vec<Value>) -> String| r.as_ref().map(|x| x.rows.iter().map(f).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let s = |v: Option<&Value>| v.map(|x| match x { Value::String(s) => s.clone(), o => o.to_string() }).unwrap_or_default();
    let mut g = format!("Project: {} (id {}). Now: {}.\nServices (24h, spans): {}\nTop routes (24h: route, requests, p95 ms): {}\nAttribute keys seen (24h): {}\n",
        pa.project.name, pa.project.id, Utc::now().to_rfc3339(),
        fmt(&services, &|r| format!("{} ({})", s(r.first()), s(r.get(1)))),
        fmt(&routes, &|r| format!("{} ({}, {:.0})", s(r.first()), s(r.get(1)), r.get(2).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|t| t.parse().ok()))).unwrap_or(0.0))),
        attrs.as_ref().map(|x| x.rows.iter().take(80).map(|r| s(r.first())).collect::<Vec<_>>().join(", ")).unwrap_or_default());
    g.push_str(&format!("Page: {}", if ctx.page.is_empty() { "unknown" } else { &ctx.page }));
    if let Some(t) = &ctx.trace_id { g.push_str(&format!(" · open trace {t}")); }
    if let Some(i) = &ctx.issue_id { g.push_str(&format!(" · open issue {i}")); }
    if let Some(t) = &ctx.trigger_id { g.push_str(&format!(" · open trigger {t}")); }
    if let Some(sid) = &ctx.session_id { g.push_str(&format!(" · open browser session {sid}")); }
    if let Some(q) = &ctx.query { if let Ok(qq) = galileo_query::Query::from_json(q.clone()) { g.push_str(&format!(" · current query: {}", galileo_query::text::stringify(&qq))); } }
    g.push_str(&format!("\nDefault time window: last {} seconds.\n", ctx.last_seconds));
    g
}

fn system_prompt(g: &str, max_steps: u32) -> String {
    format!(r#"You are Galileo's assistant inside an observability tool (traces, logs, metrics, LLM gateway, browser sessions). You answer questions about THIS project's data by running read-only tools, then explain findings concretely with numbers, names of routes/functions/tables, and next steps.

Reply with EXACTLY ONE JSON object per turn and nothing else. Actions:
{{"action":"run_query","text":"<DSL text>","last_seconds":86400,"why":"<short>"}}
{{"action":"get_trace","trace_id":"<hex>"}}
{{"action":"bubbleup","text":"<DSL raw-mode text, no aggregations>","selection":[{{"field":"duration_ms","op":"gt","value":1000}}],"last_seconds":86400}}
{{"action":"list_issues","status":"open"}}
{{"action":"get_issue","issue_id":"<uuid>"}}
{{"action":"nplusone","last_seconds":86400}}
{{"action":"deploys","last_seconds":604800}}
{{"action":"answer","markdown":"<final answer in markdown>"}}

Rules: at most {max_steps} tool actions, then you must answer. Prefer one good query over many. When a route or function is slow, check nplusone and the p95 by code_function/db_table. Never invent numbers; if a tool returned nothing, say so. Keep answers under 250 words, use a short list, mention the query you ran. Do not include prompts/completions or secrets in answers.

{}

{g}"#, DSL_CHEATSHEET)
}

async fn run_tool(st: &AppState, pa: &ProjectAccess, pid_url: Uuid, act: &Value, default_last: i64) -> (Value, Step) {
    let a = act.get("action").and_then(|x| x.as_str()).unwrap_or("");
    let last = act.get("last_seconds").and_then(|x| x.as_i64()).unwrap_or(default_last).clamp(60, 90 * 86_400);
    match a {
        "run_query" | "bubbleup" => {
            let text = act.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let mut q = match galileo_query::text::parse(&text) { Ok(q) => q, Err(e) => return (json!({ "error": format!("bad DSL: {e}") }), Step { action: a.into(), summary: format!("could not parse: {e}"), query_text: Some(text), query: None, link: None }) };
            q.time_range = TimeRange::Relative { last_seconds: last };
            if q.limit.map(|l| l > 50).unwrap_or(true) { q.limit = Some(if q.is_raw() { 20 } else { 25 }); }
            let qv = serde_json::to_value(&q).unwrap_or(Value::Null);
            let link = Some(format!("/p/{pid_url}/query?q={}", base64_url(&qv)));
            if a == "bubbleup" {
                let selection: Vec<galileo_query::Filter> = act.get("selection").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                if selection.is_empty() { return (json!({ "error": "bubbleup needs a selection" }), Step { action: a.into(), summary: "missing selection".into(), query_text: Some(text), query: Some(qv), link }); }
                let req = galileo_query::bubbleup::BubbleUpRequest { query: q, selection, max_keys: 8, max_values: 4 };
                return match galileo_query::bubbleup::run(st.storage.as_ref(), ProjectId(pa.project.id), &req).await {
                    Ok(r) => { let v = scrub(serde_json::to_value(&r).unwrap_or(Value::Null)); (v, Step { action: a.into(), summary: "BubbleUp".into(), query_text: Some(text), query: Some(qv), link }) }
                    Err(e) => (json!({ "error": e.to_string() }), Step { action: a.into(), summary: e.to_string(), query_text: Some(text), query: Some(qv), link }),
                };
            }
            match galileo_query::run::run(st.storage.as_ref(), ProjectId(pa.project.id), &q).await {
                Ok(r) => {
                    // compact: groups → rows of key + totals; raw → columns + rows (scrubbed)
                    let v = if let Some(raw) = &r.raw {
                        let keep: Vec<usize> = raw.columns.iter().enumerate().filter(|(_, c)| !matches!(c.as_str(), "attrs" | "resource" | "events" | "project_id")).map(|(i, _)| i).collect();
                        json!({ "mode": "raw", "columns": keep.iter().map(|&i| raw.columns[i].clone()).collect::<Vec<_>>(), "rows": raw.rows.iter().take(20).map(|row| keep.iter().map(|&i| row.get(i).cloned().unwrap_or(Value::Null)).collect::<Vec<_>>()).collect::<Vec<_>>(), "total_rows": raw.rows.len() })
                    } else {
                        json!({ "mode": "groups", "breakdowns": r.breakdowns, "calculations": r.calculations, "groups": r.groups.iter().take(25).map(|g| json!({ "key": g.key, "totals": g.totals.iter().map(|t| t.map(|x| (x * 100.0).round() / 100.0)).collect::<Vec<_>>(), "compare_totals": g.compare_totals })).collect::<Vec<_>>(), "start": r.start, "end": r.end })
                    };
                    (scrub(v), Step { action: a.into(), summary: format!("{} groups/rows", r.groups.len().max(r.raw.as_ref().map(|x| x.rows.len()).unwrap_or(0))), query_text: Some(galileo_query::text::stringify(&q)), query: Some(qv), link })
                }
                Err(e) => (json!({ "error": e.to_string() }), Step { action: a.into(), summary: e.to_string(), query_text: Some(text), query: Some(qv), link }),
            }
        }
        "get_trace" => {
            let tid = act.get("trace_id").and_then(|x| x.as_str()).unwrap_or("");
            let Some(t) = galileo_core::TraceId::from_hex(tid.trim()) else { return (json!({ "error": "invalid trace id" }), Step { action: a.into(), summary: "invalid trace id".into(), query_text: None, query: None, link: None }) };
            match st.storage.fetch_trace(ProjectId(pa.project.id), t).await {
                Ok(spans) if !spans.is_empty() => { let v = trace_summary(&galileo_query::trace::assemble(t, spans)); (v, Step { action: a.into(), summary: format!("trace {}", &tid[..tid.len().min(8)]), query_text: None, query: None, link: Some(format!("/p/{pid_url}/traces/{tid}")) }) }
                Ok(_) => (json!({ "error": "trace not found" }), Step { action: a.into(), summary: "trace not found".into(), query_text: None, query: None, link: None }),
                Err(e) => (json!({ "error": e.to_string() }), Step { action: a.into(), summary: e.to_string(), query_text: None, query: None, link: None }),
            }
        }
        "list_issues" => {
            let status = act.get("status").and_then(|x| x.as_str()).unwrap_or("open").to_string();
            let rows: Vec<(Value,)> = sqlx::query_as("SELECT row_to_json(i) FROM (SELECT id, title, exception_type, culprit, route, service_name, status, count, users, first_seen, last_seen, last_trace_id FROM issues WHERE project_id = $1 AND status = $2 ORDER BY last_seen DESC LIMIT 15) i").bind(pa.project.id).bind(&status).fetch_all(&st.pg).await.unwrap_or_default();
            (json!({ "issues": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() }), Step { action: a.into(), summary: format!("{status} issues"), query_text: None, query: None, link: Some(format!("/p/{pid_url}/issues")) })
        }
        "get_issue" => {
            let id = act.get("issue_id").and_then(|x| x.as_str()).and_then(|s| Uuid::parse_str(s).ok());
            let Some(id) = id else { return (json!({ "error": "invalid issue id" }), Step { action: a.into(), summary: "invalid issue id".into(), query_text: None, query: None, link: None }) };
            let row: Option<(Value,)> = sqlx::query_as("SELECT row_to_json(i) FROM (SELECT id, title, exception_type, culprit, route, service_name, status, count, users, first_seen, last_seen, last_trace_id, last_version, notes FROM issues WHERE project_id = $1 AND id = $2) i").bind(pa.project.id).bind(id).fetch_optional(&st.pg).await.unwrap_or(None);
            let events: Vec<(Value,)> = sqlx::query_as("SELECT row_to_json(e) FROM (SELECT kind, message, at FROM issue_events WHERE issue_id = $1 ORDER BY at DESC LIMIT 10) e").bind(id).fetch_all(&st.pg).await.unwrap_or_default();
            (json!({ "issue": row.map(|r| r.0), "events": events.into_iter().map(|e| e.0).collect::<Vec<_>>() }), Step { action: a.into(), summary: "issue".into(), query_text: None, query: None, link: Some(format!("/p/{pid_url}/issues/{id}")) })
        }
        "nplusone" => {
            let r = query::nplusone(State(st.clone()), pa.clone(), QueryParams(query::RangeParams { last_seconds: last })).await;
            match r { Ok(Json(v)) => (scrub(v), Step { action: a.into(), summary: "N+1 candidates".into(), query_text: None, query: None, link: Some(format!("/p/{pid_url}/traces")) }), Err(_) => (json!({ "error": "nplusone failed" }), Step { action: a.into(), summary: "failed".into(), query_text: None, query: None, link: None }) }
        }
        "deploys" => {
            let rows: Vec<(Value,)> = sqlx::query_as("SELECT row_to_json(d) FROM (SELECT version, service, at, note FROM deploys WHERE project_id = $1 AND at > now() - make_interval(secs => $2) ORDER BY at DESC LIMIT 20) d").bind(pa.project.id).bind(last as f64).fetch_all(&st.pg).await.unwrap_or_default();
            (json!({ "deploys": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() }), Step { action: a.into(), summary: "deploys".into(), query_text: None, query: None, link: None })
        }
        other => (json!({ "error": format!("unknown action '{other}'") }), Step { action: other.into(), summary: "unknown action".into(), query_text: None, query: None, link: None }),
    }
}

fn base64_url(v: &Value) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE.encode(v.to_string())
}

fn attr_str(v: &galileo_core::AttributeValue) -> String {
    match serde_json::to_value(v) { Ok(Value::String(s)) => s, Ok(o) => o.to_string(), Err(_) => String::new() }
}

/// Compact, model-friendly view of a trace: critical spans, repeated queries, errors.
pub fn trace_summary(t: &galileo_query::trace::TraceView) -> Value {
    let mut spans: Vec<&galileo_query::trace::SpanNode> = t.spans.iter().collect();
    spans.sort_by(|a, b| b.duration_ms.partial_cmp(&a.duration_ms).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<Value> = spans.iter().take(15).map(|s| json!({
        "name": s.span.name, "service": s.span.service_name, "kind": format!("{:?}", s.span.kind).to_lowercase(), "duration_ms": (s.duration_ms * 10.0).round() / 10.0, "offset_ms": s.offset_ms.round(), "depth": s.depth,
        "status": format!("{:?}", s.span.status.code).to_lowercase(),
        "route": s.span.attributes.get("http.route").map(attr_str), "function": s.span.attributes.get("code.function.name").map(attr_str), "table": s.span.attributes.get("db.sql.table").or(s.span.attributes.get("db.table")).map(attr_str),
        "exception": s.span.attributes.get("exception.type").map(attr_str),
    })).collect();
    let errors: Vec<Value> = t.spans.iter().filter(|s| format!("{:?}", s.span.status.code).to_lowercase() == "error").take(10).map(|s| json!({ "name": s.span.name, "message": s.span.status.message, "exception": s.span.attributes.get("exception.type").map(attr_str), "exception_message": s.span.attributes.get("exception.message").map(attr_str) })).collect();
    let by_kind = {
        let mut m: std::collections::BTreeMap<String, (usize, f64)> = Default::default();
        for s in &t.spans { let k = if s.span.attributes.contains_key("db.system") || s.span.attributes.contains_key("db.system.name") { "db" } else if s.span.attributes.contains_key("gen_ai.system") { "llm" } else { "other" }; let e = m.entry(k.into()).or_default(); e.0 += 1; e.1 += s.duration_ms; }
        m.into_iter().map(|(k, (n, ms))| json!({ "kind": k, "spans": n, "total_ms": ms.round() })).collect::<Vec<_>>()
    };
    json!({
        "trace_id": t.trace_id.to_hex(), "root": t.root_name, "duration_ms": t.duration_ms.round(), "span_count": t.span_count, "error_count": t.error_count, "services": t.services,
        "db_calls": t.db_calls, "db_ms": t.db_ms.round(), "llm_calls": t.llm_calls, "llm_cost_usd": t.llm_cost_usd,
        "repeated_queries": t.repeated_queries.iter().take(5).map(|r| json!({ "statement": cap(r.statement.clone(), 200), "table": r.table, "function": r.function, "namespace": r.namespace, "count": r.count, "total_ms": r.total_ms.round() })).collect::<Vec<_>>(),
        "slowest_spans": top, "errors": errors, "by_kind": by_kind,
    })
}

async fn settings_or_err(st: &AppState, pa: &ProjectAccess) -> ApiResult<org::AssistantSettings> {
    let s = org::assistant_settings_for(&st.pg, pa.project.org_id).await;
    if !s.enabled || s.project_id.is_none() || s.route_alias.is_empty() { return Err(ApiError::BadRequest("assistant is not enabled for this organization (Settings → Organization → Assistant)".into())); }
    Ok(s)
}

pub async fn chat(State(st): State<AppState>, pa: ProjectAccess, Path(pid): Path<Uuid>, Json(b): Json<ChatBody>) -> ApiResult<Json<Value>> {
    let s = settings_or_err(&st, &pa).await?;
    let rec_project = s.project_id.unwrap();
    let g = grounding(&st, &pa, &b.context).await;
    let mut convo: Vec<(String, String)> = vec![("system".into(), system_prompt(&g, s.max_steps))];
    for m in b.messages.iter().rev().take(12).rev() { convo.push((if m.role == "assistant" { "assistant".into() } else { "user".into() }, cap(m.content.clone(), 4000))); }
    let mut steps: Vec<Step> = vec![];
    let mut usage = (0u64, 0u64, 0.0f64);
    let (mut span_id, mut trace_id, mut model) = (String::new(), String::new(), String::new());
    for _ in 0..=s.max_steps {
        let r = st.gateway.chat_recorded(rec_project, &s.route_alias, "assistant", &convo, 1200, 0.1, Some(pa.user.email.clone())).await.map_err(|e| ApiError::BadRequest(format!("assistant model call failed: {e}")))?;
        usage.0 += r.input_tokens; usage.1 += r.output_tokens; usage.2 += r.cost_usd; span_id = r.span_id.clone(); trace_id = r.trace_id.clone(); model = r.model.clone();
        let Some(act) = parse_action(&r.text) else {
            // not JSON: treat the text as the answer
            return Ok(Json(json!({ "answer": r.text, "steps": steps, "recording": { "project_id": rec_project, "span_id": span_id, "trace_id": trace_id, "model": model }, "usage": { "input_tokens": usage.0, "output_tokens": usage.1, "cost_usd": usage.2 } })));
        };
        if act.get("action").and_then(|x| x.as_str()) == Some("answer") {
            let md = act.get("markdown").and_then(|x| x.as_str()).unwrap_or("").to_string();
            return Ok(Json(json!({ "answer": md, "steps": steps, "recording": { "project_id": rec_project, "span_id": span_id, "trace_id": trace_id, "model": model }, "usage": { "input_tokens": usage.0, "output_tokens": usage.1, "cost_usd": usage.2 } })));
        }
        if steps.len() as u32 >= s.max_steps {
            convo.push(("assistant".into(), r.text.clone()));
            convo.push(("user".into(), "Tool budget exhausted. Reply now with {\"action\":\"answer\",...} using what you have.".into()));
            continue;
        }
        let (result, step) = run_tool(&st, &pa, pid, &act, b.context.last_seconds).await;
        steps.push(step);
        convo.push(("assistant".into(), r.text.clone()));
        convo.push(("user".into(), format!("TOOL RESULT ({}):\n{}", act.get("action").and_then(|x| x.as_str()).unwrap_or(""), cap(result.to_string(), TOOL_RESULT_CAP))));
    }
    Ok(Json(json!({ "answer": "I ran out of steps before reaching an answer. Try a narrower question.", "steps": steps, "recording": { "project_id": rec_project, "span_id": span_id, "trace_id": trace_id, "model": model }, "usage": { "input_tokens": usage.0, "output_tokens": usage.1, "cost_usd": usage.2 } })))
}

#[derive(Deserialize)]
pub struct InvestigateBody { #[serde(default)] pub issue_id: Option<Uuid>, #[serde(default)] pub trigger_id: Option<Uuid>, #[serde(default = "d_last")] pub last_seconds: i64 }

/// Deterministic evidence pack + one model call → root-cause write-up (saved to the issue notes).
pub async fn investigate(State(st): State<AppState>, pa: ProjectAccess, Path(pid): Path<Uuid>, Json(b): Json<InvestigateBody>) -> ApiResult<Json<Value>> {
    let s = settings_or_err(&st, &pa).await?;
    let rec_project = s.project_id.unwrap();
    let mut evidence = serde_json::Map::new();
    let subject: String;
    let mut filters: Vec<galileo_query::Filter> = vec![];
    if let Some(id) = b.issue_id {
        let row: Option<(Value,)> = sqlx::query_as("SELECT row_to_json(i) FROM (SELECT id, title, exception_type, culprit, route, service_name, status, count, users, first_seen, last_seen, last_trace_id, last_version, fingerprint FROM issues WHERE project_id = $1 AND id = $2) i").bind(pa.project.id).bind(id).fetch_optional(&st.pg).await?;
        let Some((issue,)) = row else { return Err(ApiError::NotFound("issue")); };
        subject = format!("issue: {}", issue.get("title").and_then(|x| x.as_str()).unwrap_or(""));
        if let Some(t) = issue.get("exception_type").and_then(|x| x.as_str()).filter(|x| !x.is_empty()) { filters.push(galileo_query::Filter::new("exception_type", galileo_query::FilterOp::Eq, t)); }
        if let Some(r) = issue.get("route").and_then(|x| x.as_str()).filter(|x| !x.is_empty()) { filters.push(galileo_query::Filter::new("http_route", galileo_query::FilterOp::Eq, r)); }
        if let Some(tid) = issue.get("last_trace_id").and_then(|x| x.as_str()).and_then(galileo_core::TraceId::from_hex) {
            if let Ok(spans) = st.storage.fetch_trace(ProjectId(pa.project.id), tid).await { if !spans.is_empty() { evidence.insert("sample_trace".into(), trace_summary(&galileo_query::trace::assemble(tid, spans))); } }
        }
        evidence.insert("issue".into(), issue);
    } else if let Some(id) = b.trigger_id {
        let row: Option<(Value,)> = sqlx::query_as("SELECT row_to_json(t) FROM (SELECT * FROM triggers WHERE project_id = $1 AND id = $2) t").bind(pa.project.id).bind(id).fetch_optional(&st.pg).await?;
        let Some((trig,)) = row else { return Err(ApiError::NotFound("trigger")); };
        subject = format!("trigger: {}", trig.get("name").and_then(|x| x.as_str()).unwrap_or(""));
        if let Some(q) = trig.get("query").cloned() { if let Ok(qq) = galileo_query::Query::from_json(q) { filters = qq.filters.clone(); evidence.insert("trigger_query".into(), json!(galileo_query::text::stringify(&qq))); } }
        evidence.insert("trigger".into(), trig);
    } else {
        return Err(ApiError::BadRequest("issue_id or trigger_id required".into()));
    }
    // occurrences over time + by route/tenant/user + comparison with the previous window
    let mut q = galileo_query::Query { dataset: galileo_query::Dataset::Spans, time_range: TimeRange::Relative { last_seconds: b.last_seconds }, calculations: vec![galileo_query::Calculation { op: galileo_query::CalcOp::Count, field: None }], filters: filters.clone(), breakdowns: vec!["http_route".into(), "tenant_id".into()], orders: vec![], limit: Some(10), compare_to: Some("previous".into()), ..Default::default() };
    if filters.is_empty() { q.filters.push(galileo_query::Filter::new("status_code", galileo_query::FilterOp::Eq, "error")); }
    if let Ok(r) = galileo_query::run::run(st.storage.as_ref(), ProjectId(pa.project.id), &q).await {
        evidence.insert("occurrences_by_route_tenant".into(), json!(r.groups.iter().map(|g| json!({ "key": g.key, "count": g.totals.first().copied().flatten(), "previous": g.compare_totals.as_ref().and_then(|c| c.first().copied().flatten()) })).collect::<Vec<_>>()));
    }
    // BubbleUp: what distinguishes affected spans from the rest of the service
    let base = galileo_query::Query { dataset: galileo_query::Dataset::Spans, time_range: TimeRange::Relative { last_seconds: b.last_seconds }, calculations: vec![], filters: vec![], breakdowns: vec![], orders: vec![], limit: Some(20), ..Default::default() };
    let sel = if q.filters.is_empty() { vec![galileo_query::Filter::new("status_code", galileo_query::FilterOp::Eq, "error")] } else { q.filters.clone() };
    if let Ok(bu) = galileo_query::bubbleup::run(st.storage.as_ref(), ProjectId(pa.project.id), &galileo_query::bubbleup::BubbleUpRequest { query: base, selection: sel, max_keys: 8, max_values: 3 }).await {
        evidence.insert("bubbleup".into(), scrub(serde_json::to_value(&bu).unwrap_or(Value::Null)));
    }
    if let Ok(Json(n)) = query::nplusone(State(st.clone()), pa.clone(), QueryParams(query::RangeParams { last_seconds: b.last_seconds })).await { evidence.insert("nplusone".into(), scrub(n)); }
    let deploys: Vec<(Value,)> = sqlx::query_as("SELECT row_to_json(d) FROM (SELECT version, service, at, note FROM deploys WHERE project_id = $1 AND at > now() - make_interval(secs => $2) ORDER BY at DESC LIMIT 10) d").bind(pa.project.id).bind(b.last_seconds as f64).fetch_all(&st.pg).await.unwrap_or_default();
    evidence.insert("deploys".into(), json!(deploys.into_iter().map(|d| d.0).collect::<Vec<_>>()));

    let prompt = format!("You are an SRE writing a root-cause note for a {subject} in project {}. Use ONLY the evidence JSON below. Do not think step by step and do not list the evidence or the task: write the final text immediately. Output ONLY the final markdown, starting directly with the heading **What is happening**. Sections: **What is happening** (numbers, routes, tenants, users affected, trend vs previous window), **Most likely cause** (name the function/table/statement/deploy when the evidence shows it; say what is uncertain), **What to do next** (2–4 concrete steps, e.g. which query to run or which function to fix). Under 300 words. No preamble.\n\nEVIDENCE:\n{}", pa.project.name, cap(Value::Object(evidence.clone()).to_string(), 14_000));
    let r = st.gateway.chat_recorded(rec_project, &s.route_alias, "assistant.investigate", &[("user".into(), prompt)], 1600, 0.1, Some(pa.user.email.clone())).await.map_err(|e| ApiError::BadRequest(format!("assistant model call failed: {e}")))?;
    let md = strip_preamble(r.text.trim(), "**What is happening**");
    if let Some(id) = b.issue_id {
        let stamp = format!("\n\n---\n_Investigation by Ask Galileo · {} · {}_\n\n{}", Utc::now().format("%Y-%m-%d %H:%M UTC"), r.model, md);
        sqlx::query("UPDATE issues SET notes = notes || $2, updated_at = now() WHERE id = $1").bind(id).bind(&stamp).execute(&st.pg).await?;
        let _ = galileo_alerts::issues::add_event(&st.pg, id, "note", "investigation written by Ask Galileo", Some(pa.user.id)).await;
    }
    Ok(Json(json!({ "markdown": md, "subject": subject, "evidence_keys": evidence.keys().cloned().collect::<Vec<_>>(), "recording": { "project_id": rec_project, "span_id": r.span_id, "trace_id": r.trace_id, "model": r.model }, "usage": { "input_tokens": r.input_tokens, "output_tokens": r.output_tokens, "cost_usd": r.cost_usd }, "link": format!("/p/{pid}/issues") })))
}

#[derive(Deserialize)]
pub struct ExplainBody { pub trace_id: String }

pub async fn explain_trace(State(st): State<AppState>, pa: ProjectAccess, Path(_pid): Path<Uuid>, Json(b): Json<ExplainBody>) -> ApiResult<Json<Value>> {
    let s = settings_or_err(&st, &pa).await?;
    let tid = galileo_core::TraceId::from_hex(b.trace_id.trim()).ok_or_else(|| ApiError::BadRequest("invalid trace id".into()))?;
    let spans = st.storage.fetch_trace(ProjectId(pa.project.id), tid).await?;
    if spans.is_empty() { return Err(ApiError::NotFound("trace")); }
    let summary = trace_summary(&galileo_query::trace::assemble(tid, spans));
    let prompt = format!("Explain this distributed trace to a developer. Do not think step by step and do not list the inputs or the task: write the final text immediately. Output ONLY the final markdown, starting directly with the heading **Where the time went**. Sections: **Where the time went** (slowest spans, functions, tables, with ms), **What repeated** (N+1: statement × count, which function), **What failed**, **One improvement** (concrete code-level fix). Under 200 words. Use only the JSON.\n\nTRACE:\n{}", cap(summary.to_string(), 12_000));
    let r = st.gateway.chat_recorded(s.project_id.unwrap(), &s.route_alias, "assistant.explain", &[("user".into(), prompt)], 1800, 0.1, Some(pa.user.email.clone())).await.map_err(|e| ApiError::BadRequest(format!("assistant model call failed: {e}")))?;
    Ok(Json(json!({ "markdown": strip_preamble(r.text.trim(), "**Where the time went**"), "recording": { "project_id": s.project_id, "span_id": r.span_id, "trace_id": r.trace_id, "model": r.model }, "usage": { "input_tokens": r.input_tokens, "output_tokens": r.output_tokens, "cost_usd": r.cost_usd } })))
}

/// Some models think out loud before the requested markdown; keep only from the first expected
/// heading onwards when it is present.
pub fn strip_preamble(text: &str, first_heading: &str) -> String {
    // the final answer is the LAST place the heading starts a line (notes may quote it earlier)
    let mut best: Option<usize> = None;
    let mut from = 0;
    while let Some(i) = text[from..].find(first_heading) {
        let at = from + i;
        let line_start = at == 0 || text[..at].ends_with('\n') || text[..at].ends_with("\n\n");
        if line_start { best = Some(at); }
        from = at + first_heading.len();
    }
    match best { Some(i) if i > 0 => text[i..].to_string(), _ => text.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_fenced_and_loose_json() {
        assert_eq!(parse_action("```json\n{\"action\":\"answer\",\"markdown\":\"hi\"}\n```").unwrap()["action"], "answer");
        assert_eq!(parse_action("Sure! {\"action\":\"run_query\",\"text\":\"spans | count()\"} done").unwrap()["text"], "spans | count()");
        assert!(parse_action("no json here").is_none());
        assert!(parse_action("{\"foo\": 1}").is_none());
    }
    #[test]
    fn recovers_answer_with_raw_newlines() {
        let v = parse_action("{\"action\":\"answer\",\"markdown\":\"line one\nline two with \\\"quotes\\\"\"}").unwrap();
        assert_eq!(v["action"], "answer");
        assert_eq!(v["markdown"], "line one\nline two with \"quotes\"");
    }
    #[test]
    fn strips_reasoning_preamble() {
        assert_eq!(strip_preamble("Input: json… thinking\n\n**Where the time went**\nx", "**Where the time went**"), "**Where the time went**\nx");
        assert_eq!(strip_preamble("notes: start with **Where the time went**.\n  * more notes\n\n**Where the time went**\nreal", "**Where the time went**"), "**Where the time went**\nreal");
        assert_eq!(strip_preamble("**Where the time went**\nx", "**Where the time went**"), "**Where the time went**\nx");
        assert_eq!(strip_preamble("no heading", "**Where the time went**"), "no heading");
    }
    #[test]
    fn cap_truncates() {
        let s = cap("a".repeat(50), 10);
        assert!(s.starts_with("aaaaaaaaaa") && s.contains("truncated"));
        assert_eq!(cap("short".into(), 10), "short");
    }
    #[test]
    fn scrub_removes_sensitive_keys() {
        let v = scrub(json!({ "rows": [{ "gen_ai.prompt": "secret", "name": "x" }], "gen_ai.completion": "y" }));
        assert!(v["rows"][0].get("gen_ai.prompt").is_none());
        assert!(v.get("gen_ai.completion").is_none());
        assert_eq!(v["rows"][0]["name"], "x");
    }
}
