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
use crate::perms::Perm;

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

/// What each scope unlocks: OTLP ingest, the LLM gateway, browser/mobile telemetry only, and
/// deploy markers from CI.
pub const SCOPES: [&str; 4] = ["ingest", "gateway", "rum", "deploy"];

fn validate_scopes(scopes: &[String]) -> Result<Vec<String>, ApiError> {
    if scopes.is_empty() {
        return Err(ApiError::BadRequest("a key needs at least one scope".into()));
    }
    if let Some(bad) = scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
        return Err(ApiError::BadRequest(format!("unknown scope '{bad}' (expected one of {})", SCOPES.join(", "))));
    }
    let mut out = scopes.to_vec();
    out.sort();
    out.dedup();
    Ok(out)
}

/// The raw key is returned exactly once, here.
pub async fn create(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<CreateKey>) -> ApiResult<Json<serde_json::Value>> {
    pa.require(Perm::ManageIngest)?;
    let scopes = validate_scopes(&b.scopes)?;
    let raw = auth::new_api_key();
    let prefix = raw[..12].to_string();
    let key = api_keys::create(&st.pg, pa.project.id, b.name.trim(), &auth::hash_api_key(&raw), &prefix, &scopes).await?;
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
    pa.require(Perm::ManageIngest)?;
    if !api_keys::revoke(&st.pg, pa.project.id, p.key_id).await? {
        return Err(ApiError::NotFound("api key"));
    }
    st.resolver.invalidate_all();
    audit::project(&st.pg, &pa, "api_key.revoke", "api_key", p.key_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_are_checked() {
        assert_eq!(validate_scopes(&["gateway".into(), "ingest".into(), "ingest".into()]).unwrap(), vec!["gateway", "ingest"]);
        assert!(validate_scopes(&[]).is_err());
        assert!(validate_scopes(&["ingst".into()]).is_err());
        assert!(validate_scopes(&["deploy".into()]).is_ok());
    }
}
