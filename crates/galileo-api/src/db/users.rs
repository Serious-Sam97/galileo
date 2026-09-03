use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    #[serde(skip)]
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

pub async fn by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE email = $1").bind(email).fetch_optional(pool).await
}

pub async fn count(pool: &PgPool) -> sqlx::Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users").fetch_one(pool).await?;
    Ok(n)
}

pub async fn create(pool: &PgPool, email: &str, name: &str, password_hash: &str) -> sqlx::Result<User> {
    sqlx::query_as(
        "INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(email)
    .bind(name)
    .bind(password_hash)
    .fetch_one(pool)
    .await
}

pub async fn create_session(pool: &PgPool, user_id: Uuid, token_hash: &str, expires_at: DateTime<Utc>, ua: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at, user_agent) VALUES ($1, $2, $3, $4)")
        .bind(token_hash)
        .bind(user_id)
        .bind(expires_at)
        .bind(ua)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_session(pool: &PgPool, token_hash: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1").bind(token_hash).execute(pool).await?;
    Ok(())
}

pub async fn user_for_session(pool: &PgPool, token_hash: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as(
        "SELECT u.* FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.token_hash = $1 AND s.expires_at > now()",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

/// Personal API token (`glt_…`) → user; bumps last_used_at.
pub async fn user_for_api_token(pool: &PgPool, token_hash: &str) -> sqlx::Result<Option<User>> {
    let u: Option<User> = sqlx::query_as(
        "SELECT u.* FROM api_tokens t JOIN users u ON u.id = t.user_id \
         WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at > now())",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    if u.is_some() {
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE token_hash = $1").bind(token_hash).execute(pool).await;
    }
    Ok(u)
}
