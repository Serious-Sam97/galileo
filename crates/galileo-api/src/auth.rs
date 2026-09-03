//! Password hashing, session tokens and the request extractors that turn a cookie into a
//! `CurrentUser` and a path `project_id` into an authorised `ProjectAccess`.

use argon2::password_hash::{rand_core::OsRng, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Argon2, PasswordHash};
use axum::extract::{FromRequestParts, Path};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{request::Parts, HeaderValue};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::{projects::Project, users::User};
use crate::error::ApiError;
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "galileo_session";
const SESSION_DAYS: i64 = 30;

pub fn hash_password(pw: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Internal(format!("hash: {e}")))
}

pub fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

/// Random 32-byte token, URL-safe base64. Only its hash is stored.
pub fn new_token() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b)
}

pub fn hash_token(t: &str) -> String {
    hex::encode(Sha256::digest(t.as_bytes()))
}

/// Same hashing for ingest/gateway API keys.
pub fn hash_api_key(k: &str) -> String {
    hash_token(k)
}

/// API keys look like `glk_<40 url-safe chars>` so they are recognisable in configs and logs.
pub fn new_api_key() -> String {
    let mut b = [0u8; 30];
    rand::rng().fill_bytes(&mut b);
    format!("glk_{}", base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b))
}

pub fn session_expiry() -> DateTime<Utc> {
    Utc::now() + Duration::days(SESSION_DAYS)
}

/// `Secure` cookies as soon as the public URL is https; plain http (localhost) keeps them off,
/// otherwise the browser would drop the session.
pub fn secure_cookies(config: &galileo_core::config::Config) -> bool {
    config.public_url.starts_with("https://")
}

pub fn session_cookie(token: &str, secure: bool) -> HeaderValue {
    let mut v = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        SESSION_DAYS * 86400
    );
    if secure {
        v.push_str("; Secure");
    }
    HeaderValue::from_str(&v).expect("valid cookie")
}

pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("galileo_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub fn set_cookie_header(v: HeaderValue) -> [(axum::http::HeaderName, HeaderValue); 1] {
    [(SET_COOKIE, v)]
}

fn cookie_value(parts: &Parts, name: &str) -> Option<String> {
    parts.headers.get_all(COOKIE).iter().find_map(|h| {
        h.to_str().ok()?.split(';').find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == name).then(|| v.to_owned())
        })
    })
}

/// The logged-in user. Rejects with 401 when there is no valid session.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user: User,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = cookie_value(parts, SESSION_COOKIE)
            .or_else(|| {
                parts
                    .headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("Bearer "))
                    .map(str::to_owned)
            })
            .ok_or(ApiError::Unauthorized)?;
        let user = if token.starts_with("glt_") {
            crate::db::users::user_for_api_token(&state.pg, &hash_token(&token)).await?
        } else {
            crate::db::users::user_for_session(&state.pg, &hash_token(&token)).await?
        }
        .ok_or(ApiError::Unauthorized)?;
        Ok(CurrentUser { user })
    }
}

/// A project the current user may access, resolved from the `{project_id}` path segment.
#[derive(Debug, Clone)]
pub struct ProjectAccess {
    pub user: User,
    pub project: Project,
    pub role: String,
}

impl ProjectAccess {
    pub fn can_write(&self) -> bool {
        matches!(self.role.as_str(), "owner" | "admin" | "member" | "editor")
    }
    pub fn is_admin(&self) -> bool {
        matches!(self.role.as_str(), "owner" | "admin")
    }
    /// Settings, deletes and membership changes.
    pub fn require_admin(&self) -> Result<(), ApiError> {
        if self.is_admin() { Ok(()) } else { Err(ApiError::Forbidden) }
    }
    pub fn require_write(&self) -> Result<(), ApiError> {
        if self.can_write() {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }
}

#[derive(serde::Deserialize)]
struct ProjectPath {
    project_id: Uuid,
}

impl FromRequestParts<AppState> for ProjectAccess {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let CurrentUser { user } = CurrentUser::from_request_parts(parts, state).await?;
        let Path(ProjectPath { project_id }) = Path::<ProjectPath>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::BadRequest("invalid project id".into()))?;
        let (project, role) = crate::db::projects::project_for_user(&state.pg, project_id, user.id)
            .await?
            .ok_or(ApiError::NotFound("project"))?;
        Ok(ProjectAccess { user, project, role })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("hunter2").unwrap();
        assert!(verify_password("hunter2", &h));
        assert!(!verify_password("hunter3", &h));
    }

    #[test]
    fn tokens_are_unique_and_prefixed() {
        assert_ne!(new_token(), new_token());
        assert!(new_api_key().starts_with("glk_"));
        assert_eq!(hash_token("a").len(), 64);
    }
}
