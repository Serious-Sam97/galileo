use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(skip)]
    pub key_hash: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub async fn list(pool: &PgPool, project_id: Uuid) -> sqlx::Result<Vec<ApiKey>> {
    sqlx::query_as("SELECT * FROM api_keys WHERE project_id = $1 ORDER BY created_at DESC")
        .bind(project_id)
        .fetch_all(pool)
        .await
}

pub async fn create(pool: &PgPool, project_id: Uuid, name: &str, key_hash: &str, key_prefix: &str, scopes: &[String]) -> sqlx::Result<ApiKey> {
    sqlx::query_as(
        "INSERT INTO api_keys (id, project_id, name, key_hash, key_prefix, scopes) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(key_hash)
    .bind(key_prefix)
    .bind(scopes)
    .fetch_one(pool)
    .await
}

pub async fn revoke(pool: &PgPool, project_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id = $1 AND project_id = $2 AND revoked_at IS NULL")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
