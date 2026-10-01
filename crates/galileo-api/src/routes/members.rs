//! Per-project roles on top of org roles.
//!
//! Effective role = project role when present, else the org role mapped (owner/admin → admin,
//! member → editor, viewer → viewer). Org owners/admins are admins on every project.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::perms::Perm;
use crate::state::AppState;

/// Map (org role, project role) → effective role.
pub fn effective_role(org_role: &str, project_role: Option<&str>) -> &'static str {
    match org_role {
        "owner" => "owner",
        "admin" => "admin",
        _ => match project_role {
            Some("admin") => "admin",
            Some("editor") => "editor",
            Some("viewer") => "viewer",
            _ => match org_role { "member" => "editor", _ => "viewer" },
        },
    }
}

pub async fn list(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(Uuid, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT u.id, u.email, u.name, m.role AS org_role, pm.role AS project_role FROM org_members m JOIN users u ON u.id = m.user_id \
         LEFT JOIN project_members pm ON pm.user_id = u.id AND pm.project_id = $2 WHERE m.org_id = $1 ORDER BY u.email").bind(pa.project.org_id).bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "members": rows.into_iter().map(|(id, email, name, org_role, project_role)| json!({ "user_id": id, "email": email, "name": name, "org_role": org_role, "project_role": project_role, "effective": effective_role(&org_role, project_role.as_deref()) })).collect::<Vec<_>>(), "my_role": pa.role, "my_permissions": pa.perms.keys() })))
}

#[derive(Deserialize)]
pub struct RoleBody { pub role: String }
#[derive(Deserialize)]
pub struct MemberPath { #[allow(dead_code)] pub project_id: Uuid, pub user_id: Uuid }

pub async fn set_role(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<MemberPath>, Json(b): Json<RoleBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require(Perm::ManageMembers)?;
    if !matches!(b.role.as_str(), "viewer" | "editor" | "admin") { return Err(ApiError::BadRequest("role must be viewer, editor or admin".into())); }
    let in_org: Option<(String,)> = sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2").bind(pa.project.org_id).bind(p.user_id).fetch_optional(&st.pg).await?;
    if in_org.is_none() { return Err(ApiError::BadRequest("user is not a member of the organization".into())); }
    sqlx::query("INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT (project_id, user_id) DO UPDATE SET role = $3").bind(pa.project.id).bind(p.user_id).bind(&b.role).execute(&st.pg).await?;
    audit::project(&st.pg, &pa, "project.member.role", "user", p.user_id, json!({ "role": b.role })).await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn clear_role(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<MemberPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require(Perm::ManageMembers)?;
    sqlx::query("DELETE FROM project_members WHERE project_id = $1 AND user_id = $2").bind(pa.project.id).bind(p.user_id).execute(&st.pg).await?;
    audit::project(&st.pg, &pa, "project.member.clear", "user", p.user_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::effective_role;
    #[test]
    fn role_table() {
        assert_eq!(effective_role("owner", Some("viewer")), "owner");
        assert_eq!(effective_role("admin", None), "admin");
        assert_eq!(effective_role("member", None), "editor");
        assert_eq!(effective_role("member", Some("viewer")), "viewer");
        assert_eq!(effective_role("viewer", None), "viewer");
        assert_eq!(effective_role("viewer", Some("editor")), "editor");
        assert_eq!(effective_role("viewer", Some("admin")), "admin");
    }
}
