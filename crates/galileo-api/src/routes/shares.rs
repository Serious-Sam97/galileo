//! Share links: a query frozen to an absolute time range, opened read-only by slug.

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use galileo_query::{Query, TimeRange};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

fn slug() -> String {
    // 10 url-safe chars from a v7 uuid's entropy
    let u = Uuid::now_v7();
    let b = u.as_bytes();
    const A: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    (0..10).map(|i| A[(b[i] as usize) % A.len()] as char).collect()
}

#[derive(Deserialize)]
pub struct CreateShare {
    pub query: serde_json::Value,
    #[serde(default)]
    pub title: String,
    #[serde(default = "d_kind")]
    pub kind: String,
}
fn d_kind() -> String { "query".into() }

#[derive(Serialize, sqlx::FromRow)]
pub struct Share {
    pub slug: String,
    pub title: String,
    pub kind: String,
    pub query: serde_json::Value,
    pub frozen_start: Option<DateTime<Utc>>,
    pub frozen_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Freeze the current time range and store the query. Returns the slug.
pub async fn create(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<CreateShare>) -> ApiResult<Json<serde_json::Value>> {
    let mut q = Query::from_json(b.query).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let (start, end) = q.time_range.resolve(Utc::now());
    q.time_range = TimeRange::Absolute { start, end };
    let query_json = serde_json::to_value(&q).unwrap_or(serde_json::Value::Null);
    let slug = slug();
    sqlx::query("INSERT INTO query_shares (id, project_id, slug, title, kind, query, frozen_start, frozen_end, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
        .bind(Uuid::now_v7()).bind(pa.project.id).bind(&slug).bind(b.title.chars().take(200).collect::<String>()).bind(&b.kind).bind(&query_json).bind(start).bind(end).bind(pa.user.id)
        .execute(&st.pg).await?;
    Ok(Json(json!({ "slug": slug, "frozen_start": start, "frozen_end": end })))
}

/// Read a share by slug. Project access is enforced by ProjectAccess like any other read.
pub async fn get(State(st): State<AppState>, pa: ProjectAccess, Path((_, slug)): Path<(Uuid, String)>) -> ApiResult<Json<Share>> {
    let s: Option<Share> = sqlx::query_as("SELECT slug, title, kind, query, frozen_start, frozen_end, created_at FROM query_shares WHERE project_id = $1 AND slug = $2")
        .bind(pa.project.id).bind(&slug).fetch_optional(&st.pg).await?;
    s.map(Json).ok_or(ApiError::NotFound("share not found"))
}
