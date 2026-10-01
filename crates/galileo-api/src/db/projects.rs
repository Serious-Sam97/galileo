use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Project {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

pub async fn create(pool: &PgPool, org_id: Uuid, name: &str, slug: &str) -> sqlx::Result<Project> {
    sqlx::query_as("INSERT INTO projects (id, org_id, name, slug) VALUES ($1, $2, $3, $4) RETURNING *")
        .bind(Uuid::now_v7())
        .bind(org_id)
        .bind(name)
        .bind(slug)
        .fetch_one(pool)
        .await
}

/// The projects a user may open: those of their organizations, or every project for the Master.
pub async fn for_user(pool: &PgPool, user_id: Uuid, master: bool) -> sqlx::Result<Vec<Project>> {
    if master {
        return sqlx::query_as("SELECT * FROM projects ORDER BY created_at").fetch_all(pool).await;
    }
    sqlx::query_as(
        "SELECT p.* FROM projects p JOIN org_members m ON m.org_id = p.org_id \
         WHERE m.user_id = $1 ORDER BY p.created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// The project and the user's effective role in it. The Master is `owner` of every project.
pub async fn project_for_user(pool: &PgPool, project_id: Uuid, user_id: Uuid, master: bool) -> sqlx::Result<Option<(Project, String)>> {
    if master {
        let p: Option<Project> = sqlx::query_as("SELECT * FROM projects WHERE id = $1").bind(project_id).fetch_optional(pool).await?;
        return Ok(p.map(|p| (p, "owner".to_string())));
    }
    #[derive(FromRow)]
    struct Row {
        id: Uuid,
        org_id: Uuid,
        name: String,
        slug: String,
        created_at: DateTime<Utc>,
        role: String,
    }
    let r: Option<Row> = sqlx::query_as(
        "SELECT p.id, p.org_id, p.name, p.slug, p.created_at, \
                CASE m.role WHEN 'owner' THEN 'owner' WHEN 'admin' THEN 'admin' \
                     ELSE coalesce(pm.role, CASE m.role WHEN 'member' THEN 'editor' ELSE 'viewer' END) END AS role \
         FROM projects p JOIN org_members m ON m.org_id = p.org_id \
         LEFT JOIN project_members pm ON pm.project_id = p.id AND pm.user_id = m.user_id \
         WHERE p.id = $1 AND m.user_id = $2",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(r.map(|r| {
        (
            Project { id: r.id, org_id: r.org_id, name: r.name, slug: r.slug, created_at: r.created_at },
            r.role,
        )
    }))
}

pub async fn update(pool: &PgPool, id: Uuid, name: &str) -> sqlx::Result<Project> {
    sqlx::query_as("UPDATE projects SET name = $2 WHERE id = $1 RETURNING *")
        .bind(id)
        .bind(name)
        .fetch_one(pool)
        .await
}

pub async fn delete(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM projects WHERE id = $1").bind(id).execute(pool).await?;
    Ok(())
}
