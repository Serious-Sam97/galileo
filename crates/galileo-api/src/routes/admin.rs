//! Account management, for the Master only: create users with a temporary password, reset
//! passwords, disable accounts, place users in organizations and projects, and grant or remove
//! single permissions on top of their role.

use axum::extract::{Path, Query as QueryParams, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::{self, CurrentUser};
use crate::db::users;
use crate::error::{ApiError, ApiResult};
use crate::perms::{self, Perm, PermSet};
use crate::state::AppState;

/// How long a temporary password works if nobody uses it.
const TEMP_PASSWORD_HOURS: i64 = 72;
const ORG_ROLES: [&str; 4] = ["owner", "admin", "member", "viewer"];
const PROJECT_ROLES: [&str; 3] = ["admin", "editor", "viewer"];

/// A readable temporary password: four groups of four, without look-alike characters
/// (no 0/O, 1/l/I). About 95 bits of entropy.
fn temporary_password() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::rng();
    (0..4)
        .map(|_| (0..4).map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char).collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

async fn audit_user(st: &AppState, cu: &CurrentUser, target: Uuid, action: &str, details: serde_json::Value) {
    // Recorded in every organization the target belongs to, so each org's audit log tells the story.
    let orgs: Vec<(Uuid,)> = sqlx::query_as("SELECT org_id FROM org_members WHERE user_id = $1").bind(target).fetch_all(&st.pg).await.unwrap_or_default();
    for (org,) in orgs {
        crate::audit::org(&st.pg, org, cu, action, "user", target, details.clone()).await;
    }
}

async fn masters(st: &AppState) -> ApiResult<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM users WHERE is_master AND disabled_at IS NULL").fetch_one(&st.pg).await?)
}

async fn target(st: &AppState, id: Uuid) -> ApiResult<users::User> {
    users::by_id(&st.pg, id).await?.ok_or(ApiError::NotFound("user"))
}

#[derive(FromRow, serde::Serialize)]
struct UserRow {
    id: Uuid,
    email: String,
    name: String,
    created_at: DateTime<Utc>,
    is_master: bool,
    must_change_password: bool,
    temp_password_expires_at: Option<DateTime<Utc>>,
    disabled_at: Option<DateTime<Utc>>,
    last_login_at: Option<DateTime<Utc>>,
    locked_until: Option<DateTime<Utc>>,
    totp_enabled: bool,
    sso: bool,
    sessions: i64,
    orgs: serde_json::Value,
    projects: serde_json::Value,
    overrides: serde_json::Value,
}

/// Every account with its organizations, project roles and overrides.
pub async fn list_users(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let rows: Vec<UserRow> = sqlx::query_as(
        "SELECT u.id, u.email, u.name, u.created_at, u.is_master, u.must_change_password, u.temp_password_expires_at, \
                u.disabled_at, u.last_login_at, u.locked_until, u.totp_enabled, u.oidc_subject IS NOT NULL AS sso, \
                (SELECT count(*) FROM sessions s WHERE s.user_id = u.id AND s.expires_at > now()) AS sessions, \
                coalesce((SELECT json_agg(json_build_object('org_id', o.id, 'name', o.name, 'role', m.role) ORDER BY o.created_at) \
                          FROM org_members m JOIN organizations o ON o.id = m.org_id WHERE m.user_id = u.id), '[]') AS orgs, \
                coalesce((SELECT json_agg(json_build_object('project_id', p.id, 'org_id', p.org_id, 'name', p.name, 'role', pm.role)) \
                          FROM project_members pm JOIN projects p ON p.id = pm.project_id WHERE pm.user_id = u.id), '[]') AS projects, \
                coalesce((SELECT json_agg(json_build_object('org_id', mp.org_id, 'permission', mp.permission, 'allow', mp.allow)) \
                          FROM member_permissions mp WHERE mp.user_id = u.id), '[]') AS overrides \
         FROM users u ORDER BY u.created_at",
    )
    .fetch_all(&st.pg)
    .await?;
    Ok(Json(json!({ "users": rows })))
}

/// The permission catalog, the role presets and every organization with its projects: what the
/// account screens need to render choices.
pub async fn catalog(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let permissions: Vec<_> = perms::ALL.iter().map(|p| json!({ "key": p.key(), "label": p.label(), "description": p.description() })).collect();
    let presets: serde_json::Map<String, serde_json::Value> = ["viewer", "editor", "member", "admin", "owner"]
        .iter()
        .map(|r| (r.to_string(), json!(PermSet::of(perms::preset(r)).keys())))
        .collect();
    let orgs: Vec<(serde_json::Value,)> = sqlx::query_as(
        "SELECT json_build_object('id', o.id, 'name', o.name, 'projects', \
                coalesce((SELECT json_agg(json_build_object('id', p.id, 'name', p.name) ORDER BY p.created_at) FROM projects p WHERE p.org_id = o.id), '[]')) \
         FROM organizations o ORDER BY o.created_at",
    )
    .fetch_all(&st.pg)
    .await?;
    Ok(Json(json!({ "permissions": permissions, "presets": presets, "org_roles": ORG_ROLES, "project_roles": PROJECT_ROLES, "orgs": orgs.into_iter().map(|r| r.0).collect::<Vec<_>>(), "temp_password_hours": TEMP_PASSWORD_HOURS })))
}

#[derive(Deserialize)]
pub struct ProjectGrant {
    pub project_id: Uuid,
    pub role: String,
}

#[derive(Deserialize)]
pub struct CreateUser {
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub org_id: Uuid,
    pub role: String,
    /// Project roles that differ from what the organization role gives.
    #[serde(default)]
    pub projects: Vec<ProjectGrant>,
}

/// Creates an account with a temporary password, shown once in this response and nowhere else.
pub async fn create_user(State(st): State<AppState>, cu: CurrentUser, Json(b): Json<CreateUser>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let email = b.email.trim().to_ascii_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return Err(ApiError::BadRequest("invalid e-mail".into()));
    }
    if !ORG_ROLES.contains(&b.role.as_str()) {
        return Err(ApiError::BadRequest(format!("role must be one of {}", ORG_ROLES.join(", "))));
    }
    if b.projects.iter().any(|g| !PROJECT_ROLES.contains(&g.role.as_str())) {
        return Err(ApiError::BadRequest(format!("project role must be one of {}", PROJECT_ROLES.join(", "))));
    }
    if users::by_email(&st.pg, &email).await?.is_some() {
        return Err(ApiError::Conflict("an account with this e-mail exists".into()));
    }
    let org_exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM organizations WHERE id = $1").bind(b.org_id).fetch_optional(&st.pg).await?;
    org_exists.ok_or(ApiError::NotFound("org"))?;
    let name = if b.name.trim().is_empty() { email.split('@').next().unwrap_or("").to_string() } else { b.name.trim().to_string() };
    let password = temporary_password();
    let expires = Utc::now() + Duration::hours(TEMP_PASSWORD_HOURS);

    let mut tx = st.pg.begin().await?;
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, name, password_hash, must_change_password, temp_password_expires_at) VALUES ($1, $2, $3, $4, true, $5)")
        .bind(id).bind(&email).bind(&name).bind(auth::hash_password(&password)?).bind(expires)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO org_members (org_id, user_id, role) VALUES ($1, $2, $3)").bind(b.org_id).bind(id).bind(&b.role).execute(&mut *tx).await?;
    for g in &b.projects {
        let n = sqlx::query("INSERT INTO project_members (project_id, user_id, role) SELECT id, $2, $3 FROM projects WHERE id = $1 AND org_id = $4")
            .bind(g.project_id).bind(id).bind(&g.role).bind(b.org_id).execute(&mut *tx).await?.rows_affected();
        if n == 0 { return Err(ApiError::BadRequest("a project is not in the chosen organization".into())); }
    }
    tx.commit().await?;
    audit_user(&st, &cu, id, "user.create", json!({ "email": email, "role": b.role, "projects": b.projects.len() })).await;
    let user = target(&st, id).await?;
    Ok(Json(json!({ "user": user, "temporary_password": password, "expires_at": expires })))
}

#[derive(Deserialize)]
pub struct UserPath {
    pub user_id: Uuid,
}

#[derive(Deserialize)]
pub struct PatchUser {
    pub name: Option<String>,
    pub is_master: Option<bool>,
    pub disabled: Option<bool>,
    /// Clears a lockout after repeated failed sign-ins.
    pub unlock: Option<bool>,
}

pub async fn patch_user(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<UserPath>, Json(b): Json<PatchUser>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let u = target(&st, p.user_id).await?;
    let me = u.id == cu.user.id;
    if let Some(name) = b.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        sqlx::query("UPDATE users SET name = $2 WHERE id = $1").bind(u.id).bind(name).execute(&st.pg).await?;
    }
    if let Some(m) = b.is_master.filter(|m| *m != u.is_master) {
        if !m && (me || masters(&st).await? <= 1) {
            return Err(ApiError::Coded(StatusCode::CONFLICT, "last_master", "the instance needs at least one other Master first".into()));
        }
        sqlx::query("UPDATE users SET is_master = $2 WHERE id = $1").bind(u.id).bind(m).execute(&st.pg).await?;
        audit_user(&st, &cu, u.id, if m { "user.master_grant" } else { "user.master_revoke" }, json!({})).await;
    }
    if let Some(d) = b.disabled.filter(|d| *d != u.disabled_at.is_some()) {
        if d && me {
            return Err(ApiError::Coded(StatusCode::CONFLICT, "self", "you cannot disable your own account".into()));
        }
        if d && u.is_master && masters(&st).await? <= 1 {
            return Err(ApiError::Coded(StatusCode::CONFLICT, "last_master", "the last Master cannot be disabled".into()));
        }
        sqlx::query("UPDATE users SET disabled_at = CASE WHEN $2 THEN now() ELSE NULL END WHERE id = $1").bind(u.id).bind(d).execute(&st.pg).await?;
        if d {
            users::delete_sessions(&st.pg, u.id, None).await?;
        }
        audit_user(&st, &cu, u.id, if d { "user.disable" } else { "user.enable" }, json!({})).await;
    }
    if b.unlock == Some(true) {
        sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1").bind(u.id).execute(&st.pg).await?;
        audit_user(&st, &cu, u.id, "user.unlock", json!({})).await;
    }
    Ok(Json(json!({ "user": target(&st, u.id).await? })))
}

/// A new temporary password for the user: signs them out everywhere and makes them choose a new
/// password at the next sign-in. The password is in this response only.
pub async fn reset_password(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<UserPath>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let u = target(&st, p.user_id).await?;
    if u.id == cu.user.id {
        return Err(ApiError::Coded(StatusCode::CONFLICT, "self", "change your own password from your account page".into()));
    }
    let password = temporary_password();
    let expires = Utc::now() + Duration::hours(TEMP_PASSWORD_HOURS);
    users::set_temporary_password(&st.pg, u.id, &auth::hash_password(&password)?, expires).await?;
    let signed_out = users::delete_sessions(&st.pg, u.id, None).await?;
    sqlx::query("UPDATE api_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL").bind(u.id).execute(&st.pg).await?;
    audit_user(&st, &cu, u.id, "user.password_reset", json!({ "signed_out_sessions": signed_out })).await;
    Ok(Json(json!({ "temporary_password": password, "expires_at": expires, "signed_out_sessions": signed_out })))
}

/// Deletes an account. Its history stays (audit entries keep the e-mail); its tokens and sessions go.
pub async fn delete_user(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<UserPath>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let u = target(&st, p.user_id).await?;
    if u.id == cu.user.id {
        return Err(ApiError::Coded(StatusCode::CONFLICT, "self", "you cannot delete your own account".into()));
    }
    if u.is_master && masters(&st).await? <= 1 {
        return Err(ApiError::Coded(StatusCode::CONFLICT, "last_master", "the last Master cannot be deleted".into()));
    }
    audit_user(&st, &cu, u.id, "user.delete", json!({ "email": u.email })).await;
    sqlx::query("DELETE FROM users WHERE id = $1").bind(u.id).execute(&st.pg).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct UserOrgPath {
    pub user_id: Uuid,
    pub org_id: Uuid,
}

#[derive(Deserialize)]
pub struct RoleBody {
    /// `null` removes the membership (or the project-specific role).
    pub role: Option<String>,
}

async fn owners(st: &AppState, org: Uuid) -> ApiResult<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM org_members WHERE org_id = $1 AND role = 'owner'").bind(org).fetch_one(&st.pg).await?)
}

/// Adds the user to an organization, changes their role there, or removes them (`role: null`).
pub async fn set_org_role(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<UserOrgPath>, Json(b): Json<RoleBody>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    target(&st, p.user_id).await?;
    let current: Option<(String,)> = sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2").bind(p.org_id).bind(p.user_id).fetch_optional(&st.pg).await?;
    let losing_owner = current.as_ref().is_some_and(|(r,)| r == "owner") && b.role.as_deref() != Some("owner");
    if losing_owner && owners(&st, p.org_id).await? <= 1 {
        return Err(ApiError::Coded(StatusCode::CONFLICT, "last_owner", "the organization needs another owner first".into()));
    }
    match b.role.as_deref() {
        Some(r) if ORG_ROLES.contains(&r) => {
            sqlx::query("INSERT INTO org_members (org_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT (org_id, user_id) DO UPDATE SET role = $3")
                .bind(p.org_id).bind(p.user_id).bind(r).execute(&st.pg).await?;
        }
        Some(_) => return Err(ApiError::BadRequest(format!("role must be one of {}", ORG_ROLES.join(", ")))),
        None => {
            sqlx::query("DELETE FROM org_members WHERE org_id = $1 AND user_id = $2").bind(p.org_id).bind(p.user_id).execute(&st.pg).await?;
            sqlx::query("DELETE FROM project_members WHERE user_id = $1 AND project_id IN (SELECT id FROM projects WHERE org_id = $2)").bind(p.user_id).bind(p.org_id).execute(&st.pg).await?;
            sqlx::query("DELETE FROM member_permissions WHERE user_id = $1 AND org_id = $2").bind(p.user_id).bind(p.org_id).execute(&st.pg).await?;
        }
    }
    crate::audit::org(&st.pg, p.org_id, &cu, "user.org_role", "user", p.user_id, json!({ "from": current.map(|c| c.0), "to": b.role })).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct UserProjectPath {
    pub user_id: Uuid,
    pub project_id: Uuid,
}

/// A project role that differs from what the organization role gives (`role: null` goes back to it).
pub async fn set_project_role(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<UserProjectPath>, Json(b): Json<RoleBody>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let project: (Uuid,) = sqlx::query_as("SELECT org_id FROM projects WHERE id = $1").bind(p.project_id).fetch_optional(&st.pg).await?.ok_or(ApiError::NotFound("project"))?;
    let member: Option<(String,)> = sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2").bind(project.0).bind(p.user_id).fetch_optional(&st.pg).await?;
    if member.is_none() {
        return Err(ApiError::BadRequest("add the user to the project's organization first".into()));
    }
    match b.role.as_deref() {
        Some(r) if PROJECT_ROLES.contains(&r) => {
            sqlx::query("INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT (project_id, user_id) DO UPDATE SET role = $3")
                .bind(p.project_id).bind(p.user_id).bind(r).execute(&st.pg).await?;
        }
        Some(_) => return Err(ApiError::BadRequest(format!("project role must be one of {}", PROJECT_ROLES.join(", ")))),
        None => {
            sqlx::query("DELETE FROM project_members WHERE project_id = $1 AND user_id = $2").bind(p.project_id).bind(p.user_id).execute(&st.pg).await?;
        }
    }
    crate::audit::record(&st.pg, project.0, Some(p.project_id), Some((cu.user.id, &cu.user.email)), "user.project_role", "user", &p.user_id.to_string(), json!({ "to": b.role })).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct OrgParam {
    pub org_id: Uuid,
}

/// The user's role in the organization, its preset, the overrides and the resulting permissions.
pub async fn get_permissions(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<UserPath>, QueryParams(q): QueryParams<OrgParam>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let u = target(&st, p.user_id).await?;
    let (role,): (String,) = sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2").bind(q.org_id).bind(u.id).fetch_optional(&st.pg).await?.ok_or(ApiError::NotFound("membership"))?;
    let overrides = crate::db::orgs::overrides(&st.pg, q.org_id, u.id).await?;
    let effective = if u.is_master { PermSet::all() } else { PermSet::resolve(&role, &overrides) };
    Ok(Json(json!({ "role": role, "preset": PermSet::of(perms::preset(&role)).keys(), "overrides": overrides, "effective": effective.keys(), "is_master": u.is_master })))
}

#[derive(Deserialize)]
pub struct PutPermissions {
    pub org_id: Uuid,
    /// permission key → true (grant), false (remove) or null (back to the role's preset).
    pub overrides: std::collections::HashMap<String, Option<bool>>,
}

pub async fn put_permissions(State(st): State<AppState>, cu: CurrentUser, Path(p): Path<UserPath>, Json(b): Json<PutPermissions>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let u = target(&st, p.user_id).await?;
    let member: Option<(String,)> = sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2").bind(b.org_id).bind(u.id).fetch_optional(&st.pg).await?;
    let (role,) = member.ok_or(ApiError::NotFound("membership"))?;
    let preset = PermSet::of(perms::preset(&role));
    for (k, v) in &b.overrides {
        let perm: Perm = Perm::from_key(k).ok_or_else(|| ApiError::BadRequest(format!("unknown permission {k}")))?;
        match v {
            // An override equal to the preset is no override: store nothing.
            Some(allow) if *allow != preset.has(perm) => {
                sqlx::query("INSERT INTO member_permissions (org_id, user_id, permission, allow) VALUES ($1, $2, $3, $4) \
                             ON CONFLICT (org_id, user_id, permission) DO UPDATE SET allow = $4")
                    .bind(b.org_id).bind(u.id).bind(k).bind(allow).execute(&st.pg).await?;
            }
            _ => {
                sqlx::query("DELETE FROM member_permissions WHERE org_id = $1 AND user_id = $2 AND permission = $3").bind(b.org_id).bind(u.id).bind(k).execute(&st.pg).await?;
            }
        }
    }
    crate::audit::org(&st.pg, b.org_id, &cu, "user.permissions", "user", u.id, json!({ "overrides": b.overrides })).await;
    let overrides = crate::db::orgs::overrides(&st.pg, b.org_id, u.id).await?;
    Ok(Json(json!({ "overrides": overrides, "effective": PermSet::resolve(&role, &overrides).keys() })))
}

#[cfg(test)]
mod tests {
    use super::temporary_password;

    #[test]
    fn temporary_passwords_are_readable_and_distinct() {
        let a = temporary_password();
        assert_eq!(a.len(), 19);
        assert_eq!(a.matches('-').count(), 3);
        assert!(!a.chars().any(|c| "0O1lI".contains(c)));
        assert_ne!(a, temporary_password());
    }
}
