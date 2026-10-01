//! OIDC single sign-on (Authorization Code + PKCE) and TOTP two-factor for password logins.

use axum::extract::{Query as QueryParams, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::auth::{self, CurrentUser};
use crate::db::users::User;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

fn oidc(st: &AppState) -> Option<&galileo_core::config::OidcConfig> { st.config.auth.oidc.as_ref().filter(|c| !c.issuer.is_empty() && !c.client_id.is_empty()) }

pub async fn oidc_config(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "enabled": oidc(&st).is_some(), "label": oidc(&st).map(|c| c.label.clone()).unwrap_or_default() }))
}

async fn discovery(st: &AppState, issuer: &str) -> ApiResult<Value> {
    let url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
    let v: Value = reqwest::Client::new().get(&url).send().await.map_err(|e| ApiError::BadRequest(format!("oidc discovery: {e}")))?.json().await.map_err(|e| ApiError::BadRequest(format!("oidc discovery: {e}")))?;
    let _ = st;
    Ok(v)
}

fn sign_state(secret: &[u8], payload: &str) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(secret).expect("hmac");
    m.update(payload.as_bytes());
    format!("{}.{}", B64.encode(payload), B64.encode(m.finalize().into_bytes()))
}
fn verify_state(secret: &[u8], token: &str) -> Option<String> {
    let (p, sig) = token.split_once('.')?;
    let payload = String::from_utf8(B64.decode(p).ok()?).ok()?;
    let mut m = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    m.update(payload.as_bytes());
    let expected = B64.encode(m.finalize().into_bytes());
    (expected == sig).then_some(payload)
}

/// Redirect the browser to the provider. State/nonce/PKCE verifier travel in a signed cookie.
pub async fn oidc_start(State(st): State<AppState>) -> ApiResult<Response> {
    let cfg = oidc(&st).ok_or_else(|| ApiError::BadRequest("SSO is not configured".into()))?;
    let disc = discovery(&st, &cfg.issuer).await?;
    let auth_ep = disc.get("authorization_endpoint").and_then(|v| v.as_str()).ok_or_else(|| ApiError::BadRequest("provider has no authorization_endpoint".into()))?;
    let state = auth::new_token(); let nonce = auth::new_token(); let verifier = auth::new_token();
    let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
    let payload = json!({ "state": state, "nonce": nonce, "verifier": verifier, "exp": chrono::Utc::now().timestamp() + 600 }).to_string();
    let cookie = format!("galileo_oidc={}; Path=/api/auth/oidc; HttpOnly; SameSite=Lax; Max-Age=600{}", sign_state(&st.secret, &payload), if auth::secure_cookies(&st.config) { "; Secure" } else { "" });
    let redirect = format!("{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        auth_ep, urlencode(&cfg.client_id), urlencode(&cfg.redirect_url), urlencode("openid email profile"), state, nonce, challenge);
    let mut resp = Redirect::temporary(&redirect).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).map_err(|e| ApiError::Internal(e.to_string()))?);
    Ok(resp)
}

fn urlencode(s: &str) -> String { s.bytes().map(|b| match b { b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(), _ => format!("%{b:02X}") }).collect() }

#[derive(Deserialize)]
pub struct CallbackParams { #[serde(default)] pub code: String, #[serde(default)] pub state: String, #[serde(default)] pub error: Option<String> }

/// Exchange the code, verify the ID token, create/attach the user, start a session.
pub async fn oidc_callback(State(st): State<AppState>, headers: HeaderMap, QueryParams(p): QueryParams<CallbackParams>) -> ApiResult<Response> {
    let cfg = oidc(&st).ok_or_else(|| ApiError::BadRequest("SSO is not configured".into()))?;
    if let Some(e) = p.error { return Err(ApiError::BadRequest(format!("provider error: {e}"))); }
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).and_then(|c| c.split(';').map(str::trim).find_map(|kv| kv.strip_prefix("galileo_oidc="))).ok_or_else(|| ApiError::BadRequest("missing SSO state cookie".into()))?;
    let payload: Value = serde_json::from_str(&verify_state(&st.secret, cookie).ok_or_else(|| ApiError::BadRequest("bad SSO state".into()))?).map_err(|_| ApiError::BadRequest("bad SSO state".into()))?;
    if payload["state"].as_str() != Some(p.state.as_str()) || payload["exp"].as_i64().unwrap_or(0) < chrono::Utc::now().timestamp() { return Err(ApiError::BadRequest("SSO state mismatch or expired".into())); }
    let disc = discovery(&st, &cfg.issuer).await?;
    let token_ep = disc.get("token_endpoint").and_then(|v| v.as_str()).ok_or_else(|| ApiError::BadRequest("provider has no token_endpoint".into()))?;
    let http = reqwest::Client::new();
    let tok: Value = http.post(token_ep).form(&[("grant_type", "authorization_code"), ("code", p.code.as_str()), ("redirect_uri", cfg.redirect_url.as_str()), ("client_id", cfg.client_id.as_str()), ("client_secret", cfg.client_secret.as_str()), ("code_verifier", payload["verifier"].as_str().unwrap_or(""))]).send().await.map_err(|e| ApiError::BadRequest(format!("token exchange: {e}")))?.json().await.map_err(|e| ApiError::BadRequest(format!("token exchange: {e}")))?;
    let id_token = tok.get("id_token").and_then(|v| v.as_str()).ok_or_else(|| ApiError::BadRequest(format!("no id_token in token response: {}", tok)))?;
    // verify: RS256 via JWKS, or HS256 with the client secret
    let header_ = jsonwebtoken::decode_header(id_token).map_err(|e| ApiError::BadRequest(format!("id_token header: {e}")))?;
    let mut validation = jsonwebtoken::Validation::new(header_.alg);
    validation.set_audience(&[cfg.client_id.as_str()]);
    validation.set_issuer(&[cfg.issuer.as_str(), cfg.issuer.trim_end_matches('/')]);
    let key = match header_.alg {
        jsonwebtoken::Algorithm::HS256 | jsonwebtoken::Algorithm::HS384 | jsonwebtoken::Algorithm::HS512 => jsonwebtoken::DecodingKey::from_secret(cfg.client_secret.as_bytes()),
        _ => {
            let jwks_url = disc.get("jwks_uri").and_then(|v| v.as_str()).ok_or_else(|| ApiError::BadRequest("provider has no jwks_uri".into()))?;
            let jwks: Value = http.get(jwks_url).send().await.map_err(|e| ApiError::BadRequest(format!("jwks: {e}")))?.json().await.map_err(|e| ApiError::BadRequest(format!("jwks: {e}")))?;
            let jwk = jwks.get("keys").and_then(|k| k.as_array()).and_then(|ks| ks.iter().find(|k| header_.kid.as_deref().map(|kid| k.get("kid").and_then(|x| x.as_str()) == Some(kid)).unwrap_or(true))).ok_or_else(|| ApiError::BadRequest("no matching JWK".into()))?;
            let (n, e) = (jwk.get("n").and_then(|x| x.as_str()).unwrap_or(""), jwk.get("e").and_then(|x| x.as_str()).unwrap_or(""));
            jsonwebtoken::DecodingKey::from_rsa_components(n, e).map_err(|e| ApiError::BadRequest(format!("jwk: {e}")))?
        }
    };
    let claims = jsonwebtoken::decode::<Value>(id_token, &key, &validation).map_err(|e| ApiError::BadRequest(format!("id_token invalid: {e}")))?.claims;
    if claims.get("nonce").and_then(|n| n.as_str()) != payload["nonce"].as_str() { return Err(ApiError::BadRequest("nonce mismatch".into())); }
    let sub = claims.get("sub").and_then(|v| v.as_str()).ok_or_else(|| ApiError::BadRequest("id_token without sub".into()))?.to_string();
    let email = claims.get("email").and_then(|v| v.as_str()).map(|e| e.trim().to_ascii_lowercase()).ok_or_else(|| ApiError::BadRequest("id_token without email (add the email scope)".into()))?;
    let name = claims.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if !cfg.allowed_domains.is_empty() && !cfg.allowed_domains.iter().any(|d| email.ends_with(&format!("@{d}"))) { return Err(ApiError::Forbidden); }
    // Accounts are created by the Master: SSO signs in an existing, enabled account (matched by its
    // SSO identity or e-mail) and never creates one.
    let _ = &name;
    let existing: Option<User> = sqlx::query_as("SELECT * FROM users WHERE (oidc_issuer = $1 AND oidc_subject = $2) OR email = $3 ORDER BY (oidc_subject = $2) DESC NULLS LAST LIMIT 1")
        .bind(&cfg.issuer).bind(&sub).bind(&email).fetch_optional(&st.pg).await?;
    let Some(user) = existing.filter(|u| u.disabled_at.is_none()) else {
        let to = format!("{}/login?sso=no_account", st.config.public_url.trim_end_matches('/').replace(":8080", ":3000"));
        let mut resp = Redirect::temporary(&to).into_response();
        resp.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_static("galileo_oidc=; Path=/api/auth/oidc; Max-Age=0"));
        return Ok(resp);
    };
    sqlx::query("UPDATE users SET oidc_issuer = $2, oidc_subject = $3 WHERE id = $1").bind(user.id).bind(&cfg.issuer).bind(&sub).execute(&st.pg).await?;
    let (cookie, _token) = super::auth::open_session(&st, &user, &headers).await?;
    let mut resp = Redirect::temporary(&format!("{}/", st.config.public_url.trim_end_matches('/').replace(":8080", ":3000"))).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, cookie);
    resp.headers_mut().append(header::SET_COOKIE, HeaderValue::from_static("galileo_oidc=; Path=/api/auth/oidc; Max-Age=0"));
    Ok(resp)
}

// ---------------------------------------------------------------- TOTP

fn base32_encode(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new(); let mut buf: u32 = 0; let mut bits = 0;
    for b in bytes { buf = (buf << 8) | *b as u32; bits += 8; while bits >= 5 { out.push(A[((buf >> (bits - 5)) & 31) as usize] as char); bits -= 5; } }
    if bits > 0 { out.push(A[((buf << (5 - bits)) & 31) as usize] as char); }
    out
}
fn base32_decode(s: &str) -> Vec<u8> {
    const A: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = vec![]; let mut buf: u32 = 0; let mut bits = 0;
    for c in s.chars().filter(|c| *c != '=') { let Some(v) = A.find(c.to_ascii_uppercase()) else { continue }; buf = (buf << 5) | v as u32; bits += 5; if bits >= 8 { out.push(((buf >> (bits - 8)) & 0xff) as u8); bits -= 8; } }
    out
}
pub fn totp(secret: &[u8], step: u64) -> u32 {
    let mut m = Hmac::<Sha1>::new_from_slice(secret).expect("hmac");
    m.update(&step.to_be_bytes());
    let h = m.finalize().into_bytes();
    let off = (h[19] & 0x0f) as usize;
    let code = ((h[off] as u32 & 0x7f) << 24) | ((h[off + 1] as u32) << 16) | ((h[off + 2] as u32) << 8) | h[off + 3] as u32;
    code % 1_000_000
}
pub fn totp_ok(secret_b32: &str, code: &str) -> bool {
    let Ok(c) = code.trim().parse::<u32>() else { return false };
    let secret = base32_decode(secret_b32);
    let now = chrono::Utc::now().timestamp() as u64 / 30;
    (now.saturating_sub(1)..=now + 1).any(|s| totp(&secret, s) == c)
}

pub async fn twofa_setup(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<Value>> {
    let secret = base32_encode(auth::new_token().as_bytes()).chars().take(32).collect::<String>();
    sqlx::query("UPDATE users SET totp_secret = $2, totp_enabled = false WHERE id = $1").bind(cu.user.id).bind(&secret).execute(&st.pg).await?;
    Ok(Json(json!({ "secret": secret, "otpauth_url": format!("otpauth://totp/Galileo:{}?secret={}&issuer=Galileo&digits=6&period=30", cu.user.email, secret) })))
}
#[derive(Deserialize)]
pub struct CodeBody { pub code: String, #[serde(default)] pub email: String, #[serde(default)] pub password: String }
pub async fn twofa_enable(State(st): State<AppState>, cu: CurrentUser, Json(b): Json<CodeBody>) -> ApiResult<Json<Value>> {
    let (secret,): (String,) = sqlx::query_as("SELECT totp_secret FROM users WHERE id = $1").bind(cu.user.id).fetch_one(&st.pg).await?;
    if secret.is_empty() || !totp_ok(&secret, &b.code) { return Err(ApiError::BadRequest("invalid code".into())); }
    sqlx::query("UPDATE users SET totp_enabled = true WHERE id = $1").bind(cu.user.id).execute(&st.pg).await?;
    Ok(Json(json!({ "enabled": true })))
}
pub async fn twofa_disable(State(st): State<AppState>, cu: CurrentUser, Json(b): Json<CodeBody>) -> ApiResult<Json<Value>> {
    let (secret,): (String,) = sqlx::query_as("SELECT totp_secret FROM users WHERE id = $1").bind(cu.user.id).fetch_one(&st.pg).await?;
    if !totp_ok(&secret, &b.code) { return Err(ApiError::BadRequest("invalid code".into())); }
    sqlx::query("UPDATE users SET totp_enabled = false, totp_secret = '' WHERE id = $1").bind(cu.user.id).execute(&st.pg).await?;
    Ok(Json(json!({ "enabled": false })))
}
pub async fn twofa_status(State(st): State<AppState>, cu: CurrentUser) -> ApiResult<Json<Value>> {
    let (enabled,): (bool,) = sqlx::query_as("SELECT totp_enabled FROM users WHERE id = $1").bind(cu.user.id).fetch_one(&st.pg).await?;
    Ok(Json(json!({ "enabled": enabled })))
}
/// Second step of a password login: email + password + code → session.
pub async fn twofa_verify(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<CodeBody>) -> ApiResult<Response> {
    let user = super::auth::check_credentials(&st, &b.email, &b.password).await?;
    let (secret, enabled): (String, bool) = sqlx::query_as("SELECT totp_secret, totp_enabled FROM users WHERE id = $1").bind(user.id).fetch_one(&st.pg).await?;
    if enabled && !totp_ok(&secret, &b.code) { return Err(ApiError::Unauthorized); }
    let (cookie, token) = super::auth::open_session(&st, &user, &headers).await?;
    Ok((StatusCode::OK, auth::set_cookie_header(cookie), Json(json!({ "user": user, "token": token, "must_change_password": user.must_change_password }))).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn totp_rfc6238_vector() {
        // RFC 6238 test secret "12345678901234567890" (SHA1), T=59 → step 1 → 287082
        assert_eq!(totp(b"12345678901234567890", 59 / 30), 287082);
        let b32 = base32_encode(b"12345678901234567890");
        assert_eq!(base32_decode(&b32), b"12345678901234567890".to_vec());
    }
    #[test]
    fn signed_state_round_trip() {
        let secret = [7u8; 32];
        let t = sign_state(&secret, "{\"state\":\"x\"}");
        assert_eq!(verify_state(&secret, &t).as_deref(), Some("{\"state\":\"x\"}"));
        assert!(verify_state(&[8u8; 32], &t).is_none());
    }
}
