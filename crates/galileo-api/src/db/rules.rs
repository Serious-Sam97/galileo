use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RedactionRuleRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub rule: serde_json::Value,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

pub async fn list(pool: &PgPool, project_id: Uuid) -> sqlx::Result<Vec<RedactionRuleRow>> {
    sqlx::query_as("SELECT * FROM redaction_rules WHERE project_id = $1 ORDER BY created_at")
        .bind(project_id)
        .fetch_all(pool)
        .await
}

pub async fn create(pool: &PgPool, project_id: Uuid, rule: serde_json::Value, description: &str) -> sqlx::Result<RedactionRuleRow> {
    sqlx::query_as(
        "INSERT INTO redaction_rules (id, project_id, rule, description) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(rule)
    .bind(description)
    .fetch_one(pool)
    .await
}

pub async fn delete(pool: &PgPool, project_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM redaction_rules WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
