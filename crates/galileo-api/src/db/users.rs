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
    /// Above every organization: sees and manages all users, orgs and projects.
    pub is_master: bool,
    /// Signed in with a temporary password: nothing but choosing a new one is allowed.
    pub must_change_password: bool,
    #[serde(skip)]
    pub temp_password_expires_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub password_changed_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub disabled_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub last_login_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub failed_logins: i32,
    #[serde(skip)]
    pub locked_until: Option<DateTime<Utc>>,
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
         WHERE s.token_hash = $1 AND s.expires_at > now() AND u.disabled_at IS NULL",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

/// Personal API token (`glt_…`) → user; bumps last_used_at.
pub async fn user_for_api_token(pool: &PgPool, token_hash: &str) -> sqlx::Result<Option<User>> {
    let u: Option<User> = sqlx::query_as(
        "SELECT u.* FROM api_tokens t JOIN users u ON u.id = t.user_id \
         WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at > now()) \
           AND u.disabled_at IS NULL AND NOT u.must_change_password",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    if u.is_some() {
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE token_hash = $1").bind(token_hash).execute(pool).await;
    }
    Ok(u)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE id = $1").bind(id).fetch_optional(pool).await
}

/// Signs the user out everywhere, except the session whose hash is `keep`.
pub async fn delete_sessions(pool: &PgPool, user_id: Uuid, keep: Option<&str>) -> sqlx::Result<u64> {
    Ok(sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND ($2::text IS NULL OR token_hash <> $2)")
        .bind(user_id)
        .bind(keep)
        .execute(pool)
        .await?
        .rows_affected())
}

/// A new password the user chose: clears the temporary state and any lockout.
pub async fn set_password(pool: &PgPool, user_id: Uuid, hash: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET password_hash = $2, must_change_password = false, temp_password_expires_at = NULL, \
                 password_changed_at = now(), failed_logins = 0, locked_until = NULL WHERE id = $1")
        .bind(user_id)
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// A temporary password set by the Master: must be changed at the next sign-in and expires unused.
pub async fn set_temporary_password(pool: &PgPool, user_id: Uuid, hash: &str, expires_at: DateTime<Utc>) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET password_hash = $2, must_change_password = true, temp_password_expires_at = $3, \
                 failed_logins = 0, locked_until = NULL WHERE id = $1")
        .bind(user_id)
        .bind(hash)
        .bind(expires_at)
        .execute(pool)
        .await?;
    Ok(())
}
