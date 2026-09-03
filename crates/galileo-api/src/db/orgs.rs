use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct OrgWithRole {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
    pub role: String,
}

pub async fn create_with_owner(pool: &PgPool, name: &str, slug: &str, owner: Uuid) -> sqlx::Result<Organization> {
    let mut tx = pool.begin().await?;
    let org: Organization =
        sqlx::query_as("INSERT INTO organizations (id, name, slug) VALUES ($1, $2, $3) RETURNING *")
            .bind(Uuid::now_v7())
            .bind(name)
            .bind(slug)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("INSERT INTO org_members (org_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(org.id)
        .bind(owner)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(org)
}

pub async fn for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<OrgWithRole>> {
    sqlx::query_as(
        "SELECT o.id, o.name, o.slug, o.created_at, m.role FROM organizations o \
         JOIN org_members m ON m.org_id = o.id WHERE m.user_id = $1 ORDER BY o.created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn role_for(pool: &PgPool, org_id: Uuid, user_id: Uuid) -> sqlx::Result<Option<String>> {
    let r: Option<(String,)> = sqlx::query_as("SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2")
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(r.map(|(r,)| r))
}
