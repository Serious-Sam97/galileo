//! Gateway configuration: providers (with encrypted keys), routes, prompts, and usage rollups.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::{DateTime, Utc};
use galileo_core::ProjectId;
use galileo_query::TimeRange;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// ---------------------------------------------------------------- providers

#[derive(Debug, FromRow, Serialize)]
pub struct ProviderRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub has_key: bool,
    pub headers: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const PROVIDER_COLS: &str = "id, project_id, name, kind, base_url, api_key_enc IS NOT NULL AS has_key, headers, created_at, updated_at";

pub async fn list_providers(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<ProviderRow> = sqlx::query_as(&format!("SELECT {PROVIDER_COLS} FROM gateway_providers WHERE project_id = $1 ORDER BY name"))
        .bind(pa.project.id)
        .fetch_all(&st.pg)
        .await?;
    Ok(Json(json!({ "providers": rows })))
}

#[derive(Deserialize)]
pub struct ProviderBody {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Omit to keep the existing key on update; empty string clears it.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "empty_obj")]
    pub headers: serde_json::Value,
}
fn empty_obj() -> serde_json::Value {
    json!({})
}

fn default_base_url(kind: &str) -> &'static str {
    match kind {
        "anthropic" => "https://api.anthropic.com",
        "openai" => "https://api.openai.com",
        "ollama" => "http://127.0.0.1:11434",
        _ => "",
    }
}

fn validate_provider(b: &ProviderBody) -> ApiResult<String> {
    if !matches!(b.kind.as_str(), "anthropic" | "openai" | "ollama" | "openai_compatible") {
        return Err(ApiError::BadRequest("kind must be anthropic, openai, ollama or openai_compatible".into()));
    }
    if b.name.trim().is_empty() || b.name.contains('/') {
        return Err(ApiError::BadRequest("name is required and cannot contain '/'".into()));
    }
    let base = b.base_url.clone().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| default_base_url(&b.kind).to_string());
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(ApiError::BadRequest("base_url must start with http:// or https://".into()));
    }
    if !b.headers.is_object() {
        return Err(ApiError::BadRequest("headers must be an object".into()));
    }
    Ok(base)
}

pub async fn create_provider(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<ProviderBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let base = validate_provider(&b)?;
    let enc = b.api_key.as_deref().filter(|k| !k.is_empty()).map(|k| galileo_gateway::crypto::encrypt(&st.secret, k));
    let row: ProviderRow = sqlx::query_as(&format!(
        "INSERT INTO gateway_providers (id, project_id, name, kind, base_url, api_key_enc, headers) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {PROVIDER_COLS}"
    ))
    .bind(Uuid::now_v7())
    .bind(pa.project.id)
    .bind(b.name.trim())
    .bind(&b.kind)
    .bind(&base)
    .bind(enc)
    .bind(&b.headers)
    .fetch_one(&st.pg)
    .await?;
    st.gateway.invalidate();
    audit::project(&st.pg, &pa, "provider.create", "provider", row.id, json!({ "name": row.name, "kind": row.kind, "base_url": row.base_url })).await;
    Ok(Json(json!({ "provider": row })))
}

#[derive(Deserialize)]
pub struct ProviderPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub provider_id: Uuid,
}

pub async fn update_provider(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ProviderPath>, Json(b): Json<ProviderBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let base = validate_provider(&b)?;
    let row: Option<ProviderRow> = match b.api_key.as_deref() {
        None => sqlx::query_as(&format!(
            "UPDATE gateway_providers SET name = $3, kind = $4, base_url = $5, headers = $6, updated_at = now() WHERE id = $1 AND project_id = $2 RETURNING {PROVIDER_COLS}"
        ))
        .bind(p.provider_id).bind(pa.project.id).bind(b.name.trim()).bind(&b.kind).bind(&base).bind(&b.headers)
        .fetch_optional(&st.pg).await?,
        Some(k) => {
            let enc = (!k.is_empty()).then(|| galileo_gateway::crypto::encrypt(&st.secret, k));
            sqlx::query_as(&format!(
                "UPDATE gateway_providers SET name = $3, kind = $4, base_url = $5, headers = $6, api_key_enc = $7, updated_at = now() WHERE id = $1 AND project_id = $2 RETURNING {PROVIDER_COLS}"
            ))
            .bind(p.provider_id).bind(pa.project.id).bind(b.name.trim()).bind(&b.kind).bind(&base).bind(&b.headers).bind(enc)
            .fetch_optional(&st.pg).await?
        }
    };
    let row = row.ok_or(ApiError::NotFound("provider"))?;
    st.gateway.invalidate();
    audit::project(&st.pg, &pa, "provider.update", "provider", row.id, json!({ "name": row.name, "kind": row.kind, "base_url": row.base_url, "key_changed": b.api_key.is_some() })).await;
    Ok(Json(json!({ "provider": row })))
}

pub async fn delete_provider(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ProviderPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let r = sqlx::query("DELETE FROM gateway_providers WHERE id = $1 AND project_id = $2").bind(p.provider_id).bind(pa.project.id).execute(&st.pg).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound("provider"));
    }
    st.gateway.invalidate();
    audit::project(&st.pg, &pa, "provider.delete", "provider", p.provider_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

/// Ask the provider for its model list (best effort; useful in the route editor).
pub async fn provider_models(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ProviderPath>) -> ApiResult<Json<serde_json::Value>> {
    let cfg = st.gateway.routes.get(pa.project.id, &st.secret).await;
    let prov = cfg.provider(p.provider_id).ok_or(ApiError::NotFound("provider"))?;
    let base = prov.base_url.trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    let mut req = st.gateway.http.get(format!("{base}/v1/models"));
    match prov.kind {
        galileo_gateway::routing::ProviderKind::Anthropic => {
            req = req.header("anthropic-version", "2023-06-01");
            if let Some(k) = &prov.api_key {
                req = req.header("x-api-key", k);
            }
        }
        _ => {
            if let Some(k) = &prov.api_key {
                req = req.bearer_auth(k);
            }
        }
    }
    let resp = req.send().await.map_err(|e| ApiError::BadRequest(format!("provider unreachable: {e}")))?;
    let status = resp.status().as_u16();
    let v: serde_json::Value = resp.json().await.unwrap_or(json!({}));
    let models: Vec<String> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_owned)).collect())
        .unwrap_or_default();
    Ok(Json(json!({ "status": status, "models": models })))
}

// ---------------------------------------------------------------- routes

#[derive(Debug, FromRow, Serialize)]
pub struct RouteRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub alias: String,
    pub description: String,
    pub targets: serde_json::Value,
    pub budget: serde_json::Value,
    pub rate_limit: serde_json::Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn list_routes(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<RouteRow> = sqlx::query_as("SELECT * FROM gateway_routes WHERE project_id = $1 ORDER BY alias").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "routes": rows })))
}

#[derive(Deserialize)]
pub struct RouteBody {
    pub alias: String,
    #[serde(default)]
    pub description: String,
    pub targets: Vec<galileo_gateway::routing::Target>,
    #[serde(default)]
    pub budget: serde_json::Value,
    #[serde(default)]
    pub rate_limit: serde_json::Value,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}

async fn validate_route(st: &AppState, project: Uuid, b: &RouteBody) -> ApiResult<()> {
    let alias = b.alias.trim();
    if alias.is_empty() || alias.contains('/') || alias.len() > 100 {
        return Err(ApiError::BadRequest("alias is required, max 100 chars, no '/'".into()));
    }
    if b.targets.is_empty() {
        return Err(ApiError::BadRequest("at least one target is required".into()));
    }
    for t in &b.targets {
        if t.model.trim().is_empty() {
            return Err(ApiError::BadRequest("every target needs a model".into()));
        }
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM gateway_providers WHERE id = $1 AND project_id = $2")
            .bind(t.provider_id)
            .bind(project)
            .fetch_one(&st.pg)
            .await?;
        if n == 0 {
            return Err(ApiError::BadRequest(format!("provider {} does not belong to this project", t.provider_id)));
        }
    }
    let budget = if b.budget.is_null() { json!({}) } else { b.budget.clone() };
    serde_json::from_value::<galileo_gateway::routing::Budget>(budget).map_err(|e| ApiError::BadRequest(format!("budget: {e}")))?;
    let rl = if b.rate_limit.is_null() { json!({}) } else { b.rate_limit.clone() };
    serde_json::from_value::<galileo_gateway::routing::RateLimit>(rl).map_err(|e| ApiError::BadRequest(format!("rate_limit: {e}")))?;
    Ok(())
}

pub async fn create_route(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<RouteBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate_route(&st, pa.project.id, &b).await?;
    let row: RouteRow = sqlx::query_as(
        "INSERT INTO gateway_routes (id, project_id, alias, description, targets, budget, rate_limit, enabled) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(pa.project.id)
    .bind(b.alias.trim())
    .bind(&b.description)
    .bind(serde_json::to_value(&b.targets).unwrap())
    .bind(if b.budget.is_null() { json!({}) } else { b.budget })
    .bind(if b.rate_limit.is_null() { json!({}) } else { b.rate_limit })
    .bind(b.enabled)
    .fetch_one(&st.pg)
    .await?;
    st.gateway.invalidate();
    audit::project(&st.pg, &pa, "route.create", "route", row.id, json!({ "alias": row.alias, "targets": row.targets, "budget": row.budget, "rate_limit": row.rate_limit })).await;
    Ok(Json(json!({ "route": row })))
}

#[derive(Deserialize)]
pub struct RoutePath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub route_id: Uuid,
}

pub async fn update_route(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<RoutePath>, Json(b): Json<RouteBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate_route(&st, pa.project.id, &b).await?;
    let row: Option<RouteRow> = sqlx::query_as(
        "UPDATE gateway_routes SET alias = $3, description = $4, targets = $5, budget = $6, rate_limit = $7, enabled = $8, updated_at = now() \
         WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(p.route_id)
    .bind(pa.project.id)
    .bind(b.alias.trim())
    .bind(&b.description)
    .bind(serde_json::to_value(&b.targets).unwrap())
    .bind(if b.budget.is_null() { json!({}) } else { b.budget })
    .bind(if b.rate_limit.is_null() { json!({}) } else { b.rate_limit })
    .bind(b.enabled)
    .fetch_optional(&st.pg)
    .await?;
    let row = row.ok_or(ApiError::NotFound("route"))?;
    st.gateway.invalidate();
    audit::project(&st.pg, &pa, "route.update", "route", row.id, json!({ "alias": row.alias, "targets": row.targets, "budget": row.budget, "rate_limit": row.rate_limit, "enabled": row.enabled })).await;
    Ok(Json(json!({ "route": row })))
}

pub async fn delete_route(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<RoutePath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let r = sqlx::query("DELETE FROM gateway_routes WHERE id = $1 AND project_id = $2").bind(p.route_id).bind(pa.project.id).execute(&st.pg).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound("route"));
    }
    st.gateway.invalidate();
    audit::project(&st.pg, &pa, "route.delete", "route", p.route_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- prompts

#[derive(Debug, FromRow, Serialize)]
pub struct PromptRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    #[sqlx(default)] pub ci: Option<serde_json::Value>,
    #[sqlx(default)] pub promoted_version: Option<i32>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct PromptVersionRow {
    pub id: Uuid,
    pub prompt_id: Uuid,
    pub version: i32,
    pub content: serde_json::Value,
    pub note: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    #[sqlx(default)] pub ci_status: Option<String>,
    #[sqlx(default)] pub ci_score: Option<f64>,
}

pub async fn list_prompts(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    #[derive(FromRow, Serialize)]
    struct Row {
        id: Uuid,
        name: String,
        description: String,
        created_at: DateTime<Utc>,
        latest_version: Option<i32>,
        versions: i64,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT p.id, p.name, p.description, p.created_at, max(v.version) AS latest_version, count(v.id) AS versions \
         FROM prompts p LEFT JOIN prompt_versions v ON v.prompt_id = p.id WHERE p.project_id = $1 GROUP BY p.id ORDER BY p.name",
    )
    .bind(pa.project.id)
    .fetch_all(&st.pg)
    .await?;
    Ok(Json(json!({ "prompts": rows })))
}

#[derive(Deserialize)]
pub struct PromptBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Optional first version: {system?, messages: [{role, content}], variables?: [..]}
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(default)]
    pub note: String,
}

fn validate_content(c: &serde_json::Value) -> ApiResult<()> {
    if !c.is_object() {
        return Err(ApiError::BadRequest("content must be an object".into()));
    }
    if let Some(m) = c.get("messages") {
        let arr = m.as_array().ok_or_else(|| ApiError::BadRequest("content.messages must be an array".into()))?;
        for x in arr {
            if x.get("role").and_then(|r| r.as_str()).is_none() || x.get("content").and_then(|r| r.as_str()).is_none() {
                return Err(ApiError::BadRequest("each message needs string role and content".into()));
            }
        }
    }
    Ok(())
}

pub async fn create_prompt(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<PromptBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let name = b.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::BadRequest("name is required (max 100 chars)".into()));
    }
    if let Some(c) = &b.content {
        validate_content(c)?;
    }
    let mut tx = st.pg.begin().await?;
    let prompt: PromptRow = sqlx::query_as("INSERT INTO prompts (id, project_id, name, description) VALUES ($1, $2, $3, $4) RETURNING *")
        .bind(Uuid::now_v7())
        .bind(pa.project.id)
        .bind(name)
        .bind(&b.description)
        .fetch_one(&mut *tx)
        .await?;
    let mut version: Option<PromptVersionRow> = None;
    if let Some(c) = b.content {
        version = Some(
            sqlx::query_as("INSERT INTO prompt_versions (id, prompt_id, version, content, note, created_by) VALUES ($1, $2, 1, $3, $4, $5) RETURNING *")
                .bind(Uuid::now_v7())
                .bind(prompt.id)
                .bind(c)
                .bind(&b.note)
                .bind(pa.user.id)
                .fetch_one(&mut *tx)
                .await?,
        );
    }
    tx.commit().await?;
    Ok(Json(json!({ "prompt": prompt, "version": version })))
}

#[derive(Deserialize)]
pub struct PromptPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub prompt_id: Uuid,
}

pub async fn get_prompt(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<PromptPath>) -> ApiResult<Json<serde_json::Value>> {
    let prompt: PromptRow = sqlx::query_as("SELECT * FROM prompts WHERE id = $1 AND project_id = $2")
        .bind(p.prompt_id)
        .bind(pa.project.id)
        .fetch_optional(&st.pg)
        .await?
        .ok_or(ApiError::NotFound("prompt"))?;
    let versions: Vec<PromptVersionRow> = sqlx::query_as("SELECT * FROM prompt_versions WHERE prompt_id = $1 ORDER BY version DESC")
        .bind(prompt.id)
        .fetch_all(&st.pg)
        .await?;
    Ok(Json(json!({ "prompt": prompt, "versions": versions })))
}

#[derive(Deserialize)]
pub struct VersionBody {
    pub content: serde_json::Value,
    #[serde(default)]
    pub note: String,
}

pub async fn add_prompt_version(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<PromptPath>, Json(b): Json<VersionBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate_content(&b.content)?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM prompts WHERE id = $1 AND project_id = $2").bind(p.prompt_id).bind(pa.project.id).fetch_one(&st.pg).await?;
    if n == 0 {
        return Err(ApiError::NotFound("prompt"));
    }
    let v: PromptVersionRow = sqlx::query_as(
        "INSERT INTO prompt_versions (id, prompt_id, version, content, note, created_by) \
         VALUES ($1, $2, (SELECT coalesce(max(version), 0) + 1 FROM prompt_versions WHERE prompt_id = $2), $3, $4, $5) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(p.prompt_id)
    .bind(&b.content)
    .bind(&b.note)
    .bind(pa.user.id)
    .fetch_one(&st.pg)
    .await?;
    // prompt CI: judge the new version against its dataset in the background
    let st2 = st.clone(); let (pid, prid, ver, uid) = (pa.project.id, p.prompt_id, v.version, pa.user.id);
    tokio::spawn(async move { run_prompt_ci(st2, pid, prid, ver, Some(uid)).await; });
    Ok(Json(json!({ "version": v })))
}

pub async fn delete_prompt(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<PromptPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let r = sqlx::query("DELETE FROM prompts WHERE id = $1 AND project_id = $2").bind(p.prompt_id).bind(pa.project.id).execute(&st.pg).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound("prompt"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- usage

#[derive(Deserialize)]
pub struct UsageParams {
    #[serde(default = "default_last")]
    pub last_seconds: i64,
}
fn default_last() -> i64 {
    7 * 86400
}

/// Cost/token rollups by model, route and day for the AI dashboard.
pub async fn usage(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<UsageParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let pid = ProjectId(pa.project.id);
    let base = "FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND gen_ai_system != ''";
    let params = || vec![pid.into(), start.into(), end.into()];
    let by_model = st.storage.query(&galileo_storage::SqlQuery { sql: format!(
        "SELECT gen_ai_model AS model, gen_ai_system AS system, count() AS calls, sum(gen_ai_input_tokens) AS input_tokens, sum(gen_ai_output_tokens) AS output_tokens, \
         sum(gen_ai_cost_usd) AS cost_usd, quantileTDigest(0.5)(duration_ns)/1e6 AS p50_ms, quantileTDigest(0.95)(duration_ns)/1e6 AS p95_ms, countIf(status_code = 'error') AS errors {base} GROUP BY model, system ORDER BY cost_usd DESC LIMIT 100"), params: params() }).await?;
    let by_route = st.storage.query(&galileo_storage::SqlQuery { sql: format!(
        "SELECT attrs['gen_ai.galileo.route'] AS route, count() AS calls, sum(gen_ai_input_tokens + gen_ai_output_tokens) AS tokens, sum(gen_ai_cost_usd) AS cost_usd, \
         countIf(attrs['gen_ai.galileo.fallback_index'] != '0' AND attrs['gen_ai.galileo.fallback_index'] != '') AS fallbacks, countIf(status_code = 'error') AS errors {base} GROUP BY route ORDER BY cost_usd DESC LIMIT 100"), params: params() }).await?;
    let by_day = st.storage.query(&galileo_storage::SqlQuery { sql: format!(
        "SELECT toDate(timestamp) AS day, gen_ai_model AS model, count() AS calls, sum(gen_ai_cost_usd) AS cost_usd, sum(gen_ai_input_tokens + gen_ai_output_tokens) AS tokens {base} GROUP BY day, model ORDER BY day, model"), params: params() }).await?;
    let totals = st.storage.query(&galileo_storage::SqlQuery { sql: format!(
        "SELECT count() AS calls, sum(gen_ai_cost_usd) AS cost_usd, sum(gen_ai_input_tokens) AS input_tokens, sum(gen_ai_output_tokens) AS output_tokens, countIf(status_code = 'error') AS errors, uniq(trace_id) AS traces {base}"), params: params() }).await?;
    Ok(Json(json!({
        "start": start, "end": end,
        "totals": totals.to_objects().into_iter().next(),
        "by_model": by_model.to_objects(),
        "by_route": by_route.to_objects(),
        "by_day": by_day.to_objects(),
    })))
}

// ---------------------------------------------------------------- feedback + evals (quality)

#[derive(Deserialize)]
pub struct FeedbackBody { pub span_id: String, #[serde(default)] pub trace_id: String, pub rating: i16, #[serde(default)] pub comment: String }

/// Feedback from the Galileo UI (a reviewer), as opposed to /gw/v1/feedback from the app's end users.
pub async fn feedback(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<FeedbackBody>) -> ApiResult<Json<serde_json::Value>> {
    if !(-1..=5).contains(&b.rating) || b.rating == 0 { return Err(ApiError::BadRequest("rating must be -1, 1 or 1..5".into())); }
    sqlx::query("INSERT INTO gateway_feedback (id, project_id, span_id, trace_id, rating, comment, user_id) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(Uuid::now_v7()).bind(pa.project.id).bind(&b.span_id).bind(&b.trace_id).bind(b.rating).bind(b.comment.chars().take(2000).collect::<String>()).bind(&pa.user.email).execute(&st.pg).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SpanIds { pub span_ids: Vec<String> }

/// Feedback + eval scores for a set of spans (the calls table asks for what it shows).
pub async fn quality_for_spans(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<SpanIds>) -> ApiResult<Json<serde_json::Value>> {
    let ids: Vec<String> = b.span_ids.into_iter().take(500).collect();
    let fb: Vec<(String, i16, String, String)> = sqlx::query_as("SELECT span_id, rating, comment, user_id FROM gateway_feedback WHERE project_id = $1 AND span_id = ANY($2) ORDER BY created_at").bind(pa.project.id).bind(&ids).fetch_all(&st.pg).await?;
    let ev: Vec<(String, Option<i16>, String)> = sqlx::query_as("SELECT span_id, score, reasoning FROM gateway_evals WHERE project_id = $1 AND span_id = ANY($2) ORDER BY created_at DESC").bind(pa.project.id).bind(&ids).fetch_all(&st.pg).await?;
    let mut out: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for (sid, rating, comment, user) in fb { out.entry(sid).or_insert(json!({ "feedback": [], "evals": [] }))["feedback"].as_array_mut().unwrap().push(json!({ "rating": rating, "comment": comment, "user": user })); }
    for (sid, score, reasoning) in ev { out.entry(sid).or_insert(json!({ "feedback": [], "evals": [] }))["evals"].as_array_mut().unwrap().push(json!({ "score": score, "reasoning": reasoning })); }
    Ok(Json(json!({ "quality": out })))
}

#[derive(Deserialize)]
pub struct EvalRunBody { pub judge_route: String, #[serde(default = "d_sample")] pub sample_size: i32, #[serde(default)] pub rubric: String, #[serde(default)] pub filter_route: String, #[serde(default = "d_last")] pub last_seconds: i64 }
fn d_last() -> i64 { 86_400 }
fn d_sample() -> i32 { 20 }

const DEFAULT_RUBRIC: &str = "You are grading an AI assistant's answer. Score 1-5: 5 = correct, complete, well formatted and faithful to the request; 3 = partially useful or with minor errors; 1 = wrong, empty, refuses without reason, or ignores the request. Reply ONLY with JSON: {\"score\": <1-5>, \"reasoning\": \"<one sentence>\"}.";

/// LLM-as-judge over a sample of recent completions. Runs inline (bounded sample) and stores per-span scores.
pub async fn run_eval(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<EvalRunBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let sample = b.sample_size.clamp(1, 100);
    let rubric = if b.rubric.trim().is_empty() { DEFAULT_RUBRIC.to_string() } else { b.rubric.clone() };
    let run_id = Uuid::now_v7();
    sqlx::query("INSERT INTO gateway_eval_runs (id, project_id, judge_route, rubric, sample_size, filter_route, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(run_id).bind(pa.project.id).bind(&b.judge_route).bind(&rubric).bind(sample).bind(&b.filter_route).bind(pa.user.id).execute(&st.pg).await?;
    let (start, end) = TimeRange::Relative { last_seconds: b.last_seconds }.resolve(Utc::now());
    let mut params: Vec<galileo_storage::SqlValue> = vec![ProjectId(pa.project.id).into(), start.into(), end.into()];
    let mut extra = String::new();
    if !b.filter_route.is_empty() { extra = " AND attrs['gen_ai.galileo.route'] = ?".into(); params.push(b.filter_route.clone().into()); }
    let rows = st.storage.query(&galileo_storage::SqlQuery {
        sql: format!("SELECT span_id, trace_id, gen_ai_model, attrs['gen_ai.galileo.route'], attrs['gen_ai.galileo.prompt.name'], attrs['gen_ai.galileo.prompt.version'], attrs['gen_ai.prompt'], attrs['gen_ai.completion'] \
                      FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND gen_ai_system != '' AND status_code != 'error' AND attrs['gen_ai.completion'] != ''{extra} \
                      AND span_id NOT IN (SELECT span_id FROM spans WHERE 0) ORDER BY rand() LIMIT {sample}"),
        params,
    }).await?;
    let mut scored = 0; let mut total = 0.0f64; let mut error = String::new();
    for r in &rows.rows {
        let g = |i: usize| r.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (span_id, trace_id, model, route, pname, pver, prompt, completion) = (g(0), g(1), g(2), g(3), g(4), g(5), g(6), g(7));
        let user = format!("REQUEST:\n{}\n\nANSWER:\n{}", prompt.chars().take(6000).collect::<String>(), completion.chars().take(6000).collect::<String>());
        match st.gateway.simple_chat(pa.project.id, &b.judge_route, Some(&rubric), &user, 400).await {
            Ok(text) => {
                let parsed: Option<serde_json::Value> = text.find('{').and_then(|i| text.rfind('}').map(|j| (i, j))).and_then(|(i, j)| serde_json::from_str(&text[i..=j]).ok());
                let score = parsed.as_ref().and_then(|p| p.get("score")).and_then(|s| s.as_i64().or_else(|| s.as_f64().map(|f| f.round() as i64))).map(|s| s.clamp(1, 5) as i16);
                let reasoning = parsed.as_ref().and_then(|p| p.get("reasoning")).and_then(|s| s.as_str()).unwrap_or(&text).chars().take(500).collect::<String>();
                sqlx::query("INSERT INTO gateway_evals (id, run_id, project_id, span_id, trace_id, model, route, prompt_name, prompt_version, score, reasoning) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)")
                    .bind(Uuid::now_v7()).bind(run_id).bind(pa.project.id).bind(&span_id).bind(&trace_id).bind(&model).bind(&route).bind(&pname).bind(&pver).bind(score).bind(&reasoning).execute(&st.pg).await?;
                if let Some(sc) = score { scored += 1; total += sc as f64; }
            }
            Err(e) => { error = e.to_string(); break; }
        }
    }
    let avg = (scored > 0).then(|| total / scored as f64);
    sqlx::query("UPDATE gateway_eval_runs SET status = $2, scored = $3, avg_score = $4, error = $5, finished_at = now() WHERE id = $1")
        .bind(run_id).bind(if error.is_empty() { "done" } else { "failed" }).bind(scored).bind(avg).bind(&error).execute(&st.pg).await?;
    Ok(Json(json!({ "run_id": run_id, "sampled": rows.rows.len(), "scored": scored, "avg_score": avg, "error": error })))
}

#[derive(FromRow, Serialize)]
pub struct EvalRun { pub id: Uuid, pub judge_route: String, pub rubric: String, pub sample_size: i32, pub filter_route: String, pub status: String, pub scored: i32, pub avg_score: Option<f64>, pub error: String, pub created_at: DateTime<Utc>, pub finished_at: Option<DateTime<Utc>> }

pub async fn list_evals(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let runs: Vec<EvalRun> = sqlx::query_as("SELECT id, judge_route, rubric, sample_size, filter_route, status, scored, avg_score, error, created_at, finished_at FROM gateway_eval_runs WHERE project_id = $1 ORDER BY created_at DESC LIMIT 50").bind(pa.project.id).fetch_all(&st.pg).await?;
    #[derive(FromRow, Serialize)]
    struct Agg { key: String, n: i64, avg: Option<f64> }
    let by_model: Vec<Agg> = sqlx::query_as("SELECT model AS key, count(*) AS n, avg(score)::float8 AS avg FROM gateway_evals WHERE project_id = $1 AND score IS NOT NULL GROUP BY model ORDER BY n DESC LIMIT 20").bind(pa.project.id).fetch_all(&st.pg).await?;
    let by_route: Vec<Agg> = sqlx::query_as("SELECT route AS key, count(*) AS n, avg(score)::float8 AS avg FROM gateway_evals WHERE project_id = $1 AND score IS NOT NULL GROUP BY route ORDER BY n DESC LIMIT 20").bind(pa.project.id).fetch_all(&st.pg).await?;
    let by_prompt: Vec<Agg> = sqlx::query_as("SELECT prompt_name || '@' || prompt_version AS key, count(*) AS n, avg(score)::float8 AS avg FROM gateway_evals WHERE project_id = $1 AND score IS NOT NULL AND prompt_name != '' GROUP BY prompt_name, prompt_version ORDER BY n DESC LIMIT 20").bind(pa.project.id).fetch_all(&st.pg).await?;
    let fb: Vec<Agg> = sqlx::query_as("SELECT 'all' AS key, count(*) AS n, avg(CASE WHEN rating = -1 THEN 0 WHEN rating = 1 THEN 1 ELSE (rating - 1) / 4.0 END)::float8 AS avg FROM gateway_feedback WHERE project_id = $1").bind(pa.project.id).fetch_all(&st.pg).await?;
    let latest: Vec<(String, Option<i16>, String, String, String)> = sqlx::query_as("SELECT span_id, score, reasoning, model, route FROM gateway_evals WHERE project_id = $1 ORDER BY created_at DESC LIMIT 30").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "runs": runs, "by_model": by_model, "by_route": by_route, "by_prompt": by_prompt, "feedback": fb.first(), "latest": latest.into_iter().map(|(s, sc, r, m, ro)| json!({ "span_id": s, "score": sc, "reasoning": r, "model": m, "route": ro })).collect::<Vec<_>>() })))
}

// ---------------------------------------------------------------- datasets + prompt CI

#[derive(Deserialize)]
pub struct DatasetBody { pub name: String, #[serde(default)] pub description: String }

pub async fn list_datasets(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT row_to_json(d) FROM (SELECT ds.id, ds.name, ds.description, ds.created_at, (SELECT count(*) FROM gateway_dataset_items i WHERE i.dataset_id = ds.id) AS items FROM gateway_datasets ds WHERE ds.project_id = $1 ORDER BY ds.name) d").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "datasets": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}

pub async fn create_dataset(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<DatasetBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if b.name.trim().is_empty() { return Err(ApiError::BadRequest("name is required".into())); }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO gateway_datasets (id, project_id, name, description, created_by) VALUES ($1, $2, $3, $4, $5)").bind(id).bind(pa.project.id).bind(b.name.trim()).bind(&b.description).bind(pa.user.id).execute(&st.pg).await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct DatasetPath { #[allow(dead_code)] pub project_id: Uuid, pub dataset_id: Uuid }

pub async fn get_dataset(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<DatasetPath>) -> ApiResult<Json<serde_json::Value>> {
    let ds: Option<(serde_json::Value,)> = sqlx::query_as("SELECT row_to_json(d) FROM (SELECT id, name, description, created_at FROM gateway_datasets WHERE id = $1 AND project_id = $2) d").bind(p.dataset_id).bind(pa.project.id).fetch_optional(&st.pg).await?;
    let Some((ds,)) = ds else { return Err(ApiError::NotFound("dataset")); };
    let items: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT row_to_json(i) FROM (SELECT id, input, expected, rubric, source_span, created_at FROM gateway_dataset_items WHERE dataset_id = $1 ORDER BY created_at) i").bind(p.dataset_id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "dataset": ds, "items": items.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}

pub async fn delete_dataset(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<DatasetPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    sqlx::query("DELETE FROM gateway_datasets WHERE id = $1 AND project_id = $2").bind(p.dataset_id).bind(pa.project.id).execute(&st.pg).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ItemsBody {
    /// Build items from recorded calls (their prompt becomes the input, completion the expected answer).
    #[serde(default)] pub span_ids: Vec<String>,
    /// Or explicit items.
    #[serde(default)] pub items: Vec<serde_json::Value>,
}

pub async fn add_items(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<DatasetPath>, Json(b): Json<ItemsBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let mut added = 0;
    if !b.span_ids.is_empty() {
        let ids: Vec<String> = b.span_ids.into_iter().take(200).collect();
        let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let mut params: Vec<galileo_storage::SqlValue> = vec![ProjectId(pa.project.id).into()];
        params.extend(ids.iter().map(|x| galileo_storage::SqlValue::from(x.clone())));
        let rows = st.storage.query(&galileo_storage::SqlQuery { sql: format!("SELECT span_id, attrs['gen_ai.prompt'], attrs['gen_ai.completion'] FROM spans WHERE project_id = ? AND span_id IN ({ph}) AND gen_ai_system != ''"), params }).await?;
        for r in &rows.rows {
            let g = |i: usize| r.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let (sid, prompt, completion) = (g(0), g(1), g(2));
            if prompt.is_empty() { continue; }
            // recorded prompt text is "[role] content" lines; keep the last user turn as the input
            let user_turn = prompt.split("\n[").filter(|l| l.starts_with("user] ") || l.starts_with("[user] ")).last().map(|l| l.trim_start_matches('[').trim_start_matches("user] ").to_string()).unwrap_or(prompt.clone());
            sqlx::query("INSERT INTO gateway_dataset_items (id, dataset_id, input, expected, source_span) VALUES ($1, $2, $3, $4, $5)")
                .bind(Uuid::now_v7()).bind(p.dataset_id).bind(json!({ "messages": [{ "role": "user", "content": user_turn }] })).bind(&completion).bind(&sid).execute(&st.pg).await?;
            added += 1;
        }
    }
    for it in b.items.into_iter().take(500) {
        let input = it.get("input").cloned().unwrap_or_else(|| json!({ "messages": [{ "role": "user", "content": it.get("prompt").and_then(|x| x.as_str()).unwrap_or("") }] }));
        sqlx::query("INSERT INTO gateway_dataset_items (id, dataset_id, input, expected, rubric) VALUES ($1, $2, $3, $4, $5)")
            .bind(Uuid::now_v7()).bind(p.dataset_id).bind(&input).bind(it.get("expected").and_then(|x| x.as_str()).unwrap_or("")).bind(it.get("rubric").and_then(|x| x.as_str()).unwrap_or("")).execute(&st.pg).await?;
        added += 1;
    }
    Ok(Json(json!({ "added": added })))
}

#[derive(Deserialize)]
pub struct ItemPath { #[allow(dead_code)] pub project_id: Uuid, pub dataset_id: Uuid, pub item_id: Uuid }
pub async fn delete_item(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ItemPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    sqlx::query("DELETE FROM gateway_dataset_items WHERE id = $1 AND dataset_id IN (SELECT id FROM gateway_datasets WHERE id = $2 AND project_id = $3)").bind(p.item_id).bind(p.dataset_id).bind(pa.project.id).execute(&st.pg).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Prompt CI configuration: which dataset, which route runs the candidate, which judges, the bar.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptCi {
    #[serde(default)] pub dataset_id: Option<Uuid>,
    #[serde(default)] pub run_route: String,
    #[serde(default)] pub judge_route: String,
    #[serde(default = "d_min")] pub min_score: f64,
    #[serde(default)] pub required: bool,
}
fn d_min() -> f64 { 3.5 }

#[derive(Deserialize)]
pub struct PromptPatch { #[serde(default)] pub description: Option<String>, #[serde(default)] pub ci: Option<PromptCi> }

pub async fn patch_prompt(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<PromptPath>, Json(b): Json<PromptPatch>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if let Some(d) = &b.description { sqlx::query("UPDATE prompts SET description = $3 WHERE id = $1 AND project_id = $2").bind(p.prompt_id).bind(pa.project.id).bind(d).execute(&st.pg).await?; }
    if let Some(ci) = &b.ci { sqlx::query("UPDATE prompts SET ci = $3 WHERE id = $1 AND project_id = $2").bind(p.prompt_id).bind(pa.project.id).bind(serde_json::to_value(ci).unwrap_or_default()).execute(&st.pg).await?; }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PromoteBody { pub version: i32 }

/// Promote a version: it becomes what the gateway serves when the app omits the version.
pub async fn promote_prompt(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<PromptPath>, Json(b): Json<PromoteBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let row: Option<(serde_json::Value, String, Option<f64>)> = sqlx::query_as("SELECT pr.ci, pv.ci_status, pv.ci_score FROM prompts pr JOIN prompt_versions pv ON pv.prompt_id = pr.id WHERE pr.id = $1 AND pr.project_id = $2 AND pv.version = $3").bind(p.prompt_id).bind(pa.project.id).bind(b.version).fetch_optional(&st.pg).await?;
    let Some((ci, status, score)) = row else { return Err(ApiError::NotFound("prompt version")); };
    let ci: PromptCi = serde_json::from_value(ci).unwrap_or_default();
    if ci.required && ci.dataset_id.is_some() && status != "passed" {
        return Err(ApiError::BadRequest(format!("version {} has not passed CI (status {status}{}); required by this prompt's CI settings", b.version, score.map(|s| format!(", score {s:.2}")).unwrap_or_default())));
    }
    sqlx::query("UPDATE prompts SET promoted_version = $3 WHERE id = $1 AND project_id = $2").bind(p.prompt_id).bind(pa.project.id).bind(b.version).execute(&st.pg).await?;
    audit::project(&st.pg, &pa, "prompt.promote", "prompt", p.prompt_id, json!({ "version": b.version })).await;
    Ok(Json(json!({ "ok": true, "promoted_version": b.version })))
}

/// Run CI for one prompt version: for each dataset item, inject the version's content + item input
/// through `run_route`, then judge against expected/rubric through `judge_route`. Stores the score
/// on the version and a prompt_ci eval run. Spawned after a version is created; also callable.
pub async fn run_prompt_ci(st: AppState, project: Uuid, prompt_id: Uuid, version: i32, user: Option<Uuid>) {
    let row: Option<(serde_json::Value, serde_json::Value, String)> = sqlx::query_as("SELECT pr.ci, pv.content, pr.name FROM prompts pr JOIN prompt_versions pv ON pv.prompt_id = pr.id WHERE pr.id = $1 AND pv.version = $2").bind(prompt_id).bind(version).fetch_optional(&st.pg).await.unwrap_or(None);
    let Some((ci_v, content, pname)) = row else { return };
    let ci: PromptCi = serde_json::from_value(ci_v).unwrap_or_default();
    let (Some(ds), false, false) = (ci.dataset_id, ci.run_route.is_empty(), ci.judge_route.is_empty()) else { return };
    let run_id = Uuid::now_v7();
    let _ = sqlx::query("UPDATE prompt_versions SET ci_status = 'running', ci_run_id = $3 WHERE prompt_id = $1 AND version = $2").bind(prompt_id).bind(version).bind(run_id).execute(&st.pg).await;
    let _ = sqlx::query("INSERT INTO gateway_eval_runs (id, project_id, judge_route, rubric, sample_size, filter_route, created_by, kind, prompt_id, prompt_version, dataset_id) VALUES ($1, $2, $3, $4, 0, $5, $6, 'prompt_ci', $7, $8, $9)")
        .bind(run_id).bind(project).bind(&ci.judge_route).bind("prompt CI").bind(&ci.run_route).bind(user).bind(prompt_id).bind(version).bind(ds).execute(&st.pg).await;
    let items: Vec<(Uuid, serde_json::Value, String, String)> = sqlx::query_as("SELECT id, input, expected, rubric FROM gateway_dataset_items WHERE dataset_id = $1 ORDER BY created_at LIMIT 100").bind(ds).fetch_all(&st.pg).await.unwrap_or_default();
    let system = content.get("system").and_then(|s| s.as_str()).map(str::to_owned);
    let pre: Vec<(String, String)> = content.get("messages").and_then(|m| m.as_array()).map(|a| a.iter().filter_map(|m| Some((m.get("role")?.as_str()?.to_string(), m.get("content")?.as_str()?.to_string()))).collect()).unwrap_or_default();
    let mut scored = 0; let mut total = 0.0; let mut error = String::new();
    for (item_id, input, expected, rubric) in &items {
        let mut msgs: Vec<(String, String)> = vec![];
        if let Some(sys) = &system { msgs.push(("system".into(), sys.clone())); }
        msgs.extend(pre.iter().cloned());
        for m in input.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default() { if let (Some(r), Some(c)) = (m.get("role").and_then(|x| x.as_str()), m.get("content").and_then(|x| x.as_str())) { msgs.push((r.into(), c.into())); } }
        let answer = match st.gateway.chat_recorded(project, &ci.run_route, "prompt_ci", &msgs, 800, 0.2, None).await { Ok(r) => r.text, Err(e) => { error = format!("run: {e}"); break; } };
        let judge_prompt = format!("Grade the ANSWER to the REQUEST on a 1-5 scale.{}{} Reply ONLY with JSON {{\"score\": <1-5>, \"reasoning\": \"<one sentence>\"}}.\n\nREQUEST:\n{}\n\nANSWER:\n{}",
            if expected.is_empty() { String::new() } else { format!(" A reference answer is given; 5 = equivalent in meaning and completeness, 1 = wrong or missing.\nREFERENCE:\n{}\n", expected.chars().take(3000).collect::<String>()) },
            if rubric.is_empty() { String::new() } else { format!(" Rubric: {rubric}") },
            input.get("messages").and_then(|m| m.as_array()).and_then(|a| a.last()).and_then(|m| m.get("content")).and_then(|c| c.as_str()).unwrap_or("").chars().take(3000).collect::<String>(), answer.chars().take(4000).collect::<String>());
        let verdict = match st.gateway.chat_recorded(project, &ci.judge_route, "prompt_ci.judge", &[("user".into(), judge_prompt)], 600, 0.0, None).await { Ok(r) => r.text, Err(e) => { error = format!("judge: {e}"); break; } };
        let parsed: Option<serde_json::Value> = verdict.find('{').and_then(|i| verdict.rfind('}').map(|j| (i, j))).and_then(|(i, j)| serde_json::from_str(&verdict[i..=j]).ok());
        let score = parsed.as_ref().and_then(|p| p.get("score")).and_then(|s| s.as_i64().or_else(|| s.as_f64().map(|f| f.round() as i64))).map(|s| s.clamp(1, 5) as i16);
        let reasoning = parsed.as_ref().and_then(|p| p.get("reasoning")).and_then(|s| s.as_str()).unwrap_or("").chars().take(500).collect::<String>();
        let _ = sqlx::query("INSERT INTO gateway_evals (id, run_id, project_id, span_id, trace_id, model, route, prompt_name, prompt_version, score, reasoning) VALUES ($1, $2, $3, $4, '', '', $5, $6, $7, $8, $9)")
            .bind(Uuid::now_v7()).bind(run_id).bind(project).bind(item_id.to_string()).bind(&ci.run_route).bind(&pname).bind(version.to_string()).bind(score).bind(&reasoning).execute(&st.pg).await;
        if let Some(sc) = score { scored += 1; total += sc as f64; }
    }
    let avg = (scored > 0).then(|| total / scored as f64);
    let status = if !error.is_empty() { "error" } else if avg.map(|a| a >= ci.min_score).unwrap_or(false) { "passed" } else { "failed" };
    let _ = sqlx::query("UPDATE gateway_eval_runs SET status = $2, scored = $3, avg_score = $4, error = $5, sample_size = $6, finished_at = now() WHERE id = $1").bind(run_id).bind(if error.is_empty() { "done" } else { "failed" }).bind(scored).bind(avg).bind(&error).bind(items.len() as i32).execute(&st.pg).await;
    let _ = sqlx::query("UPDATE prompt_versions SET ci_status = $3, ci_score = $4 WHERE prompt_id = $1 AND version = $2").bind(prompt_id).bind(version).bind(status).bind(avg).execute(&st.pg).await;
}

#[derive(Deserialize)]
pub struct CiRunBody { pub version: i32 }
/// Re-run CI for a version by hand.
pub async fn run_ci(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<PromptPath>, Json(b): Json<CiRunBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let st2 = st.clone(); let (pid, prid, ver, uid) = (pa.project.id, p.prompt_id, b.version, pa.user.id);
    tokio::spawn(async move { run_prompt_ci(st2, pid, prid, ver, Some(uid)).await; });
    Ok(Json(json!({ "started": true })))
}
