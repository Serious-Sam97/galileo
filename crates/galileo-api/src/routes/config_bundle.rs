//! Config as code: export a project's configuration as a bundle (YAML/JSON) and import it back
//! idempotently (matched by name/alias), with a dry-run diff.

use axum::extract::{Query as QueryParams, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const REDACTED: &str = "<redacted>";

async fn rows(pg: &sqlx::PgPool, sql: &str, project: Uuid) -> Vec<Value> {
    let r: Vec<(Value,)> = sqlx::query_as(sql).bind(project).fetch_all(pg).await.unwrap_or_default();
    r.into_iter().map(|x| x.0).collect()
}

fn strip(v: &mut Value, keys: &[&str]) {
    if let Some(o) = v.as_object_mut() { for k in keys { o.remove(*k); } }
}

pub async fn build_bundle(st: &AppState, project: Uuid) -> Value {
    let pg = &st.pg;
    let mut triggers = rows(pg, "SELECT row_to_json(t) FROM (SELECT name, description, query, op, threshold, frequency_secs, window_secs, enabled, recipients, warn_threshold, for_secs, per_group, mode, baseline_factor, baseline_min_delta, sensitivity, min_value, composite FROM triggers WHERE project_id = $1 ORDER BY name) t", project).await;
    for t in &mut triggers { if let Some(r) = t.get_mut("recipients") { *r = json!(r.as_array().map(|a| a.iter().filter(|x| x.get("type").and_then(|t| t.as_str()) != Some("channel") && x.get("type").and_then(|t| t.as_str()) != Some("oncall")).cloned().collect::<Vec<_>>()).unwrap_or_default()); } }
    let slos = rows(pg, "SELECT row_to_json(s) FROM (SELECT name, description, dataset, total_filters, good_filters, target, window_days, recipients FROM slos WHERE project_id = $1 ORDER BY name) s", project).await;
    let boards = rows(pg, "SELECT row_to_json(b) FROM (SELECT name, description, panels, variables, time_range, compare, template FROM boards WHERE project_id = $1 ORDER BY name) b", project).await;
    let mut providers = rows(pg, "SELECT row_to_json(p) FROM (SELECT name, kind, base_url, headers FROM gateway_providers WHERE project_id = $1 ORDER BY name) p", project).await;
    for p in &mut providers { if let Some(o) = p.as_object_mut() { o.insert("api_key".into(), json!(REDACTED)); } }
    let routes = rows(pg, "SELECT row_to_json(r) FROM (SELECT r.alias, r.description, r.enabled, r.budget, r.rate_limit, (SELECT json_agg(json_build_object('provider', p.name, 'model', t->>'model', 'price_input', t->'price_input', 'price_output', t->'price_output')) FROM jsonb_array_elements(r.targets) t JOIN gateway_providers p ON p.id::text = t->>'provider_id') AS targets FROM gateway_routes r WHERE r.project_id = $1 ORDER BY r.alias) r", project).await;
    let prompts = rows(pg, "SELECT row_to_json(p) FROM (SELECT pr.name, pr.description, pr.ci, pr.promoted_version, (SELECT json_agg(json_build_object('version', v.version, 'content', v.content, 'note', v.note) ORDER BY v.version) FROM prompt_versions v WHERE v.prompt_id = pr.id) AS versions FROM prompts pr WHERE pr.project_id = $1 ORDER BY pr.name) p", project).await;
    let redaction = rows(pg, "SELECT row_to_json(r) FROM (SELECT rule, description FROM redaction_rules WHERE project_id = $1 ORDER BY created_at) r", project).await;
    let pipeline = rows(pg, "SELECT pipeline FROM log_pipelines WHERE project_id = $1", project).await.into_iter().next().unwrap_or(json!({ "enabled": true, "processors": [] }));
    let log_metrics = rows(pg, "SELECT rule FROM log_metrics WHERE project_id = $1 ORDER BY name", project).await;
    let mut channels = rows(pg, "SELECT row_to_json(c) FROM (SELECT name, kind, config, enabled FROM notification_channels WHERE project_id = $1 ORDER BY name) c", project).await;
    for c in &mut channels { if let Some(cfg) = c.get_mut("config").and_then(|x| x.as_object_mut()) { for k in ["url", "bot_token"] { if cfg.contains_key(k) { cfg.insert(k.into(), json!(REDACTED)); } } } }
    let settings = rows(pg, "SELECT row_to_json(s) FROM (SELECT retention_spans_days, retention_logs_days, retention_metrics_days, sampling, quotas, issue_recipients FROM project_settings WHERE project_id = $1) s", project).await.into_iter().next().unwrap_or(json!({}));
    json!({ "galileo": { "bundle": 1 }, "triggers": triggers, "slos": slos, "boards": boards, "gateway": { "providers": providers, "routes": routes, "prompts": prompts }, "redaction_rules": redaction, "log_pipeline": pipeline, "log_metrics": log_metrics, "channels": channels, "settings": settings })
}

#[derive(Deserialize)]
pub struct ExportParams { #[serde(default = "d_fmt")] pub format: String }
fn d_fmt() -> String { "yaml".into() }

pub async fn export(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<ExportParams>) -> ApiResult<axum::response::Response> {
    let bundle = build_bundle(&st, pa.project.id).await;
    if p.format == "json" { return Ok(Json(bundle).into_response()); }
    let y = serde_yaml::to_string(&bundle).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "application/yaml"), (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"galileo.yaml\"")], y).into_response())
}

#[derive(Deserialize)]
pub struct ImportParams { #[serde(default)] pub dry_run: bool }

fn names(list: &[Value], key: &str) -> Vec<String> { list.iter().filter_map(|x| x.get(key).and_then(|n| n.as_str()).map(str::to_owned)).collect() }

/// Canonical form for comparison: integral floats become ints, nulls are dropped, ids ignored.
fn canon(v: &Value) -> Value {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 1e15).map(|f| json!(f as i64)).unwrap_or_else(|| v.clone()),
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        Value::Object(o) => Value::Object(o.iter().filter(|(k, x)| !x.is_null() && k.as_str() != "id").map(|(k, x)| (k.clone(), canon(x))).collect()),
        _ => v.clone(),
    }
}

fn diff_section(name: &str, key: &str, current: &[Value], incoming: &[Value], out: &mut Vec<Value>) {
    let cur: Map<String, Value> = current.iter().filter_map(|x| x.get(key).and_then(|n| n.as_str()).map(|n| (n.to_string(), x.clone()))).collect();
    for inc in incoming {
        let Some(n) = inc.get(key).and_then(|n| n.as_str()) else { continue };
        match cur.get(n) {
            None => out.push(json!({ "section": name, "name": n, "action": "create" })),
            Some(c) => { let mut a = c.clone(); let mut b = inc.clone(); for k in ["api_key", "config"] { strip(&mut a, &[k]); strip(&mut b, &[k]); } if canon(&a) != canon(&b) { out.push(json!({ "section": name, "name": n, "action": "update" })); } }
        }
    }
    let _ = names(incoming, key);
}

/// Apply a bundle. Matched by name/alias; secrets marked `<redacted>` keep their current value.
pub async fn import(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<ImportParams>, body: String) -> ApiResult<Json<Value>> {
    pa.require_write()?;
    let bundle: Value = serde_json::from_str(&body).or_else(|_| serde_yaml::from_str::<Value>(&body)).map_err(|e| ApiError::BadRequest(format!("bundle must be YAML or JSON: {e}")))?;
    let current = build_bundle(&st, pa.project.id).await;
    let arr = |v: &Value, path: &[&str]| -> Vec<Value> { let mut x = v; for k in path { x = x.get(*k).unwrap_or(&Value::Null); } x.as_array().cloned().unwrap_or_default() };
    let mut diff = vec![];
    diff_section("triggers", "name", &arr(&current, &["triggers"]), &arr(&bundle, &["triggers"]), &mut diff);
    diff_section("slos", "name", &arr(&current, &["slos"]), &arr(&bundle, &["slos"]), &mut diff);
    diff_section("boards", "name", &arr(&current, &["boards"]), &arr(&bundle, &["boards"]), &mut diff);
    diff_section("providers", "name", &arr(&current, &["gateway", "providers"]), &arr(&bundle, &["gateway", "providers"]), &mut diff);
    diff_section("routes", "alias", &arr(&current, &["gateway", "routes"]), &arr(&bundle, &["gateway", "routes"]), &mut diff);
    diff_section("prompts", "name", &arr(&current, &["gateway", "prompts"]), &arr(&bundle, &["gateway", "prompts"]), &mut diff);
    diff_section("channels", "name", &arr(&current, &["channels"]), &arr(&bundle, &["channels"]), &mut diff);
    diff_section("log_metrics", "name", &arr(&current, &["log_metrics"]), &arr(&bundle, &["log_metrics"]), &mut diff);
    if bundle.get("log_pipeline").map(|x| canon(x) != canon(current.get("log_pipeline").unwrap_or(&Value::Null))).unwrap_or(false) { diff.push(json!({ "section": "log_pipeline", "name": "pipeline", "action": "update" })); }
    if bundle.get("settings").map(|x| canon(x) != canon(current.get("settings").unwrap_or(&Value::Null))).unwrap_or(false) { diff.push(json!({ "section": "settings", "name": "settings", "action": "update" })); }
    if p.dry_run { return Ok(Json(json!({ "dry_run": true, "changes": diff }))); }

    let pg = &st.pg; let pid = pa.project.id;
    // providers first (routes reference them by name)
    for pr in arr(&bundle, &["gateway", "providers"]) {
        let (Some(name), Some(kind)) = (pr.get("name").and_then(|x| x.as_str()), pr.get("kind").and_then(|x| x.as_str())) else { continue };
        let key = pr.get("api_key").and_then(|x| x.as_str()).filter(|k| *k != REDACTED);
        let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM gateway_providers WHERE project_id = $1 AND name = $2").bind(pid).bind(name).fetch_optional(pg).await?;
        match existing {
            Some((id,)) => { sqlx::query("UPDATE gateway_providers SET kind = $3, base_url = $4, headers = $5 WHERE id = $1 AND project_id = $2").bind(id).bind(pid).bind(kind).bind(pr.get("base_url").and_then(|x| x.as_str()).unwrap_or("")).bind(pr.get("headers").cloned().unwrap_or(json!({}))).execute(pg).await?;
                if let Some(k) = key { sqlx::query("UPDATE gateway_providers SET api_key_enc = $3 WHERE id = $1 AND project_id = $2").bind(id).bind(pid).bind(galileo_gateway::crypto::encrypt(&st.secret, k)).execute(pg).await?; } }
            None => { sqlx::query("INSERT INTO gateway_providers (id, project_id, name, kind, base_url, api_key_enc, headers) VALUES ($1, $2, $3, $4, $5, $6, $7)").bind(Uuid::now_v7()).bind(pid).bind(name).bind(kind).bind(pr.get("base_url").and_then(|x| x.as_str()).unwrap_or("")).bind(key.map(|k| galileo_gateway::crypto::encrypt(&st.secret, k))).bind(pr.get("headers").cloned().unwrap_or(json!({}))).execute(pg).await?; }
        }
    }
    let prov: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, name FROM gateway_providers WHERE project_id = $1").bind(pid).fetch_all(pg).await?;
    for r in arr(&bundle, &["gateway", "routes"]) {
        let Some(alias) = r.get("alias").and_then(|x| x.as_str()) else { continue };
        let targets: Vec<Value> = r.get("targets").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|t| { let pname = t.get("provider").and_then(|x| x.as_str())?; let pid_ = prov.iter().find(|p| p.1 == pname)?.0; Some(json!({ "provider_id": pid_, "model": t.get("model").and_then(|x| x.as_str()).unwrap_or(""), "price_input": t.get("price_input"), "price_output": t.get("price_output") })) }).collect()).unwrap_or_default();
        sqlx::query("INSERT INTO gateway_routes (id, project_id, alias, description, targets, budget, rate_limit, enabled) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (project_id, alias) DO UPDATE SET description = $4, targets = $5, budget = $6, rate_limit = $7, enabled = $8, updated_at = now()")
            .bind(Uuid::now_v7()).bind(pid).bind(alias).bind(r.get("description").and_then(|x| x.as_str()).unwrap_or("")).bind(json!(targets)).bind(r.get("budget").cloned().unwrap_or(json!({}))).bind(r.get("rate_limit").cloned().unwrap_or(json!({}))).bind(r.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true)).execute(pg).await?;
    }
    for t in arr(&bundle, &["triggers"]) {
        let Some(name) = t.get("name").and_then(|x| x.as_str()) else { continue };
        let g = |k: &str| t.get(k).cloned().unwrap_or(Value::Null);
        let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM triggers WHERE project_id = $1 AND name = $2 ORDER BY created_at LIMIT 1").bind(pid).bind(name).fetch_optional(pg).await?;
        let id = match existing { Some((id,)) => id, None => { let id = Uuid::now_v7(); sqlx::query("INSERT INTO triggers (id, project_id, name, query, op, threshold) VALUES ($1, $2, $3, $4, $5, $6)").bind(id).bind(pid).bind(name).bind(g("query")).bind(g("op").as_str().unwrap_or(">")).bind(g("threshold").as_f64().unwrap_or(0.0)).execute(pg).await?; id } };
        sqlx::query("UPDATE triggers SET description = $3, query = $4, op = $5, threshold = $6, frequency_secs = $7, window_secs = $8, enabled = $9, recipients = $10, warn_threshold = $11, for_secs = $12, per_group = $13, mode = $14, baseline_factor = $15, baseline_min_delta = $16, sensitivity = $17, min_value = $18, composite = $19, updated_at = now() WHERE id = $1 AND project_id = $2")
            .bind(id).bind(pid).bind(g("description").as_str().unwrap_or("")).bind(g("query")).bind(g("op").as_str().unwrap_or(">")).bind(g("threshold").as_f64().unwrap_or(0.0)).bind(g("frequency_secs").as_i64().unwrap_or(60) as i32).bind(g("window_secs").as_i64().unwrap_or(300) as i32).bind(g("enabled").as_bool().unwrap_or(true)).bind(if g("recipients").is_null() { json!([]) } else { g("recipients") }).bind(g("warn_threshold").as_f64()).bind(g("for_secs").as_i64().unwrap_or(0) as i32).bind(g("per_group").as_bool().unwrap_or(false)).bind(g("mode").as_str().unwrap_or("threshold")).bind(g("baseline_factor").as_f64().unwrap_or(2.0)).bind(g("baseline_min_delta").as_f64().unwrap_or(0.0)).bind(g("sensitivity").as_i64().unwrap_or(3) as i32).bind(g("min_value").as_f64().unwrap_or(0.0)).bind(if g("composite").is_null() { None } else { Some(g("composite")) }).execute(pg).await?;
    }
    for b in arr(&bundle, &["boards"]) {
        let Some(name) = b.get("name").and_then(|x| x.as_str()) else { continue };
        let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM boards WHERE project_id = $1 AND name = $2").bind(pid).bind(name).fetch_optional(pg).await?;
        let (desc, panels, vars, tr, comp) = (b.get("description").and_then(|x| x.as_str()).unwrap_or(""), b.get("panels").cloned().unwrap_or(json!([])), b.get("variables").cloned().unwrap_or(json!([])), b.get("time_range").cloned(), b.get("compare").and_then(|x| x.as_bool()).unwrap_or(false));
        match existing {
            Some((id,)) => { sqlx::query("UPDATE boards SET description = $3, panels = $4, variables = $5, time_range = $6, compare = $7, updated_at = now() WHERE id = $1 AND project_id = $2").bind(id).bind(pid).bind(desc).bind(&panels).bind(&vars).bind(&tr).bind(comp).execute(pg).await?; }
            None => { sqlx::query("INSERT INTO boards (id, project_id, name, description, panels, variables, time_range, compare) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)").bind(Uuid::now_v7()).bind(pid).bind(name).bind(desc).bind(&panels).bind(&vars).bind(&tr).bind(comp).execute(pg).await?; }
        }
    }
    for pr in arr(&bundle, &["gateway", "prompts"]) {
        let Some(name) = pr.get("name").and_then(|x| x.as_str()) else { continue };
        let id: Uuid = match sqlx::query_as::<_, (Uuid,)>("SELECT id FROM prompts WHERE project_id = $1 AND name = $2").bind(pid).bind(name).fetch_optional(pg).await? { Some((id,)) => id, None => { let id = Uuid::now_v7(); sqlx::query("INSERT INTO prompts (id, project_id, name, description) VALUES ($1, $2, $3, $4)").bind(id).bind(pid).bind(name).bind(pr.get("description").and_then(|x| x.as_str()).unwrap_or("")).execute(pg).await?; id } };
        if let Some(ci) = pr.get("ci") { sqlx::query("UPDATE prompts SET ci = $2, promoted_version = $3 WHERE id = $1").bind(id).bind(ci).bind(pr.get("promoted_version").and_then(|x| x.as_i64()).map(|v| v as i32)).execute(pg).await?; }
        for v in pr.get("versions").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
            let Some(ver) = v.get("version").and_then(|x| x.as_i64()) else { continue };
            let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM prompt_versions WHERE prompt_id = $1 AND version = $2").bind(id).bind(ver as i32).fetch_optional(pg).await?;
            match existing { Some((vid,)) => { sqlx::query("UPDATE prompt_versions SET content = $2, note = $3 WHERE id = $1").bind(vid).bind(v.get("content").cloned().unwrap_or(json!({}))).bind(v.get("note").and_then(|x| x.as_str()).unwrap_or("")).execute(pg).await?; } None => { sqlx::query("INSERT INTO prompt_versions (id, prompt_id, version, content, note) VALUES ($1, $2, $3, $4, $5)").bind(Uuid::now_v7()).bind(id).bind(ver as i32).bind(v.get("content").cloned().unwrap_or(json!({}))).bind(v.get("note").and_then(|x| x.as_str()).unwrap_or("")).execute(pg).await?; } }
        }
    }
    if let Some(pl) = bundle.get("log_pipeline") { sqlx::query("INSERT INTO log_pipelines (project_id, pipeline) VALUES ($1, $2) ON CONFLICT (project_id) DO UPDATE SET pipeline = $2, updated_at = now()").bind(pid).bind(pl).execute(pg).await?; }
    for m in arr(&bundle, &["log_metrics"]) { if let Some(name) = m.get("name").and_then(|x| x.as_str()) {
        let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM log_metrics WHERE project_id = $1 AND name = $2").bind(pid).bind(name).fetch_optional(pg).await?;
        match existing { Some((id,)) => { sqlx::query("UPDATE log_metrics SET rule = $2 WHERE id = $1").bind(id).bind(&m).execute(pg).await?; } None => { sqlx::query("INSERT INTO log_metrics (id, project_id, name, rule) VALUES ($1, $2, $3, $4)").bind(Uuid::now_v7()).bind(pid).bind(name).bind(&m).execute(pg).await?; } }
    } }
    for c in arr(&bundle, &["channels"]) {
        let (Some(name), Some(kind)) = (c.get("name").and_then(|x| x.as_str()), c.get("kind").and_then(|x| x.as_str())) else { continue };
        let mut cfg = c.get("config").cloned().unwrap_or(json!({}));
        let existing: Option<(Uuid, Value)> = sqlx::query_as("SELECT id, config FROM notification_channels WHERE project_id = $1 AND name = $2").bind(pid).bind(name).fetch_optional(pg).await?;
        if let (Some((_, old)), Some(o)) = (&existing, cfg.as_object_mut()) { for k in ["url", "bot_token"] { if o.get(k).and_then(|x| x.as_str()) == Some(REDACTED) { if let Some(v) = old.get(k) { o.insert(k.into(), v.clone()); } } } }
        match existing {
            Some((id, _)) => { sqlx::query("UPDATE notification_channels SET kind = $3, config = $4, enabled = $5 WHERE id = $1 AND project_id = $2").bind(id).bind(pid).bind(kind).bind(&cfg).bind(c.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true)).execute(pg).await?; }
            None => { sqlx::query("INSERT INTO notification_channels (id, project_id, name, kind, config, enabled) VALUES ($1, $2, $3, $4, $5, $6)").bind(Uuid::now_v7()).bind(pid).bind(name).bind(kind).bind(&cfg).bind(c.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true)).execute(pg).await?; }
        }
    }
    for r in arr(&bundle, &["redaction_rules"]) { if let Some(rule) = r.get("rule") { let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM redaction_rules WHERE project_id = $1 AND rule = $2").bind(pid).bind(rule).fetch_optional(pg).await?; if exists.is_none() { sqlx::query("INSERT INTO redaction_rules (id, project_id, rule, description) VALUES ($1, $2, $3, $4)").bind(Uuid::now_v7()).bind(pid).bind(rule).bind(r.get("description").and_then(|x| x.as_str()).unwrap_or("")).execute(pg).await?; } } }
    if let Some(s) = bundle.get("settings").and_then(|x| x.as_object()) {
        sqlx::query("INSERT INTO project_settings (project_id) VALUES ($1) ON CONFLICT (project_id) DO NOTHING").bind(pid).execute(pg).await?;
        if let Some(q) = s.get("quotas") { sqlx::query("UPDATE project_settings SET quotas = $2 WHERE project_id = $1").bind(pid).bind(q).execute(pg).await?; }
        if let Some(q) = s.get("sampling") { sqlx::query("UPDATE project_settings SET sampling = $2 WHERE project_id = $1").bind(pid).bind(q).execute(pg).await?; }
        if let Some(q) = s.get("issue_recipients") { sqlx::query("UPDATE project_settings SET issue_recipients = $2 WHERE project_id = $1").bind(pid).bind(q).execute(pg).await?; }
        for (col, key) in [("retention_spans_days", "retention_spans_days"), ("retention_logs_days", "retention_logs_days"), ("retention_metrics_days", "retention_metrics_days")] { if let Some(v) = s.get(key) { sqlx::query(&format!("UPDATE project_settings SET {col} = $2 WHERE project_id = $1")).bind(pid).bind(v.as_i64().map(|x| x as i32)).execute(pg).await?; } }
    }
    st.resolver.invalidate_all();
    audit::project(pg, &pa, "config.import", "project", pid, json!({ "changes": diff.len() })).await;
    Ok(Json(json!({ "applied": true, "changes": diff })))
}
