use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::auth::{self, CurrentUser};
use crate::db::{self, orgs, projects, users};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

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

/// Whether the instance still needs its first user. The UI shows "create admin" vs "log in".
pub async fn setup_status(State(st): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let n = users::count(&st.pg).await?;
    Ok(Json(json!({ "needs_setup": n == 0, "users": n })))
}

pub async fn register(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<RegisterBody>) -> ApiResult<impl IntoResponse> {
    let email = b.email.trim().to_ascii_lowercase();
    if !email.contains('@') {
        return Err(ApiError::BadRequest("invalid email".into()));
    }
    if b.password.len() < 8 {
        return Err(ApiError::BadRequest("password must be at least 8 characters".into()));
    }
    if users::by_email(&st.pg, &email).await?.is_some() {
        return Err(ApiError::Conflict("email already registered".into()));
    }
    let name = if b.name.trim().is_empty() { email.split('@').next().unwrap_or("me").to_string() } else { b.name.trim().to_string() };
    let user = users::create(&st.pg, &email, &name, &auth::hash_password(&b.password)?).await?;

    // Every new user gets an org and a first project so the UI is never empty.
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

    let token = auth::new_token();
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
    users::create_session(&st.pg, user.id, &auth::hash_token(&token), auth::session_expiry(), ua).await?;
    Ok((
        auth::set_cookie_header(auth::session_cookie(&token, false)),
        Json(json!({ "user": user, "org": org, "project": project, "token": token })),
    ))
}

pub async fn login(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<LoginBody>) -> ApiResult<impl IntoResponse> {
    let email = b.email.trim().to_ascii_lowercase();
    let user = users::by_email(&st.pg, &email).await?.ok_or(ApiError::Unauthorized)?;
    if !auth::verify_password(&b.password, &user.password_hash) {
        return Err(ApiError::Unauthorized);
    }
    let (totp,): (bool,) = sqlx::query_as("SELECT totp_enabled FROM users WHERE id = $1").bind(user.id).fetch_one(&st.pg).await?;
    if totp {
        return Ok((auth::set_cookie_header(HeaderValue::from_static("galileo_noop=1; Max-Age=0; Path=/")), Json(json!({ "needs_2fa": true }))));
    }
    let token = auth::new_token();
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
    users::create_session(&st.pg, user.id, &auth::hash_token(&token), auth::session_expiry(), ua).await?;
    Ok((auth::set_cookie_header(auth::session_cookie(&token, false)), Json(json!({ "user": user, "token": token }))))
}

pub async fn logout(State(st): State<AppState>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    if let Some(tok) = headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == auth::SESSION_COOKIE)
        .map(|(_, v)| v.to_owned())
    {
        users::delete_session(&st.pg, &auth::hash_token(&tok)).await?;
    }
    Ok((auth::set_cookie_header(auth::clear_cookie()), Json(json!({ "ok": true }))))
}

pub async fn me(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<serde_json::Value>> {
    let orgs = orgs::for_user(&st.pg, cu.user.id).await?;
    let projects = projects::for_user(&st.pg, cu.user.id).await?;
    Ok(Json(json!({ "user": cu.user, "orgs": orgs, "projects": projects })))
}
