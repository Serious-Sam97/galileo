use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::ProjectAccess;
use crate::db::boards;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct BoardPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub board_id: Uuid,
}

#[derive(Deserialize)]
pub struct Body {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "empty_panels")]
    pub panels: serde_json::Value,
}
fn empty_panels() -> serde_json::Value {
    json!([])
}

pub async fn list(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "boards": boards::list(&st.pg, pa.project.id).await? })))
}

pub async fn get(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<BoardPath>) -> ApiResult<Json<serde_json::Value>> {
    let b = boards::get(&st.pg, pa.project.id, p.board_id).await?.ok_or(ApiError::NotFound("board"))?;
    Ok(Json(json!({ "board": b })))
}

pub async fn create(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<Body>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if !b.panels.is_array() {
        return Err(ApiError::BadRequest("panels must be an array".into()));
    }
    let board = boards::create(&st.pg, pa.project.id, b.name.trim(), &b.description, b.panels).await?;
    Ok(Json(json!({ "board": board })))
}

pub async fn update(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<BoardPath>, Json(b): Json<Body>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if !b.panels.is_array() {
        return Err(ApiError::BadRequest("panels must be an array".into()));
    }
    let board = boards::update(&st.pg, pa.project.id, p.board_id, b.name.trim(), &b.description, b.panels)
        .await?
        .ok_or(ApiError::NotFound("board"))?;
    Ok(Json(json!({ "board": board })))
}

pub async fn delete(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<BoardPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if !boards::delete(&st.pg, pa.project.id, p.board_id).await? {
        return Err(ApiError::NotFound("board"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- boards v2: settings, variables, templates

#[derive(Deserialize)]
pub struct SettingsBody { #[serde(default)] pub variables: Option<serde_json::Value>, #[serde(default)] pub time_range: Option<serde_json::Value>, #[serde(default)] pub compare: Option<bool> }

/// Board-level settings: variables (`[{name, field, default, label}]`), time range, compare.
pub async fn patch_settings(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<BoardPath>, Json(b): Json<SettingsBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if let Some(v) = &b.variables { if !v.is_array() { return Err(ApiError::BadRequest("variables must be an array".into())); } }
    let n = sqlx::query("UPDATE boards SET variables = coalesce($3, variables), time_range = CASE WHEN $4::jsonb IS NULL THEN time_range ELSE nullif($4::jsonb, 'null'::jsonb) END, compare = coalesce($5, compare), updated_at = now() WHERE id = $1 AND project_id = $2")
        .bind(p.board_id).bind(pa.project.id).bind(&b.variables).bind(&b.time_range).bind(b.compare).execute(&st.pg).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound("board")); }
    let board = boards::get(&st.pg, pa.project.id, p.board_id).await?.ok_or(ApiError::NotFound("board"))?;
    Ok(Json(json!({ "board": board })))
}

#[derive(Deserialize)]
pub struct VarPath { #[allow(dead_code)] pub project_id: Uuid, pub board_id: Uuid, pub name: String }
#[derive(Deserialize)]
pub struct VarParams { #[serde(default = "d_last")] pub last_seconds: i64, #[serde(default)] pub q: Option<String> }
fn d_last() -> i64 { 86_400 }

/// Values for a board variable (distinct field values in the window), for the header pickers.
pub async fn variable_values(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<VarPath>, axum::extract::Query(vp): axum::extract::Query<VarParams>) -> ApiResult<Json<serde_json::Value>> {
    let row: Option<(serde_json::Value,)> = sqlx::query_as("SELECT variables FROM boards WHERE id = $1 AND project_id = $2").bind(p.board_id).bind(pa.project.id).fetch_optional(&st.pg).await?;
    let Some((vars,)) = row else { return Err(ApiError::NotFound("board")); };
    let var = vars.as_array().and_then(|a| a.iter().find(|v| v.get("name").and_then(|n| n.as_str()) == Some(p.name.as_str()))).cloned().ok_or(ApiError::NotFound("variable"))?;
    let field = var.get("field").and_then(|f| f.as_str()).unwrap_or("").to_string();
    let dataset = var.get("dataset").and_then(|d| d.as_str()).and_then(|d| serde_json::from_value::<galileo_query::Dataset>(json!(d)).ok()).unwrap_or_default();
    let r = crate::routes::query::values(State(st.clone()), pa.clone(), axum::extract::Query(crate::routes::query::ValuesParams { dataset, field, q: vp.q, last_seconds: vp.last_seconds })).await?;
    Ok(r)
}

#[derive(Deserialize)]
pub struct TemplateBody { pub kind: String, #[serde(default)] pub name: String, #[serde(default)] pub service: Option<String> }

fn q(dataset: &str, calcs: serde_json::Value, filters: serde_json::Value, breakdowns: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({ "dataset": dataset, "time_range": { "last_seconds": 3600 }, "calculations": calcs, "filters": filters, "breakdowns": breakdowns, "orders": [], "limit": 10 });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) { for (k, val) in e { o.insert(k.clone(), val.clone()); } }
    v
}
#[allow(clippy::too_many_arguments)]
fn panel(id: &str, title: &str, viz: &str, query: serde_json::Value, x: u32, y: u32, w: u32, h: u32) -> serde_json::Value {
    json!({ "id": id, "title": title, "viz": viz, "query": query, "x": x, "y": y, "w": w, "h": h })
}

/// One-click boards: RED per service, LLM cost, browser vitals.
pub async fn create_template(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<TemplateBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let svc = b.service.clone().filter(|s| !s.is_empty());
    let svc_filter = |extra: Vec<serde_json::Value>| -> serde_json::Value { let mut f = vec![]; if let Some(s) = &svc { f.push(json!({ "field": "service_name", "op": "eq", "value": s })); } f.extend(extra); json!(f) };
    let (name, variables, panels): (String, serde_json::Value, Vec<serde_json::Value>) = match b.kind.as_str() {
        "red" => (
            if b.name.is_empty() { format!("RED — {}", svc.clone().unwrap_or_else(|| "all services".into())) } else { b.name.clone() },
            json!([{ "name": "service", "field": "service_name", "label": "Service", "default": svc.clone().unwrap_or_default() }, { "name": "route", "field": "http_route", "label": "Route", "default": "" }]),
            vec![
                panel("rate", "Requests / s", "line", q("spans", json!([{ "op": "RATE_PER_SEC" }]), svc_filter(vec![json!({ "field": "parent_span_id", "op": "eq", "value": "" })]), json!([]), json!({})), 0, 0, 4, 4),
                panel("errors", "Error rate", "line", q("spans", json!([{ "op": "AVG", "field": "is_error" }]), svc_filter(vec![json!({ "field": "parent_span_id", "op": "eq", "value": "" })]), json!([]), json!({})), 4, 0, 4, 4),
                panel("latency", "Latency p50 / p95 / p99", "line", q("spans", json!([{ "op": "P50", "field": "duration_ms" }, { "op": "P95", "field": "duration_ms" }, { "op": "P99", "field": "duration_ms" }]), svc_filter(vec![json!({ "field": "parent_span_id", "op": "eq", "value": "" })]), json!([]), json!({})), 8, 0, 4, 4),
                panel("routes", "Slowest routes (p95)", "table", q("spans", json!([{ "op": "COUNT" }, { "op": "P95", "field": "duration_ms" }, { "op": "AVG", "field": "is_error" }]), svc_filter(vec![json!({ "field": "parent_span_id", "op": "eq", "value": "" })]), json!(["http_route"]), json!({ "orders": [{ "field": "P95(duration_ms)", "direction": "desc" }] })), 0, 4, 6, 5),
                panel("db", "DB time by table", "line", q("spans", json!([{ "op": "SUM", "field": "duration_ms" }]), svc_filter(vec![json!({ "field": "db_system", "op": "exists" })]), json!(["db_table"]), json!({})), 6, 4, 6, 5),
                panel("issues", "Open issues", "issues", json!({}), 0, 9, 6, 4),
                panel("errors_by_type", "Errors by type", "table", q("spans", json!([{ "op": "COUNT" }]), svc_filter(vec![json!({ "field": "status_code", "op": "eq", "value": "error" })]), json!(["exception_type", "http_route"]), json!({})), 6, 9, 6, 4),
            ],
        ),
        "llm" => (
            if b.name.is_empty() { "LLM cost & quality".into() } else { b.name.clone() },
            json!([{ "name": "route", "field": "gen_ai.galileo.route", "label": "Route", "default": "" }, { "name": "model", "field": "gen_ai_model", "label": "Model", "default": "" }]),
            vec![
                panel("cost", "Cost per hour by model", "line", q("spans", json!([{ "op": "SUM", "field": "gen_ai.usage.cost_usd" }]), json!([{ "field": "gen_ai_system", "op": "exists" }]), json!(["gen_ai_model"]), json!({ "time_range": { "last_seconds": 86400 } })), 0, 0, 6, 4),
                panel("calls", "Calls & errors", "line", q("spans", json!([{ "op": "COUNT" }, { "op": "AVG", "field": "is_error" }]), json!([{ "field": "gen_ai_system", "op": "exists" }]), json!([]), json!({ "time_range": { "last_seconds": 86400 } })), 6, 0, 6, 4),
                panel("latency", "Latency p95 by route", "line", q("spans", json!([{ "op": "P95", "field": "duration_ms" }]), json!([{ "field": "gen_ai_system", "op": "exists" }]), json!(["gen_ai.galileo.route"]), json!({ "time_range": { "last_seconds": 86400 } })), 0, 4, 6, 4),
                panel("tokens", "Tokens by route", "table", q("spans", json!([{ "op": "SUM", "field": "gen_ai.usage.input_tokens" }, { "op": "SUM", "field": "gen_ai.usage.output_tokens" }, { "op": "SUM", "field": "gen_ai.usage.cost_usd" }]), json!([{ "field": "gen_ai_system", "op": "exists" }]), json!(["gen_ai.galileo.route", "gen_ai_model"]), json!({ "time_range": { "last_seconds": 86400 }, "derived": [{ "name": "usd_per_1k_out", "expr": "SUM(gen_ai.usage.cost_usd) / SUM(gen_ai.usage.output_tokens) * 1000" }] })), 6, 4, 6, 4),
                panel("guard", "Guardrail hits & cache", "table", q("spans", json!([{ "op": "COUNT" }]), json!([{ "field": "gen_ai_system", "op": "exists" }]), json!(["gen_ai.galileo.guardrail", "gen_ai.galileo.cache_kind"]), json!({ "time_range": { "last_seconds": 86400 } })), 0, 8, 6, 4),
                panel("md", "About", "markdown", json!({ "markdown": "**LLM board** — cost, calls, latency and quality per route and model. Use the `$route` and `$model` variables to focus. Feedback and judge scores live in AI → Evals." }), 6, 8, 6, 4),
            ],
        ),
        "browser" => (
            if b.name.is_empty() { "Browser vitals".into() } else { b.name.clone() },
            json!([{ "name": "page", "field": "url.path", "label": "Page", "default": "" }]),
            vec![
                panel("lcp", "LCP p75", "line", q("metrics", json!([{ "op": "P75", "field": "value" }]), json!([{ "field": "name", "op": "eq", "value": "browser.web_vital.lcp" }]), json!([]), json!({ "time_range": { "last_seconds": 86400 } })), 0, 0, 4, 4),
                panel("inp", "INP p75", "line", q("metrics", json!([{ "op": "P75", "field": "value" }]), json!([{ "field": "name", "op": "eq", "value": "browser.web_vital.inp" }]), json!([]), json!({ "time_range": { "last_seconds": 86400 } })), 4, 0, 4, 4),
                panel("cls", "CLS p75", "line", q("metrics", json!([{ "op": "P75", "field": "value" }]), json!([{ "field": "name", "op": "eq", "value": "browser.web_vital.cls" }]), json!([]), json!({ "time_range": { "last_seconds": 86400 } })), 8, 0, 4, 4),
                panel("pages", "Page views & JS errors", "line", q("spans", json!([{ "op": "COUNT" }]), json!([{ "field": "rum.type", "op": "in", "value": ["pageload", "navigation", "error"] }]), json!(["rum.type"]), json!({ "time_range": { "last_seconds": 86400 } })), 0, 4, 6, 4),
                panel("fetch", "Slowest fetches (p95)", "table", q("spans", json!([{ "op": "COUNT" }, { "op": "P95", "field": "duration_ms" }, { "op": "AVG", "field": "is_error" }]), json!([{ "field": "rum.type", "op": "eq", "value": "fetch" }]), json!(["http.url.path"]), json!({ "time_range": { "last_seconds": 86400 }, "orders": [{ "field": "P95(duration_ms)", "direction": "desc" }] })), 6, 4, 6, 4),
            ],
        ),
        other => return Err(ApiError::BadRequest(format!("unknown template '{other}' (red | llm | browser)"))),
    };
    let board = boards::create(&st.pg, pa.project.id, &name, &format!("from the {} template", b.kind), json!(panels)).await?;
    sqlx::query("UPDATE boards SET variables = $2, template = $3, time_range = $4 WHERE id = $1").bind(board.id).bind(&variables).bind(&b.kind).bind(json!({ "last_seconds": 3600 })).execute(&st.pg).await?;
    let board = boards::get(&st.pg, pa.project.id, board.id).await?.ok_or(ApiError::NotFound("board"))?;
    Ok(Json(json!({ "board": board })))
}

// ---------------------------------------------------------------- annotations

#[derive(Deserialize)]
pub struct AnnotationBody { #[serde(default = "d_kind")] pub kind: String, #[serde(default)] pub board_id: Option<Uuid>, #[serde(default)] pub panel_id: String, #[serde(default)] pub query_text: String, #[serde(default)] pub at: Option<chrono::DateTime<chrono::Utc>>, pub text: String, #[serde(default)] pub mentions: Vec<Uuid> }
fn d_kind() -> String { "chart".into() }

#[derive(Deserialize)]
pub struct AnnotationParams { #[serde(default)] pub board_id: Option<Uuid>, #[serde(default)] pub panel_id: Option<String>, #[serde(default = "d_last7")] pub last_seconds: i64, #[serde(default)] pub query_text: Option<String> }
fn d_last7() -> i64 { 7 * 86_400 }

pub async fn list_annotations(State(st): State<AppState>, pa: ProjectAccess, axum::extract::Query(p): axum::extract::Query<AnnotationParams>) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
        "SELECT row_to_json(a) FROM (SELECT an.id, an.kind, an.board_id, an.panel_id, an.query_text, an.at, an.text, an.mentions, an.created_at, u.email AS author, an.author_id          FROM annotations an LEFT JOIN users u ON u.id = an.author_id          WHERE an.project_id = $1 AND ($2::uuid IS NULL OR an.board_id = $2) AND ($3::text IS NULL OR an.panel_id = $3) AND ($4::text IS NULL OR an.query_text = $4)            AND (an.at IS NULL OR an.at > now() - make_interval(secs => $5)) ORDER BY an.at DESC NULLS LAST, an.created_at DESC LIMIT 200) a")
        .bind(pa.project.id).bind(p.board_id).bind(p.panel_id).bind(p.query_text).bind(p.last_seconds as f64).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "annotations": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}

pub async fn create_annotation(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<AnnotationBody>) -> ApiResult<Json<serde_json::Value>> {
    if b.text.trim().is_empty() { return Err(ApiError::BadRequest("text is required".into())); }
    let id = Uuid::now_v7();
    // @mentions by e-mail in the text resolve to org members
    let mut mentions = b.mentions.clone();
    for word in b.text.split_whitespace().filter(|w| w.starts_with('@') && w.contains('@') && w.len() > 3) {
        let email = word.trim_start_matches('@').trim_end_matches(|c: char| !c.is_alphanumeric());
        if let Ok(Some((uid,))) = sqlx::query_as::<_, (Uuid,)>("SELECT u.id FROM users u JOIN org_members m ON m.user_id = u.id WHERE m.org_id = $1 AND u.email = $2").bind(pa.project.org_id).bind(email).fetch_optional(&st.pg).await {
            if !mentions.contains(&uid) { mentions.push(uid); }
        }
    }
    sqlx::query("INSERT INTO annotations (id, project_id, kind, board_id, panel_id, query_text, at, text, mentions, author_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)")
        .bind(id).bind(pa.project.id).bind(&b.kind).bind(b.board_id).bind(&b.panel_id).bind(&b.query_text).bind(b.at).bind(b.text.trim()).bind(&mentions).bind(pa.user.id).execute(&st.pg).await?;
    // notify: the project's issue recipients carry the mention (members have no personal channels yet)
    if !mentions.is_empty() {
        let names: Vec<(String,)> = sqlx::query_as("SELECT email FROM users WHERE id = ANY($1)").bind(&mentions).fetch_all(&st.pg).await.unwrap_or_default();
        let recipients: Option<(serde_json::Value,)> = sqlx::query_as("SELECT issue_recipients FROM project_settings WHERE project_id = $1").bind(pa.project.id).fetch_optional(&st.pg).await.unwrap_or(None);
        let url = b.board_id.map(|bid| format!("{}/p/{}/boards/{}", st.config.public_url, pa.project.id, bid)).unwrap_or_else(|| format!("{}/p/{}/query", st.config.public_url, pa.project.id));
        let n = galileo_alerts::notify::Notification { kind: "annotation", state: "info".into(), name: "annotation".into(), project_id: pa.project.id, title: format!("{} mentioned {}", pa.user.email, names.iter().map(|n| n.0.as_str()).collect::<Vec<_>>().join(", ")), message: b.text.trim().to_string(), value: None, threshold: None, url: Some(url), at: chrono::Utc::now() };
        if let Some((r,)) = recipients { st.alerts.notify(pa.project.id, &r, &n).await; }
    }
    Ok(Json(json!({ "id": id, "mentions": mentions })))
}

#[derive(Deserialize)]
pub struct AnnotationPath { #[allow(dead_code)] pub project_id: Uuid, pub annotation_id: Uuid }
pub async fn delete_annotation(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<AnnotationPath>) -> ApiResult<Json<serde_json::Value>> {
    let n = sqlx::query("DELETE FROM annotations WHERE id = $1 AND project_id = $2 AND (author_id = $3 OR $4)").bind(p.annotation_id).bind(pa.project.id).bind(pa.user.id).bind(pa.role == "owner" || pa.role == "admin").execute(&st.pg).await?.rows_affected();
    if n == 0 { return Err(ApiError::NotFound("annotation")); }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- query history

pub async fn record_history(pg: &sqlx::PgPool, user: Uuid, project: Uuid, query: &galileo_query::Query) {
    let text = galileo_query::text::stringify(query);
    let recent: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM query_history WHERE user_id = $1 AND project_id = $2 AND text = $3 AND ran_at > now() - interval '1 minute' LIMIT 1").bind(user).bind(project).bind(&text).fetch_optional(pg).await.unwrap_or(None);
    if recent.is_some() { return; }
    let _ = sqlx::query("INSERT INTO query_history (id, user_id, project_id, query, text) VALUES ($1, $2, $3, $4, $5)").bind(Uuid::now_v7()).bind(user).bind(project).bind(serde_json::to_value(query).unwrap_or_default()).bind(&text).execute(pg).await;
    let _ = sqlx::query("DELETE FROM query_history WHERE user_id = $1 AND project_id = $2 AND id NOT IN (SELECT id FROM query_history WHERE user_id = $1 AND project_id = $2 ORDER BY ran_at DESC LIMIT 200)").bind(user).bind(project).execute(pg).await;
}

pub async fn query_history(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT row_to_json(h) FROM (SELECT id, query, text, ran_at FROM query_history WHERE user_id = $1 AND project_id = $2 ORDER BY ran_at DESC LIMIT 100) h").bind(pa.user.id).bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "history": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}

// ---------------------------------------------------------------- send an image to a channel

#[derive(Deserialize)]
pub struct ChannelPath { #[allow(dead_code)] pub project_id: Uuid, pub channel_id: Uuid }

/// POST a PNG (body: image/png bytes; `?caption=`) to a Discord/webhook channel as an attachment.
pub async fn send_image(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ChannelPath>, axum::extract::Query(cp): axum::extract::Query<CaptionParams>, body: axum::body::Bytes) -> ApiResult<Json<serde_json::Value>> {
    let row: Option<(String, serde_json::Value)> = sqlx::query_as("SELECT kind, config FROM notification_channels WHERE id = $1 AND project_id = $2").bind(p.channel_id).bind(pa.project.id).fetch_optional(&st.pg).await?;
    let Some((kind, config)) = row else { return Err(ApiError::NotFound("channel")); };
    let url = config.get("url").and_then(|u| u.as_str()).ok_or_else(|| ApiError::BadRequest("channel has no url".into()))?.to_string();
    if body.len() > 8_000_000 { return Err(ApiError::BadRequest("image too large".into())); }
    let caption = cp.caption.unwrap_or_else(|| "Galileo".into());
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::bytes(body.to_vec()).file_name("galileo.png").mime_str("image/png").map_err(|e| ApiError::Internal(e.to_string()))?;
    let form = match kind.as_str() {
        "discord" => reqwest::multipart::Form::new().text("payload_json", json!({ "content": caption }).to_string()).part("files[0]", part),
        "webhook" | "slack" => reqwest::multipart::Form::new().text("caption", caption.clone()).part("file", part),
        other => return Err(ApiError::BadRequest(format!("channel kind '{other}' cannot receive images (discord or webhook)"))),
    };
    let r = client.post(&url).multipart(form).send().await.map_err(|e| ApiError::BadRequest(format!("send failed: {e}")))?;
    let status = r.status().as_u16();
    if status >= 300 { return Err(ApiError::BadRequest(format!("channel returned {status}"))); }
    Ok(Json(json!({ "ok": true, "status": status })))
}
#[derive(Deserialize)]
pub struct CaptionParams { #[serde(default)] pub caption: Option<String> }
