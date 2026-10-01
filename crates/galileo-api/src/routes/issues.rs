//! Issues (grouped exceptions), issue notification settings, and deploy markers.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::{DateTime, Utc};
use galileo_alerts::issues::{self, IssueRow};
use galileo_core::ProjectId;
use galileo_query::TimeRange;
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ListParams {
    #[serde(default = "d_status")]
    pub status: String,
    #[serde(default = "d_sort")]
    pub sort: String,
    #[serde(default = "d_last")]
    pub last_seconds: i64,
    #[serde(default)]
    pub q: Option<String>,
}
fn d_status() -> String { "open".into() }
fn d_sort() -> String { "last_seen".into() }
fn d_last() -> i64 { 86400 }

pub async fn list(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<ListParams>) -> ApiResult<Json<serde_json::Value>> {
    let order = match p.sort.as_str() { "count" => "count DESC", "first_seen" => "first_seen DESC", "users" => "users DESC", _ => "last_seen DESC" };
    let status_filter = if p.status == "all" { String::new() } else { " AND status = $2".into() };
    let sql = format!("SELECT * FROM issues WHERE project_id = $1{status_filter} AND ($3 = '' OR title ILIKE '%' || $3 || '%' OR culprit ILIKE '%' || $3 || '%' OR route ILIKE '%' || $3 || '%') ORDER BY {order} LIMIT 200");
    let rows: Vec<IssueRow> = sqlx::query_as(&sql).bind(pa.project.id).bind(&p.status).bind(p.q.clone().unwrap_or_default()).fetch_all(&st.pg).await?;
    // Window counts + sparkline (12 buckets) from ClickHouse, one query for all listed issues.
    let (start, end) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let bucket = (p.last_seconds / 12).max(60);
    let fps: Vec<String> = rows.iter().map(|r| r.fingerprint.clone()).collect();
    let mut window: std::collections::HashMap<String, (i64, Vec<i64>)> = std::collections::HashMap::new();
    if !fps.is_empty() {
        let res = st.storage.query(&galileo_storage::SqlQuery {
            sql: format!("SELECT exception_fingerprint, count(), groupArray((toUnixTimestamp(toStartOfInterval(timestamp, INTERVAL {bucket} SECOND)), 1)) \
                          FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND exception_fingerprint IN (?) GROUP BY exception_fingerprint"),
            params: vec![ProjectId(pa.project.id).into(), start.into(), end.into(), galileo_storage::SqlValue::StrList(fps)],
        }).await?;
        let first = (start.timestamp() / bucket) * bucket;
        for r in &res.rows {
            let fp = r[0].as_str().unwrap_or("").to_string();
            let total = r[1].as_i64().or_else(|| r[1].as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
            let mut spark = vec![0i64; 12];
            if let Some(arr) = r[2].as_array() {
                for pair in arr {
                    if let Some(ts) = pair.as_array().and_then(|p| p.first()).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))) {
                        let idx = ((ts - first) / bucket) as usize;
                        if idx < 12 { spark[idx] += 1; }
                    }
                }
            }
            window.insert(fp, (total, spark));
        }
    }
    let counts: Vec<(String, i64)> = sqlx::query_as("SELECT status, count(*) FROM issues WHERE project_id = $1 GROUP BY status").bind(pa.project.id).fetch_all(&st.pg).await?;
    let items: Vec<serde_json::Value> = rows.into_iter().map(|r| {
        let (wc, spark) = window.get(&r.fingerprint).cloned().unwrap_or((0, vec![0; 12]));
        let mut v = serde_json::to_value(&r).unwrap_or(json!({}));
        v["window_count"] = json!(wc);
        v["sparkline"] = json!(spark);
        v
    }).collect();
    let counts: serde_json::Map<String, serde_json::Value> = counts.into_iter().map(|(k, v)| (k, json!(v))).collect();
    Ok(Json(json!({ "issues": items, "counts": counts, "start": start, "end": end })))
}

#[derive(Deserialize)]
pub struct IssuePath { #[allow(dead_code)] pub project_id: Uuid, pub issue_id: Uuid }

async fn load(st: &AppState, project: Uuid, id: Uuid) -> ApiResult<IssueRow> {
    sqlx::query_as("SELECT * FROM issues WHERE id = $1 AND project_id = $2").bind(id).bind(project).fetch_optional(&st.pg).await?.ok_or(ApiError::NotFound("issue"))
}

#[derive(Debug, FromRow, serde::Serialize)]
pub struct IssueEvent { pub id: Uuid, pub kind: String, pub at: DateTime<Utc>, pub message: String, pub user_id: Option<Uuid> }

pub async fn get(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<IssuePath>, QueryParams(r): QueryParams<ListParams>) -> ApiResult<Json<serde_json::Value>> {
    let issue = load(&st, pa.project.id, p.issue_id).await?;
    let events: Vec<IssueEvent> = sqlx::query_as("SELECT id, kind, at, message, user_id FROM issue_events WHERE issue_id = $1 ORDER BY at DESC LIMIT 50").bind(issue.id).fetch_all(&st.pg).await?;
    let (start, end) = TimeRange::Relative { last_seconds: r.last_seconds }.resolve(Utc::now());
    let pid = ProjectId(pa.project.id);
    let base = "FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND exception_fingerprint = ?";
    let params = || vec![pid.into(), start.into(), end.into(), issue.fingerprint.clone().into()];
    let gran = (r.last_seconds / 48).max(60);
    let series = st.storage.query(&galileo_storage::SqlQuery { sql: format!("SELECT toUnixTimestamp(toStartOfInterval(timestamp, INTERVAL {gran} SECOND)) AS ts, count() AS n {base} GROUP BY ts ORDER BY ts"), params: params() }).await?;
    let by = |col: &str| galileo_storage::SqlQuery { sql: format!("SELECT {col} AS key, count() AS n {base} AND {col} != '' GROUP BY key ORDER BY n DESC LIMIT 8"), params: params() };
    let routes = st.storage.query(&by("http_route")).await?;
    let users = st.storage.query(&by("user_id")).await?;
    let tenants = st.storage.query(&by("tenant_id")).await?;
    let versions = st.storage.query(&by("service_version")).await?;
    let samples = st.storage.query(&galileo_storage::SqlQuery {
        sql: format!("SELECT timestamp, trace_id, http_route, user_id, tenant_id, duration_ns / 1e6 AS duration_ms, exception_message, service_version {base} ORDER BY timestamp DESC LIMIT 10"), params: params() }).await?;
    let latest = st.storage.query(&galileo_storage::SqlQuery {
        sql: "SELECT arrayFirst(m -> mapContains(m, 'exception.stacktrace'), events_attrs)['exception.stacktrace'] AS stack, exception_message, timestamp, trace_id \
              FROM spans WHERE project_id = ? AND exception_fingerprint = ? AND timestamp >= fromUnixTimestamp64Nano(?) ORDER BY timestamp DESC LIMIT 1".into(),
        params: vec![pid.into(), issue.fingerprint.clone().into(), (end - chrono::Duration::days(30)).into()],
    }).await?;
    let latest_obj = latest.to_objects().into_iter().next();
    Ok(Json(json!({
        "issue": issue, "events": events, "start": start, "end": end, "granularity": gran,
        "series": series.to_objects(), "routes": routes.to_objects(), "users": users.to_objects(), "tenants": tenants.to_objects(), "versions": versions.to_objects(),
        "samples": samples.to_objects(), "latest": latest_obj,
    })))
}

#[derive(Deserialize)]
pub struct StatusBody { #[serde(default)] pub note: String, #[serde(default)] pub version: String }

async fn set_status(st: &AppState, pa: &ProjectAccess, id: Uuid, status: &str, kind: &str, b: &StatusBody) -> ApiResult<IssueRow> {
    pa.require_write()?;
    let issue = load(st, pa.project.id, id).await?;
    let row: IssueRow = sqlx::query_as(
        "UPDATE issues SET status = $2, resolved_at = CASE WHEN $2 = 'resolved' THEN now() ELSE NULL END, resolved_version = CASE WHEN $2 = 'resolved' THEN $3 ELSE '' END, \
         resolved_by = CASE WHEN $2 = 'resolved' THEN $4 ELSE NULL END, notes = CASE WHEN $5 = '' THEN notes ELSE $5 END, updated_at = now() WHERE id = $1 RETURNING *",
    ).bind(issue.id).bind(status).bind(&b.version).bind(pa.user.id).bind(&b.note).fetch_one(&st.pg).await?;
    issues::add_event(&st.pg, issue.id, kind, if b.note.is_empty() { "" } else { &b.note }, Some(pa.user.id)).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    audit::project(&st.pg, pa, &format!("issue.{kind}"), "issue", issue.id, json!({ "title": issue.title, "version": b.version })).await;
    Ok(row)
}
pub async fn resolve(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<IssuePath>, Json(b): Json<StatusBody>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "issue": set_status(&st, &pa, p.issue_id, "resolved", "resolved", &b).await? })))
}
pub async fn ignore(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<IssuePath>, Json(b): Json<StatusBody>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "issue": set_status(&st, &pa, p.issue_id, "ignored", "ignored", &b).await? })))
}
pub async fn reopen(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<IssuePath>, Json(b): Json<StatusBody>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "issue": set_status(&st, &pa, p.issue_id, "open", "reopened", &b).await? })))
}

#[derive(Deserialize)]
pub struct SettingsBody { pub issue_recipients: serde_json::Value }

pub async fn get_settings(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let r: Option<(serde_json::Value, Option<DateTime<Utc>>)> = sqlx::query_as("SELECT issue_recipients, issues_last_run FROM project_settings WHERE project_id = $1").bind(pa.project.id).fetch_optional(&st.pg).await?;
    let (rec, last) = r.unwrap_or((json!([]), None));
    Ok(Json(json!({ "issue_recipients": rec, "issues_last_run": last })))
}

pub async fn put_settings(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<SettingsBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_admin()?;
    if !b.issue_recipients.is_array() { return Err(ApiError::BadRequest("issue_recipients must be an array".into())); }
    sqlx::query("INSERT INTO project_settings (project_id, issue_recipients) VALUES ($1, $2) ON CONFLICT (project_id) DO UPDATE SET issue_recipients = $2, updated_at = now()")
        .bind(pa.project.id).bind(&b.issue_recipients).execute(&st.pg).await?;
    audit::project(&st.pg, &pa, "issue_settings.update", "project", pa.project.id, json!({ "recipients": b.issue_recipients.as_array().map(|a| a.len()).unwrap_or(0) })).await;
    Ok(Json(json!({ "issue_recipients": b.issue_recipients })))
}

#[derive(Debug, FromRow, serde::Serialize)]
pub struct Deploy { pub id: Uuid, pub service: String, pub version: String, pub at: DateTime<Utc>, pub note: String, pub url: String }

#[derive(Deserialize)]
pub struct DeployBody { #[serde(default)] pub service: String, pub version: String, #[serde(default)] pub at: Option<DateTime<Utc>>, #[serde(default)] pub note: String, #[serde(default)] pub url: String }

pub async fn list_deploys(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<ListParams>) -> ApiResult<Json<serde_json::Value>> {
    let (start, _) = TimeRange::Relative { last_seconds: p.last_seconds }.resolve(Utc::now());
    let rows: Vec<Deploy> = sqlx::query_as("SELECT id, service, version, at, note, url FROM deploys WHERE project_id = $1 AND at >= $2 ORDER BY at DESC LIMIT 200").bind(pa.project.id).bind(start).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "deploys": rows })))
}

/// Who may record a deploy: a signed-in member with write access, or — for CI — a project API
/// key with the `deploy` scope, so pipelines do not need a personal token.
pub enum DeployActor {
    User(ProjectAccess),
    Key(Uuid),
}

impl axum::extract::FromRequestParts<AppState> for DeployActor {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, st: &AppState) -> Result<Self, Self::Rejection> {
        let bearer = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(|v| v.trim().to_owned());
        let Some(raw) = bearer.filter(|k| k.starts_with("glk_")) else {
            let pa = ProjectAccess::from_request_parts(parts, st).await?;
            pa.require_write()?;
            return Ok(DeployActor::User(pa));
        };
        let Path(DeployPath { project_id }) = Path::<DeployPath>::from_request_parts(parts, st).await.map_err(|_| ApiError::BadRequest("invalid project id".into()))?;
        use galileo_otlp::ApiKeyResolver;
        let ctx = st.resolver.resolve(&raw).await.ok_or(ApiError::Unauthorized)?;
        if ctx.project_id.0 != project_id { return Err(ApiError::NotFound("project")); }
        if !ctx.has_scope("deploy") { return Err(ApiError::Forbidden); }
        Ok(DeployActor::Key(project_id))
    }
}

#[derive(Deserialize)]
pub struct DeployPath { project_id: Uuid }

/// Called from CI: `POST /api/projects/{id}/deploys {"version": "1.4.3"}` with a session, a
/// personal token (`glt_`) or a project key with the `deploy` scope (`glk_`).
pub async fn create_deploy(State(st): State<AppState>, actor: DeployActor, Json(b): Json<DeployBody>) -> ApiResult<Json<serde_json::Value>> {
    let project_id = match &actor { DeployActor::User(pa) => pa.project.id, DeployActor::Key(p) => *p };
    if b.version.trim().is_empty() { return Err(ApiError::BadRequest("version required".into())); }
    let row: Deploy = sqlx::query_as("INSERT INTO deploys (id, project_id, service, version, at, note, url) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, service, version, at, note, url")
        .bind(Uuid::now_v7()).bind(project_id).bind(&b.service).bind(b.version.trim()).bind(b.at.unwrap_or_else(Utc::now)).bind(&b.note).bind(&b.url).fetch_one(&st.pg).await?;
    Ok(Json(json!({ "deploy": row })))
}
