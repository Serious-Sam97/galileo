use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::auth::{self, ProjectAccess};
use crate::db::api_keys;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub async fn list(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "api_keys": api_keys::list(&st.pg, pa.project.id).await? })))
}

#[derive(Deserialize)]
pub struct CreateKey {
    pub name: String,
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
}
fn default_scopes() -> Vec<String> {
    vec!["ingest".into(), "gateway".into()]
}

/// The raw key is returned exactly once, here.
pub async fn create(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<CreateKey>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let raw = auth::new_api_key();
    let prefix = raw[..12].to_string();
    let key = api_keys::create(&st.pg, pa.project.id, b.name.trim(), &auth::hash_api_key(&raw), &prefix, &b.scopes).await?;
    audit::project(&st.pg, &pa, "api_key.create", "api_key", key.id, json!({ "name": key.name, "scopes": key.scopes })).await;
    Ok(Json(json!({ "api_key": key, "key": raw })))
}

#[derive(Deserialize)]
pub struct KeyPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub key_id: Uuid,
}

pub async fn revoke(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<KeyPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if !api_keys::revoke(&st.pg, pa.project.id, p.key_id).await? {
        return Err(ApiError::NotFound("api key"));
    }
    st.resolver.invalidate_all();
    audit::project(&st.pg, &pa, "api_key.revoke", "api_key", p.key_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}
