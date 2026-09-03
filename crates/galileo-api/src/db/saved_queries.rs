use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SavedQuery {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub query: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn list(pool: &PgPool, project_id: Uuid) -> sqlx::Result<Vec<SavedQuery>> {
    sqlx::query_as("SELECT * FROM saved_queries WHERE project_id = $1 ORDER BY updated_at DESC")
        .bind(project_id)
        .fetch_all(pool)
        .await
}

pub async fn get(pool: &PgPool, project_id: Uuid, id: Uuid) -> sqlx::Result<Option<SavedQuery>> {
    sqlx::query_as("SELECT * FROM saved_queries WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
}

pub async fn create(pool: &PgPool, project_id: Uuid, name: &str, description: &str, query: serde_json::Value, by: Uuid) -> sqlx::Result<SavedQuery> {
    sqlx::query_as(
        "INSERT INTO saved_queries (id, project_id, name, description, query, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(description)
    .bind(query)
    .bind(by)
    .fetch_one(pool)
    .await
}

pub async fn update(pool: &PgPool, project_id: Uuid, id: Uuid, name: &str, description: &str, query: serde_json::Value) -> sqlx::Result<Option<SavedQuery>> {
    sqlx::query_as(
        "UPDATE saved_queries SET name = $3, description = $4, query = $5, updated_at = now() \
         WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(description)
    .bind(query)
    .fetch_optional(pool)
    .await
}

pub async fn delete(pool: &PgPool, project_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM saved_queries WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
