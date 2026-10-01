use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{self, CurrentUser};
use crate::db::{self, orgs, projects, users, users::User};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Failed password attempts before the account is locked, and for how long.
const MAX_FAILED_LOGINS: i32 = 5;
const LOCK_MINUTES: i64 = 15;

#[derive(Deserialize)]
pub struct RegisterBody {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub name: String,
    /// Organization to create for this user. Defaults to their name.
    #[serde(default)]
    pub org_name: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

/// Whether the instance still needs its first user. The UI shows "create the first account" vs
/// "sign in"; once anyone exists, accounts come from the Master only.
pub async fn setup_status(State(st): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let n = users::count(&st.pg).await?;
    Ok(Json(json!({ "needs_setup": n == 0, "registration_open": n == 0, "users": n })))
}

/// Creates the first account of the instance, which becomes its Master. Closed afterwards.
pub async fn register(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<RegisterBody>) -> ApiResult<impl IntoResponse> {
    if users::count(&st.pg).await? > 0 {
        return Err(ApiError::Coded(StatusCode::FORBIDDEN, "registration_closed", "accounts are created by the Master".into()));
    }
    let email = b.email.trim().to_ascii_lowercase();
    if !email.contains('@') {
        return Err(ApiError::BadRequest("invalid email".into()));
    }
    check_new_password(&b.password, &email)?;
    let name = if b.name.trim().is_empty() { email.split('@').next().unwrap_or("me").to_string() } else { b.name.trim().to_string() };
    let user = users::create(&st.pg, &email, &name, &auth::hash_password(&b.password)?).await?;
    sqlx::query("UPDATE users SET is_master = true WHERE id = $1").bind(user.id).execute(&st.pg).await?;

    // The first user gets an org and a first project so the UI is never empty.
    let org_name = b.org_name.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| format!("{name}'s org"));
    let mut slug = db::slugify(&org_name);
    if sqlx::query_scalar::<_, i64>("SELECT count(*) FROM organizations WHERE slug = $1")
        .bind(&slug)
        .fetch_one(&st.pg)
        .await?
        > 0
    {
        slug = format!("{slug}-{}", &user.id.simple().to_string()[..6]);
    }
    let org = orgs::create_with_owner(&st.pg, &org_name, &slug, user.id).await?;
    let project = projects::create(&st.pg, org.id, "Default", "default").await?;

    let user = users::by_id(&st.pg, user.id).await?.ok_or(ApiError::NotFound("user"))?;
    let (cookie, token) = open_session(&st, &user, &headers).await?;
    Ok((auth::set_cookie_header(cookie), Json(json!({ "user": user, "org": org, "project": project, "token": token }))))
}

/// Email + password → the user, applying every sign-in rule: lockout after repeated failures,
/// disabled accounts, expired temporary passwords. Shared by the password login and its 2FA step.
pub(crate) async fn check_credentials(st: &AppState, email: &str, password: &str) -> ApiResult<User> {
    let email = email.trim().to_ascii_lowercase();
    let Some(user) = users::by_email(&st.pg, &email).await? else {
        // Same work and answer as a wrong password, so the response does not reveal which e-mails exist.
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let dummy = DUMMY.get_or_init(|| auth::hash_password("galileo-timing-equaliser").unwrap_or_default());
        let _ = auth::verify_password(password, dummy);
        return Err(ApiError::Unauthorized);
    };
    if let Some(until) = user.locked_until.filter(|t| *t > Utc::now()) {
        let mins = (until - Utc::now()).num_minutes() + 1;
        return Err(ApiError::Coded(StatusCode::TOO_MANY_REQUESTS, "locked", format!("too many failed attempts: try again in {mins} min")));
    }
    if !auth::verify_password(password, &user.password_hash) {
        let failed = user.failed_logins + 1;
        let lock: Option<DateTime<Utc>> = (failed >= MAX_FAILED_LOGINS).then(|| Utc::now() + Duration::minutes(LOCK_MINUTES));
        sqlx::query("UPDATE users SET failed_logins = $2, locked_until = $3 WHERE id = $1")
            .bind(user.id)
            .bind(if lock.is_some() { 0 } else { failed })
            .bind(lock)
            .execute(&st.pg)
            .await?;
        return Err(ApiError::Unauthorized);
    }
    if user.disabled_at.is_some() {
        return Err(ApiError::Coded(StatusCode::FORBIDDEN, "disabled", "this account is disabled".into()));
    }
    if user.must_change_password && user.temp_password_expires_at.is_some_and(|t| t <= Utc::now()) {
        return Err(ApiError::Coded(StatusCode::UNAUTHORIZED, "temporary_password_expired", "this temporary password expired: ask the Master for a new one".into()));
    }
    if user.failed_logins > 0 {
        sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1").bind(user.id).execute(&st.pg).await?;
    }
    Ok(user)
}

/// A new session for a user who passed every check: the cookie to set and the raw token.
pub(crate) async fn open_session(st: &AppState, user: &User, headers: &HeaderMap) -> ApiResult<(HeaderValue, String)> {
    let token = auth::new_token();
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
    users::create_session(&st.pg, user.id, &auth::hash_token(&token), auth::session_expiry(), ua).await?;
    sqlx::query("UPDATE users SET last_login_at = now() WHERE id = $1").bind(user.id).execute(&st.pg).await?;
    Ok((auth::session_cookie(&token, auth::secure_cookies(&st.config)), token))
}

pub async fn login(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<LoginBody>) -> ApiResult<impl IntoResponse> {
    let user = check_credentials(&st, &b.email, &b.password).await?;
    let (totp,): (bool,) = sqlx::query_as("SELECT totp_enabled FROM users WHERE id = $1").bind(user.id).fetch_one(&st.pg).await?;
    if totp {
        return Ok((auth::set_cookie_header(HeaderValue::from_static("galileo_noop=1; Max-Age=0; Path=/")), Json(json!({ "needs_2fa": true }))));
    }
    let (cookie, token) = open_session(&st, &user, &headers).await?;
    Ok((auth::set_cookie_header(cookie), Json(json!({ "user": user, "token": token, "must_change_password": user.must_change_password }))))
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == auth::SESSION_COOKIE)
        .map(|(_, v)| v.to_owned())
        .or_else(|| headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).filter(|t| !t.starts_with("glt_")).map(str::to_owned))
}

pub async fn logout(State(st): State<AppState>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    if let Some(tok) = session_token(&headers) {
        users::delete_session(&st.pg, &auth::hash_token(&tok)).await?;
    }
    Ok((auth::set_cookie_header(auth::clear_cookie()), Json(json!({ "ok": true }))))
}

pub async fn me(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    let orgs = orgs::for_user(&st.pg, cu.user.id, cu.user.is_master).await?;
    let projects = projects::for_user(&st.pg, cu.user.id, cu.user.is_master).await?;
    Ok(Json(json!({ "user": cu.user, "orgs": orgs, "projects": projects })))
}

/// The rule every chosen password follows (temporary ones are generated, not chosen).
pub(crate) fn check_new_password(pw: &str, email: &str) -> ApiResult<()> {
    let len = pw.chars().count();
    if len < 10 {
        return Err(ApiError::BadRequest("the password needs at least 10 characters".into()));
    }
    if len > 200 {
        return Err(ApiError::BadRequest("the password is too long".into()));
    }
    let local = email.split('@').next().unwrap_or("").to_lowercase();
    if local.len() >= 3 && pw.to_lowercase().contains(&local) {
        return Err(ApiError::BadRequest("the password must not contain your e-mail name".into()));
    }
    if pw.chars().all(|c| Some(c) == pw.chars().next()) {
        return Err(ApiError::BadRequest("the password must not repeat one character".into()));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct PasswordBody {
    /// Required, unless the user signed in with a temporary password and must change it.
    #[serde(default)]
    pub current_password: Option<String>,
    pub new_password: String,
}

/// Choose a new password. Signs out every other session.
pub async fn change_password(State(st): State<AppState>, headers: HeaderMap, cu: CurrentUser, Json(b): Json<PasswordBody>) -> ApiResult<Json<serde_json::Value>> {
    let user = &cu.user;
    if !user.must_change_password {
        let current = b.current_password.as_deref().unwrap_or("");
        if !auth::verify_password(current, &user.password_hash) {
            return Err(ApiError::Coded(StatusCode::BAD_REQUEST, "wrong_password", "the current password is not right".into()));
        }
    }
    check_new_password(&b.new_password, &user.email)?;
    if auth::verify_password(&b.new_password, &user.password_hash) {
        return Err(ApiError::BadRequest("choose a password different from the current one".into()));
    }
    users::set_password(&st.pg, user.id, &auth::hash_password(&b.new_password)?).await?;
    let keep = session_token(&headers).map(|t| auth::hash_token(&t));
    let signed_out = users::delete_sessions(&st.pg, user.id, keep.as_deref()).await?;
    for o in orgs::for_user(&st.pg, user.id, false).await? {
        crate::audit::org(&st.pg, o.id, &cu, "user.password_change", "user", user.id, json!({ "was_temporary": user.must_change_password })).await;
    }
    Ok(Json(json!({ "ok": true, "signed_out_sessions": signed_out })))
}

/// The user's active sessions; `current` marks the one making this request.
pub async fn sessions(State(st): State<AppState>, headers: HeaderMap, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    let current = session_token(&headers).map(|t| auth::hash_token(&t));
    let rows: Vec<(String, DateTime<Utc>, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT token_hash, created_at, expires_at, user_agent FROM sessions WHERE user_id = $1 AND expires_at > now() ORDER BY created_at DESC",
    )
    .bind(cu.user.id)
    .fetch_all(&st.pg)
    .await?;
    // A short prefix of the token's hash names a session; neither the token nor its hash leaves.
    let list: Vec<_> = rows
        .into_iter()
        .map(|(h, created, expires, ua)| json!({ "id": &h[..16], "created_at": created, "expires_at": expires, "user_agent": ua, "current": current.as_deref() == Some(h.as_str()) }))
        .collect();
    Ok(Json(json!({ "sessions": list })))
}

/// Sign out one session (by id) or, with `all`, every session but this one.
pub async fn revoke_session(State(st): State<AppState>, headers: HeaderMap, cu: CurrentUser, Path(id): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    let n = if id == "others" {
        let keep = session_token(&headers).map(|t| auth::hash_token(&t));
        users::delete_sessions(&st.pg, cu.user.id, keep.as_deref()).await?
    } else {
        if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ApiError::BadRequest("invalid session id".into()));
        }
        sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND starts_with(token_hash, $2)").bind(cu.user.id).bind(&id).execute(&st.pg).await?.rows_affected()
    };
    Ok(Json(json!({ "signed_out": n })))
}

#[cfg(test)]
mod tests {
    use super::check_new_password;

    #[test]
    fn password_rule() {
        assert!(check_new_password("short", "ana@x.io").is_err());
        assert!(check_new_password("aaaaaaaaaaaa", "ana@x.io").is_err());
        assert!(check_new_password("my-anabela-pass", "anabela@x.io").is_err(), "contains the e-mail name");
        assert!(check_new_password("correct horse battery", "ana@x.io").is_ok());
        assert!(check_new_password("ana-is-fine-here", "an@x.io").is_ok(), "names under 3 letters are not checked");
    }
}
