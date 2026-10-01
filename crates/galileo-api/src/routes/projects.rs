use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{CurrentUser, ProjectAccess};
use crate::db::{self, orgs, projects};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::perms::Perm;

#[derive(Deserialize)]
pub struct CreateOrg {
    pub name: String,
}

pub async fn list_orgs(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "orgs": orgs::for_user(&st.pg, cu.user.id, cu.user.is_master).await? })))
}

/// Organizations are created by the Master, who becomes their first owner.
pub async fn create_org(State(st): State<AppState>, cu: CurrentUser, Json(b): Json<CreateOrg>) -> ApiResult<Json<serde_json::Value>> {
    cu.require_master()?;
    let name = b.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name required".into()));
    }
    let org = orgs::create_with_owner(&st.pg, name, &db::slugify(name), cu.user.id).await?;
    Ok(Json(json!({ "org": org })))
}

#[derive(Deserialize)]
pub struct CreateProject {
    pub org_id: Uuid,
    pub name: String,
}

pub async fn list(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "projects": projects::for_user(&st.pg, cu.user.id, cu.user.is_master).await? })))
}

pub async fn create(State(st): State<AppState>, cu: CurrentUser, Json(b): Json<CreateProject>) -> ApiResult<Json<serde_json::Value>> {
    let (_, perms) = crate::auth::org_access(&st, b.org_id, &cu.user).await?;
    if !perms.has(Perm::ManageOrg) {
        return Err(ApiError::Forbidden);
    }
    let name = b.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name required".into()));
    }
    let p = projects::create(&st.pg, b.org_id, name, &db::slugify(name)).await?;
    Ok(Json(json!({ "project": p })))
}

pub async fn get(pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "project": pa.project, "role": pa.role, "permissions": pa.perms.keys(), "is_master": pa.user.is_master })))
}

#[derive(Deserialize)]
pub struct UpdateProject {
    pub name: String,
}

pub async fn update(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<UpdateProject>) -> ApiResult<Json<serde_json::Value>> {
    pa.require(Perm::ManageProject)?;
    let p = projects::update(&st.pg, pa.project.id, b.name.trim()).await?;
    Ok(Json(json!({ "project": p })))
}

pub async fn delete(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    pa.require(Perm::ManageOrg)?;
    projects::delete(&st.pg, pa.project.id).await?;
    st.resolver.invalidate_all();
    Ok(Json(json!({ "ok": true })))
}
