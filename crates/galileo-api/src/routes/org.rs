//! Organization: cross-project overview, members and roles, invites, audit log, personal tokens,
//! per-project settings (retention).

use axum::extract::{Path, Query as QueryParams, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use galileo_query::TimeRange;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::audit;
use crate::auth::{self, CurrentUser, ProjectAccess};
use crate::db::{orgs, users};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct OrgPath { pub org_id: Uuid }

async fn require_role(st: &AppState, org: Uuid, cu: &CurrentUser, roles: &[&str]) -> ApiResult<String> {
    let role = orgs::role_for(&st.pg, org, cu.user.id).await?.ok_or(ApiError::NotFound("org"))?;
    if !roles.contains(&role.as_str()) { return Err(ApiError::Forbidden); }
    Ok(role)
}

#[derive(Deserialize)]
pub struct RangeParams { #[serde(default = "d_last")] pub last_seconds: i64 }
fn d_last() -> i64 { 3600 }

/// Every project of the org on one page.
pub async fn overview(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<OrgPath>, QueryParams(r): QueryParams<RangeParams>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin", "member", "viewer"]).await?;
    #[derive(FromRow, serde::Serialize)]
    struct Proj { id: Uuid, name: String, slug: String, created_at: DateTime<Utc>, open_issues: i64 }
    let projects: Vec<Proj> = sqlx::query_as(
        "SELECT p.id, p.name, p.slug, p.created_at, (SELECT count(*) FROM issues i WHERE i.project_id = p.id AND i.status = 'open') AS open_issues \
         FROM projects p WHERE p.org_id = $1 ORDER BY p.created_at").bind(p.org_id).fetch_all(&st.pg).await?;
    let (start, end) = TimeRange::Relative { last_seconds: r.last_seconds }.resolve(Utc::now());
    let ids: Vec<String> = projects.iter().map(|x| x.id.to_string()).collect();
    let mut stats: std::collections::HashMap<String, serde_json::Map<String, serde_json::Value>> = std::collections::HashMap::new();
    if !ids.is_empty() {
        let res = st.storage.query(&galileo_storage::SqlQuery {
            sql: "SELECT toString(project_id) AS pid, countIf(parent_span_id = '' AND http_route != '') AS requests, countIf(parent_span_id = '' AND status_code = 'error') AS errors, \
                  quantileTDigestIf(0.95)(duration_ns, parent_span_id = '') / 1e6 AS p95_ms, countIf(gen_ai_system != '') AS llm_calls, sum(gen_ai_cost_usd) AS llm_cost_usd, \
                  uniq(service_name) AS services, max(timestamp) AS last_seen, uniqIf(user_id, user_id != '') AS users \
                  FROM spans WHERE project_id IN (?) AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) GROUP BY pid".into(),
            params: vec![galileo_storage::SqlValue::StrList(ids), start.into(), end.into()],
        }).await?;
        for o in res.to_objects() { if let Some(pid) = o.get("pid").and_then(|v| v.as_str()) { stats.insert(pid.to_string(), o); } }
    }
    let out: Vec<serde_json::Value> = projects.into_iter().map(|pr| {
        let mut v = serde_json::to_value(&pr).unwrap_or(json!({}));
        if let Some(s) = stats.get(&pr.id.to_string()) { for (k, val) in s { if k != "pid" { v[k] = val.clone(); } } }
        v
    }).collect();
    Ok(Json(json!({ "projects": out, "start": start, "end": end })))
}

// ------------------------------------------------------------------ members

#[derive(FromRow, serde::Serialize)]
pub struct Member { pub user_id: Uuid, pub email: String, pub name: String, pub role: String, pub created_at: DateTime<Utc> }

pub async fn members(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<OrgPath>) -> ApiResult<Json<serde_json::Value>> {
    let my_role = require_role(&st, p.org_id, &cu, &["owner", "admin", "member", "viewer"]).await?;
    let rows: Vec<Member> = sqlx::query_as("SELECT m.user_id, u.email, u.name, m.role, m.created_at FROM org_members m JOIN users u ON u.id = m.user_id WHERE m.org_id = $1 ORDER BY m.created_at")
        .bind(p.org_id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "members": rows, "my_role": my_role })))
}

#[derive(Deserialize)] pub struct MemberPath { pub org_id: Uuid, pub user_id: Uuid }
#[derive(Deserialize)] pub struct RoleBody { pub role: String }

pub async fn set_role(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<MemberPath>, Json(b): Json<RoleBody>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner"]).await?;
    if !matches!(b.role.as_str(), "owner" | "admin" | "member" | "viewer") { return Err(ApiError::BadRequest("invalid role".into())); }
    let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM org_members WHERE org_id = $1 AND role = 'owner'").bind(p.org_id).fetch_one(&st.pg).await?;
    let target_role = orgs::role_for(&st.pg, p.org_id, p.user_id).await?.ok_or(ApiError::NotFound("member"))?;
    if target_role == "owner" && b.role != "owner" && owners <= 1 { return Err(ApiError::BadRequest("an organization needs at least one owner".into())); }
    sqlx::query("UPDATE org_members SET role = $3 WHERE org_id = $1 AND user_id = $2").bind(p.org_id).bind(p.user_id).bind(&b.role).execute(&st.pg).await?;
    audit::org(&st.pg, p.org_id, &cu, "member.role", "user", p.user_id, json!({ "from": target_role, "to": b.role })).await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_member(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<MemberPath>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin"]).await?;
    let target_role = orgs::role_for(&st.pg, p.org_id, p.user_id).await?.ok_or(ApiError::NotFound("member"))?;
    let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM org_members WHERE org_id = $1 AND role = 'owner'").bind(p.org_id).fetch_one(&st.pg).await?;
    if target_role == "owner" && owners <= 1 { return Err(ApiError::BadRequest("cannot remove the last owner".into())); }
    sqlx::query("DELETE FROM org_members WHERE org_id = $1 AND user_id = $2").bind(p.org_id).bind(p.user_id).execute(&st.pg).await?;
    audit::org(&st.pg, p.org_id, &cu, "member.remove", "user", p.user_id, json!({ "role": target_role })).await;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ invites

#[derive(FromRow, serde::Serialize)]
pub struct Invite { pub id: Uuid, pub email: String, pub role: String, pub created_at: DateTime<Utc>, pub expires_at: DateTime<Utc>, pub accepted_at: Option<DateTime<Utc>> }

#[derive(Deserialize)] pub struct InviteBody { pub email: String, #[serde(default = "d_member")] pub role: String }
fn d_member() -> String { "member".into() }

pub async fn list_invites(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<OrgPath>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin"]).await?;
    let rows: Vec<Invite> = sqlx::query_as("SELECT id, email, role, created_at, expires_at, accepted_at FROM org_invites WHERE org_id = $1 ORDER BY created_at DESC LIMIT 100").bind(p.org_id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "invites": rows })))
}

/// Creates an invite and returns the one-time link. E-mail delivery arrives with Phase 5; until
/// then the admin copies the link.
pub async fn create_invite(State(st): State<AppState>, cu: CurrentUser, headers: HeaderMap, Path(p): Path<OrgPath>, Json(b): Json<InviteBody>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin"]).await?;
    let email = b.email.trim().to_ascii_lowercase();
    if !email.contains('@') { return Err(ApiError::BadRequest("invalid email".into())); }
    if !matches!(b.role.as_str(), "admin" | "member" | "viewer") { return Err(ApiError::BadRequest("role must be admin, member or viewer".into())); }
    let token = auth::new_token();
    let row: Invite = sqlx::query_as("INSERT INTO org_invites (id, org_id, email, role, token_hash, invited_by, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, email, role, created_at, expires_at, accepted_at")
        .bind(Uuid::now_v7()).bind(p.org_id).bind(&email).bind(&b.role).bind(auth::hash_token(&token)).bind(cu.user.id).bind(Utc::now() + Duration::days(7))
        .fetch_one(&st.pg).await?;
    let origin = headers.get("origin").and_then(|v| v.to_str().ok()).map(str::to_owned).or_else(|| st.config.server.cors_origins.first().cloned()).unwrap_or_else(|| "http://localhost:3000".into());
    audit::org(&st.pg, p.org_id, &cu, "invite.create", "invite", row.id, json!({ "email": email, "role": b.role })).await;
    let link = format!("{origin}/invite/{token}");
    let mut emailed = false;
    if st.alerts.mailer.enabled() {
        let org_name: (String,) = sqlx::query_as("SELECT name FROM organizations WHERE id = $1").bind(p.org_id).fetch_one(&st.pg).await?;
        let html = format!("<div style=\"font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;max-width:560px;margin:0 auto;padding:24px\"><h2>You're invited to {} on Galileo</h2><p>{} invited you as <b>{}</b>.</p><p><a href=\"{}\" style=\"background:#f5a524;color:#1a1200;padding:10px 16px;border-radius:6px;text-decoration:none\">Accept invitation</a></p><p style=\"color:#888;font-size:12px\">The link works once and expires in 7 days.</p></div>", galileo_alerts::notify::html_escape(&org_name.0), galileo_alerts::notify::html_escape(&cu.user.email), b.role, link);
        emailed = st.alerts.mailer.send(std::slice::from_ref(&email), &format!("Invitation to {} on Galileo", org_name.0), &html, &format!("{} invited you to {} as {}. Accept: {}", cu.user.email, org_name.0, b.role, link)).await.is_ok();
    }
    Ok(Json(json!({ "invite": row, "link": link, "emailed": emailed })))
}

#[derive(Deserialize)] pub struct InvitePath { pub org_id: Uuid, pub invite_id: Uuid }

pub async fn revoke_invite(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<InvitePath>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin"]).await?;
    let r = sqlx::query("DELETE FROM org_invites WHERE id = $1 AND org_id = $2 AND accepted_at IS NULL").bind(p.invite_id).bind(p.org_id).execute(&st.pg).await?;
    if r.rows_affected() == 0 { return Err(ApiError::NotFound("invite")); }
    audit::org(&st.pg, p.org_id, &cu, "invite.revoke", "invite", p.invite_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)] pub struct TokenPath { pub token: String }
#[derive(FromRow)]
struct InviteFull { id: Uuid, org_id: Uuid, email: String, role: String, expires_at: DateTime<Utc>, accepted_at: Option<DateTime<Utc>>, org_name: String }

async fn load_invite(st: &AppState, token: &str) -> ApiResult<InviteFull> {
    let inv: InviteFull = sqlx::query_as("SELECT i.id, i.org_id, i.email, i.role, i.expires_at, i.accepted_at, o.name AS org_name FROM org_invites i JOIN organizations o ON o.id = i.org_id WHERE i.token_hash = $1")
        .bind(auth::hash_token(token)).fetch_optional(&st.pg).await?.ok_or(ApiError::NotFound("invite"))?;
    if inv.accepted_at.is_some() { return Err(ApiError::Conflict("invite already used".into())); }
    if inv.expires_at < Utc::now() { return Err(ApiError::Conflict("invite expired".into())); }
    Ok(inv)
}

/// Public: what the invite is for (so the accept page can show it).
pub async fn invite_info(State(st): State<AppState>, Path(p): Path<TokenPath>) -> ApiResult<Json<serde_json::Value>> {
    let inv = load_invite(&st, &p.token).await?;
    let exists = users::by_email(&st.pg, &inv.email).await?.is_some();
    Ok(Json(json!({ "org": inv.org_name, "email": inv.email, "role": inv.role, "expires_at": inv.expires_at, "user_exists": exists })))
}

#[derive(Deserialize)] pub struct AcceptBody { #[serde(default)] pub name: String, #[serde(default)] pub password: String }

/// Public: accept. Existing account → password must match; new account → created with the given password.
pub async fn invite_accept(State(st): State<AppState>, headers: HeaderMap, Path(p): Path<TokenPath>, Json(b): Json<AcceptBody>) -> ApiResult<impl IntoResponse> {
    let inv = load_invite(&st, &p.token).await?;
    let user = match users::by_email(&st.pg, &inv.email).await? {
        Some(u) => { if !auth::verify_password(&b.password, &u.password_hash) { return Err(ApiError::Unauthorized); } u }
        None => {
            if b.password.len() < 8 { return Err(ApiError::BadRequest("password must be at least 8 characters".into())); }
            let name = if b.name.trim().is_empty() { inv.email.split('@').next().unwrap_or("user").to_string() } else { b.name.trim().to_string() };
            users::create(&st.pg, &inv.email, &name, &auth::hash_password(&b.password)?).await?
        }
    };
    sqlx::query("INSERT INTO org_members (org_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role")
        .bind(inv.org_id).bind(user.id).bind(&inv.role).execute(&st.pg).await?;
    sqlx::query("UPDATE org_invites SET accepted_at = now(), accepted_by = $2 WHERE id = $1").bind(inv.id).bind(user.id).execute(&st.pg).await?;
    audit::record(&st.pg, inv.org_id, None, Some((user.id, &user.email)), "invite.accept", "invite", &inv.id.to_string(), json!({ "role": inv.role })).await;
    let token = auth::new_token();
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
    users::create_session(&st.pg, user.id, &auth::hash_token(&token), auth::session_expiry(), ua).await?;
    let first_project: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM projects WHERE org_id = $1 ORDER BY created_at LIMIT 1").bind(inv.org_id).fetch_optional(&st.pg).await?;
    Ok((auth::set_cookie_header(auth::session_cookie(&token, auth::secure_cookies(&st.config))), Json(json!({ "user": user, "org_id": inv.org_id, "project_id": first_project.map(|p| p.0), "token": token }))))
}

// ------------------------------------------------------------------ personal API tokens

#[derive(FromRow, serde::Serialize)]
pub struct ApiToken { pub id: Uuid, pub name: String, pub token_prefix: String, pub created_at: DateTime<Utc>, pub expires_at: Option<DateTime<Utc>>, pub last_used_at: Option<DateTime<Utc>>, pub revoked_at: Option<DateTime<Utc>> }

pub async fn list_tokens(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<ApiToken> = sqlx::query_as("SELECT id, name, token_prefix, created_at, expires_at, last_used_at, revoked_at FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC").bind(cu.user.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "tokens": rows })))
}

#[derive(Deserialize)] pub struct TokenBody { pub name: String, #[serde(default)] pub expires_days: Option<i64> }

pub async fn create_token(State(st): State<AppState>, cu: CurrentUser, Json(b): Json<TokenBody>) -> ApiResult<Json<serde_json::Value>> {
    if b.name.trim().is_empty() { return Err(ApiError::BadRequest("name required".into())); }
    let raw = format!("glt_{}", auth::new_token());
    let row: ApiToken = sqlx::query_as("INSERT INTO api_tokens (id, user_id, name, token_hash, token_prefix, expires_at) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, name, token_prefix, created_at, expires_at, last_used_at, revoked_at")
        .bind(Uuid::now_v7()).bind(cu.user.id).bind(b.name.trim()).bind(auth::hash_token(&raw)).bind(&raw[..12]).bind(b.expires_days.map(|d| Utc::now() + Duration::days(d.clamp(1, 3650))))
        .fetch_one(&st.pg).await?;
    Ok(Json(json!({ "api_token": row, "token": raw })))
}

#[derive(Deserialize)] pub struct TokenIdPath { pub token_id: Uuid }

pub async fn revoke_token(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<TokenIdPath>) -> ApiResult<Json<serde_json::Value>> {
    let r = sqlx::query("UPDATE api_tokens SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL").bind(p.token_id).bind(cu.user.id).execute(&st.pg).await?;
    if r.rows_affected() == 0 { return Err(ApiError::NotFound("token")); }
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ audit

#[derive(FromRow, serde::Serialize)]
pub struct AuditRow { pub id: Uuid, pub project_id: Option<Uuid>, pub user_email: String, pub action: String, pub target_type: String, pub target_id: String, pub details: serde_json::Value, pub at: DateTime<Utc> }

pub async fn org_audit(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<OrgPath>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin"]).await?;
    let rows: Vec<AuditRow> = sqlx::query_as("SELECT id, project_id, user_email, action, target_type, target_id, details, at FROM audit_log WHERE org_id = $1 ORDER BY at DESC LIMIT 300").bind(p.org_id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "audit": rows })))
}

pub async fn project_audit(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<AuditRow> = sqlx::query_as("SELECT id, project_id, user_email, action, target_type, target_id, details, at FROM audit_log WHERE project_id = $1 ORDER BY at DESC LIMIT 300").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "audit": rows })))
}

// ------------------------------------------------------------------ project settings (retention)

#[derive(FromRow, serde::Serialize)]
pub struct ProjectSettings { pub retention_spans_days: Option<i32>, pub retention_logs_days: Option<i32>, pub retention_metrics_days: Option<i32>, pub retention_last_run: Option<DateTime<Utc>>, pub sampling: serde_json::Value }

pub async fn get_project_settings(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let s: Option<ProjectSettings> = sqlx::query_as("SELECT retention_spans_days, retention_logs_days, retention_metrics_days, retention_last_run, sampling FROM project_settings WHERE project_id = $1").bind(pa.project.id).fetch_optional(&st.pg).await?;
    let g = &st.config.retention;
    Ok(Json(json!({ "settings": s.unwrap_or(ProjectSettings { retention_spans_days: None, retention_logs_days: None, retention_metrics_days: None, retention_last_run: None, sampling: serde_json::to_value(galileo_core::Sampling::default()).unwrap_or_default() }),
        "global": { "spans_days": g.spans.as_secs() / 86400, "logs_days": g.logs.as_secs() / 86400, "metrics_days": g.metrics.as_secs() / 86400 } })))
}

#[derive(Deserialize)] pub struct SettingsBody { pub retention_spans_days: Option<i32>, pub retention_logs_days: Option<i32>, pub retention_metrics_days: Option<i32>, #[serde(default)] pub sampling: Option<galileo_core::Sampling> }

pub async fn put_project_settings(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<SettingsBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    for v in [b.retention_spans_days, b.retention_logs_days, b.retention_metrics_days].into_iter().flatten() {
        if !(1..=3650).contains(&v) { return Err(ApiError::BadRequest("retention must be 1..3650 days".into())); }
    }
    sqlx::query("INSERT INTO project_settings (project_id, retention_spans_days, retention_logs_days, retention_metrics_days) VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (project_id) DO UPDATE SET retention_spans_days = $2, retention_logs_days = $3, retention_metrics_days = $4, retention_last_run = NULL, updated_at = now()")
        .bind(pa.project.id).bind(b.retention_spans_days).bind(b.retention_logs_days).bind(b.retention_metrics_days).execute(&st.pg).await?;
    if let Some(sampling) = b.sampling.clone() {
        let sampling = sampling.normalized();
        if !(0.0..=1.0).contains(&sampling.rate) { return Err(ApiError::BadRequest("sampling.rate must be 0..1".into())); }
        sqlx::query("UPDATE project_settings SET sampling = $2, updated_at = now() WHERE project_id = $1")
            .bind(pa.project.id).bind(serde_json::to_value(&sampling).unwrap_or_default()).execute(&st.pg).await?;
        st.resolver.invalidate_all();
        audit::project(&st.pg, &pa, "project.sampling", "project", pa.project.id, serde_json::to_value(&sampling).unwrap_or_default()).await;
    }
    audit::project(&st.pg, &pa, "project.retention", "project", pa.project.id, json!({ "spans": b.retention_spans_days, "logs": b.retention_logs_days, "metrics": b.retention_metrics_days })).await;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- assistant settings (org level)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantSettings {
    #[serde(default)]
    pub enabled: bool,
    /// Project whose gateway route answers and where the assistant's own calls are recorded.
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub route_alias: String,
    #[serde(default = "d_steps")]
    pub max_steps: u32,
}
fn d_steps() -> u32 { 6 }
impl Default for AssistantSettings {
    fn default() -> Self { Self { enabled: false, project_id: None, route_alias: String::new(), max_steps: 6 } }
}

pub async fn assistant_settings_for(pg: &sqlx::PgPool, org: Uuid) -> AssistantSettings {
    let row: Option<(serde_json::Value,)> = sqlx::query_as("SELECT assistant FROM org_settings WHERE org_id = $1").bind(org).fetch_optional(pg).await.unwrap_or(None);
    row.and_then(|(v,)| serde_json::from_value(v).ok()).unwrap_or_default()
}

pub async fn get_assistant(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<OrgPath>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin", "member", "viewer"]).await?;
    let s = assistant_settings_for(&st.pg, p.org_id).await;
    // routes available in the chosen project, for the picker
    let routes: Vec<(String,)> = match s.project_id {
        Some(pid) => sqlx::query_as("SELECT alias FROM gateway_routes WHERE project_id = $1 AND enabled ORDER BY alias").bind(pid).fetch_all(&st.pg).await.unwrap_or_default(),
        None => vec![],
    };
    Ok(Json(json!({ "assistant": s, "routes": routes.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}

pub async fn put_assistant(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<OrgPath>, Json(b): Json<AssistantSettings>) -> ApiResult<Json<serde_json::Value>> {
    require_role(&st, p.org_id, &cu, &["owner", "admin"]).await?;
    let mut b = b;
    b.max_steps = b.max_steps.clamp(1, 12);
    if let Some(pid) = b.project_id {
        let ok: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM projects WHERE id = $1 AND org_id = $2").bind(pid).bind(p.org_id).fetch_optional(&st.pg).await?;
        if ok.is_none() { return Err(ApiError::BadRequest("project is not in this organization".into())); }
    }
    if b.enabled && (b.project_id.is_none() || b.route_alias.trim().is_empty()) {
        return Err(ApiError::BadRequest("choose a project and a gateway route before enabling".into()));
    }
    sqlx::query("INSERT INTO org_settings (org_id, assistant) VALUES ($1, $2) ON CONFLICT (org_id) DO UPDATE SET assistant = $2, updated_at = now()")
        .bind(p.org_id).bind(serde_json::to_value(&b).unwrap_or_default()).execute(&st.pg).await?;
    audit::org(&st.pg, p.org_id, &cu, "org.assistant", "org", p.org_id, serde_json::to_value(&b).unwrap_or_default()).await;
    Ok(Json(json!({ "ok": true, "assistant": b })))
}
