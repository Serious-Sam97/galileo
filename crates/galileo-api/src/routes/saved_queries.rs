use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::ProjectAccess;
use crate::db::saved_queries as sq;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct QueryPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub query_id: Uuid,
}

#[derive(Deserialize)]
pub struct Body {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub query: serde_json::Value,
}

fn validate(q: &serde_json::Value) -> ApiResult<()> {
    galileo_query::Query::from_json(q.clone()).map(|_| ()).map_err(|e| ApiError::Query(e.to_string()))
}

pub async fn list(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "saved_queries": sq::list(&st.pg, pa.project.id).await? })))
}

pub async fn get(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<QueryPath>) -> ApiResult<Json<serde_json::Value>> {
    let q = sq::get(&st.pg, pa.project.id, p.query_id).await?.ok_or(ApiError::NotFound("saved query"))?;
    Ok(Json(json!({ "saved_query": q })))
}

pub async fn create(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<Body>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate(&b.query)?;
    let q = sq::create(&st.pg, pa.project.id, b.name.trim(), &b.description, b.query, pa.user.id).await?;
    Ok(Json(json!({ "saved_query": q })))
}

pub async fn update(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<QueryPath>, Json(b): Json<Body>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate(&b.query)?;
    let q = sq::update(&st.pg, pa.project.id, p.query_id, b.name.trim(), &b.description, b.query)
        .await?
        .ok_or(ApiError::NotFound("saved query"))?;
    Ok(Json(json!({ "saved_query": q })))
}

pub async fn delete(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<QueryPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if !sq::delete(&st.pg, pa.project.id, p.query_id).await? {
        return Err(ApiError::NotFound("saved query"));
    }
    Ok(Json(json!({ "ok": true })))
}
