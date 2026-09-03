//! Notification channels and digest settings.

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use galileo_alerts::notify::{self, Notification};
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, FromRow, serde::Serialize)]
pub struct Channel { pub id: Uuid, pub name: String, pub kind: String, pub config: serde_json::Value, pub enabled: bool, pub created_at: DateTime<Utc> }

fn public_config(mut c: Channel) -> Channel {
    // never echo secrets back
    if let Some(o) = c.config.as_object_mut() {
        if o.contains_key("bot_token") { o.insert("bot_token".into(), json!("••••••")); }
    }
    c
}

pub async fn list(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<Channel> = sqlx::query_as("SELECT id, name, kind, config, enabled, created_at FROM notification_channels WHERE project_id = $1 ORDER BY name").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "channels": rows.into_iter().map(public_config).collect::<Vec<_>>(), "email_enabled": !st.config.smtp.host.trim().is_empty() })))
}

#[derive(Deserialize)]
pub struct ChannelBody { pub name: String, pub kind: String, #[serde(default)] pub config: serde_json::Value, #[serde(default = "yes")] pub enabled: bool }
fn yes() -> bool { true }

fn validate(st: &AppState, b: &ChannelBody) -> ApiResult<()> {
    if b.name.trim().is_empty() { return Err(ApiError::BadRequest("name required".into())); }
    let url_ok = |k: &str| b.config.get(k).and_then(|v| v.as_str()).map(|u| u.starts_with("http://") || u.starts_with("https://")).unwrap_or(false);
    match b.kind.as_str() {
        "webhook" | "slack" | "discord" => if !url_ok("url") { return Err(ApiError::BadRequest("config.url must be an http(s) URL".into())); },
        "telegram" => if b.config.get("bot_token").and_then(|v| v.as_str()).map(|s| s.is_empty() || s == "••••••").unwrap_or(true) || b.config.get("chat_id").and_then(|v| v.as_str()).map(str::is_empty).unwrap_or(true) { return Err(ApiError::BadRequest("config.bot_token and config.chat_id are required".into())); },
        "email" => {
            if st.config.smtp.host.trim().is_empty() { return Err(ApiError::BadRequest("SMTP is not configured on this server ([smtp] in galileo.toml or GALILEO_SMTP_* env)".into())); }
            let to = b.config.get("to").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).filter(|s| s.contains('@')).count()).unwrap_or(0);
            if to == 0 { return Err(ApiError::BadRequest("config.to must list at least one e-mail address".into())); }
        }
        _ => return Err(ApiError::BadRequest("kind must be webhook, slack, discord, telegram or email".into())),
    }
    Ok(())
}

pub async fn create(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<ChannelBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?; validate(&st, &b)?;
    let row: Channel = sqlx::query_as("INSERT INTO notification_channels (id, project_id, name, kind, config, enabled) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, name, kind, config, enabled, created_at")
        .bind(Uuid::now_v7()).bind(pa.project.id).bind(b.name.trim()).bind(&b.kind).bind(&b.config).bind(b.enabled).fetch_one(&st.pg).await?;
    audit::project(&st.pg, &pa, "channel.create", "channel", row.id, json!({ "name": row.name, "kind": row.kind })).await;
    Ok(Json(json!({ "channel": public_config(row) })))
}

#[derive(Deserialize)] pub struct ChannelPath { #[allow(dead_code)] pub project_id: Uuid, pub channel_id: Uuid }

pub async fn update(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ChannelPath>, Json(b): Json<ChannelBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    // keep the stored telegram token when the client sends the mask back
    let mut config = b.config.clone();
    if b.kind == "telegram" && config.get("bot_token").and_then(|v| v.as_str()) == Some("••••••") {
        let prev: Option<(serde_json::Value,)> = sqlx::query_as("SELECT config FROM notification_channels WHERE id = $1 AND project_id = $2").bind(p.channel_id).bind(pa.project.id).fetch_optional(&st.pg).await?;
        if let Some((pc,)) = prev { if let Some(tok) = pc.get("bot_token") { config["bot_token"] = tok.clone(); } }
    }
    let b2 = ChannelBody { name: b.name.clone(), kind: b.kind.clone(), config: config.clone(), enabled: b.enabled };
    validate(&st, &b2)?;
    let row: Option<Channel> = sqlx::query_as("UPDATE notification_channels SET name = $3, kind = $4, config = $5, enabled = $6, updated_at = now() WHERE id = $1 AND project_id = $2 RETURNING id, name, kind, config, enabled, created_at")
        .bind(p.channel_id).bind(pa.project.id).bind(b.name.trim()).bind(&b.kind).bind(&config).bind(b.enabled).fetch_optional(&st.pg).await?;
    let row = row.ok_or(ApiError::NotFound("channel"))?;
    audit::project(&st.pg, &pa, "channel.update", "channel", row.id, json!({ "name": row.name, "kind": row.kind, "enabled": row.enabled })).await;
    Ok(Json(json!({ "channel": public_config(row) })))
}

pub async fn delete(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ChannelPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let r = sqlx::query("DELETE FROM notification_channels WHERE id = $1 AND project_id = $2").bind(p.channel_id).bind(pa.project.id).execute(&st.pg).await?;
    if r.rows_affected() == 0 { return Err(ApiError::NotFound("channel")); }
    audit::project(&st.pg, &pa, "channel.delete", "channel", p.channel_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

/// Send a test message through one channel.
pub async fn test(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<ChannelPath>) -> ApiResult<Json<serde_json::Value>> {
    let row: Option<(String, serde_json::Value)> = sqlx::query_as("SELECT kind, config FROM notification_channels WHERE id = $1 AND project_id = $2").bind(p.channel_id).bind(pa.project.id).fetch_optional(&st.pg).await?;
    let (kind, config) = row.ok_or(ApiError::NotFound("channel"))?;
    let target = notify::target_from_channel(&kind, &config).ok_or_else(|| ApiError::BadRequest("channel config incomplete".into()))?;
    let n = Notification { kind: "test", state: "ok".into(), name: "test".into(), project_id: pa.project.id, title: format!("Test from Galileo · {}", pa.project.name), message: format!("Sent by {} to check this {kind} channel.", pa.user.email), value: None, threshold: None, url: Some(format!("{}/p/{}/settings", st.config.public_url.trim_end_matches('/'), pa.project.id)), at: Utc::now() };
    notify::send_target(&st.alerts.http, &st.alerts.mailer, &target, &n).await.map_err(|e| ApiError::BadRequest(format!("send failed: {e}")))?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- digest settings

#[derive(FromRow, serde::Serialize)]
pub struct DigestSettings { pub digest_channels: serde_json::Value, pub digest_hour_utc: i32, pub digest_last_sent: Option<NaiveDate> }

pub async fn get_digest(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let s: Option<DigestSettings> = sqlx::query_as("SELECT digest_channels, digest_hour_utc, digest_last_sent FROM project_settings WHERE project_id = $1").bind(pa.project.id).fetch_optional(&st.pg).await?;
    Ok(Json(json!({ "digest": s.unwrap_or(DigestSettings { digest_channels: json!([]), digest_hour_utc: 8, digest_last_sent: None }) })))
}

#[derive(Deserialize)] pub struct DigestBody { pub digest_channels: serde_json::Value, #[serde(default = "d8")] pub digest_hour_utc: i32 }
fn d8() -> i32 { 8 }

pub async fn put_digest(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<DigestBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if !b.digest_channels.is_array() || !(0..=23).contains(&b.digest_hour_utc) { return Err(ApiError::BadRequest("digest_channels must be an array and digest_hour_utc 0..23".into())); }
    sqlx::query("INSERT INTO project_settings (project_id, digest_channels, digest_hour_utc) VALUES ($1, $2, $3) ON CONFLICT (project_id) DO UPDATE SET digest_channels = $2, digest_hour_utc = $3, updated_at = now()")
        .bind(pa.project.id).bind(&b.digest_channels).bind(b.digest_hour_utc).execute(&st.pg).await?;
    audit::project(&st.pg, &pa, "digest.update", "project", pa.project.id, json!({ "hour_utc": b.digest_hour_utc, "channels": b.digest_channels.as_array().map(|a| a.len()).unwrap_or(0) })).await;
    Ok(Json(json!({ "ok": true })))
}

/// Send yesterday's digest now (uses saved digest channels, or the body's).
pub async fn send_digest(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<serde_json::Value>) -> ApiResult<Json<serde_json::Value>> {
    let channels = if b.get("channels").map(|c| c.is_array()).unwrap_or(false) { b["channels"].clone() } else {
        let s: Option<(serde_json::Value,)> = sqlx::query_as("SELECT digest_channels FROM project_settings WHERE project_id = $1").bind(pa.project.id).fetch_optional(&st.pg).await?;
        s.map(|x| x.0).unwrap_or(json!([]))
    };
    if channels.as_array().map(|a| a.is_empty()).unwrap_or(true) { return Err(ApiError::BadRequest("no digest channels configured".into())); }
    let date = (Utc::now() - Duration::days(1)).date_naive();
    let sent = galileo_alerts::digest::send(&st.alerts, pa.project.id, &channels, date).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(json!({ "sent": sent, "date": date })))
}

/// Preview the digest text without sending.
pub async fn preview_digest(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let date = (Utc::now() - Duration::days(1)).date_naive();
    let d = galileo_alerts::digest::build(&st.alerts, pa.project.id, date).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(json!({ "date": date, "text": galileo_alerts::digest::render_text(&d) })))
}
