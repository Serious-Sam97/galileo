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

/// The user's organizations with their role; the Master sees every organization (as owner where
/// not a member).
pub async fn for_user(pool: &PgPool, user_id: Uuid, master: bool) -> sqlx::Result<Vec<OrgWithRole>> {
    if master {
        return sqlx::query_as(
            "SELECT o.id, o.name, o.slug, o.created_at, coalesce(m.role, 'owner') AS role FROM organizations o \
             LEFT JOIN org_members m ON m.org_id = o.id AND m.user_id = $1 ORDER BY o.created_at",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await;
    }
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

/// Permission overrides of one user in one organization: permission key → granted (true) or removed (false).
pub async fn overrides(pool: &PgPool, org_id: Uuid, user_id: Uuid) -> sqlx::Result<std::collections::HashMap<String, bool>> {
    let rows: Vec<(String, bool)> = sqlx::query_as("SELECT permission, allow FROM member_permissions WHERE org_id = $1 AND user_id = $2")
        .bind(org_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().collect())
}
