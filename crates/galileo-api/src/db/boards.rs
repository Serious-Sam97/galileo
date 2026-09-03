use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Board {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub panels: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(default)] pub variables: Option<serde_json::Value>,
    #[sqlx(default)] pub time_range: Option<serde_json::Value>,
    #[sqlx(default)] pub compare: Option<bool>,
    #[sqlx(default)] pub template: Option<String>,
}

pub async fn list(pool: &PgPool, project_id: Uuid) -> sqlx::Result<Vec<Board>> {
    sqlx::query_as("SELECT * FROM boards WHERE project_id = $1 ORDER BY updated_at DESC")
        .bind(project_id)
        .fetch_all(pool)
        .await
}

pub async fn get(pool: &PgPool, project_id: Uuid, id: Uuid) -> sqlx::Result<Option<Board>> {
    sqlx::query_as("SELECT * FROM boards WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
}

pub async fn create(pool: &PgPool, project_id: Uuid, name: &str, description: &str, panels: serde_json::Value) -> sqlx::Result<Board> {
    sqlx::query_as(
        "INSERT INTO boards (id, project_id, name, description, panels) VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(description)
    .bind(panels)
    .fetch_one(pool)
    .await
}

pub async fn update(pool: &PgPool, project_id: Uuid, id: Uuid, name: &str, description: &str, panels: serde_json::Value) -> sqlx::Result<Option<Board>> {
    sqlx::query_as(
        "UPDATE boards SET name = $3, description = $4, panels = $5, updated_at = now() \
         WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(description)
    .bind(panels)
    .fetch_optional(pool)
    .await
}

pub async fn delete(pool: &PgPool, project_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM boards WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
