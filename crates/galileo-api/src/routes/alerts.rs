//! Triggers and SLOs: CRUD, history, and on-demand evaluation.

use axum::extract::{Path, Query as QueryParams, State};
use axum::Json;
use chrono::{DateTime, Utc};
use galileo_alerts::slos::SloRow;
use galileo_alerts::triggers::TriggerRow;
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// ---------------------------------------------------------------- triggers

fn d_sens() -> i32 { 3 }

#[derive(Deserialize)]
pub struct TriggerBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub query: serde_json::Value,
    pub op: String,
    #[serde(default)]
    pub threshold: f64,
    #[serde(default = "d_freq")]
    pub frequency_secs: i32,
    #[serde(default = "d_window")]
    pub window_secs: i32,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "empty_arr")]
    pub recipients: serde_json::Value,
    #[serde(default)]
    pub warn_threshold: Option<f64>,
    #[serde(default)]
    pub for_secs: i32,
    #[serde(default)]
    pub mute_hours: Option<f64>,
    #[serde(default)]
    pub per_group: bool,
    #[serde(default = "d_mode")]
    pub mode: String,
    #[serde(default = "d_factor")]
    pub baseline_factor: f64,
    #[serde(default)]
    pub baseline_min_delta: f64,
    #[serde(default = "d_sens")]
    pub sensitivity: i32,
    #[serde(default)]
    pub min_value: f64,
    #[serde(default)]
    pub composite: Option<serde_json::Value>,
}
fn d_mode() -> String { "threshold".into() }
fn d_factor() -> f64 { 2.0 }
fn d_freq() -> i32 {
    60
}
fn d_window() -> i32 {
    300
}
fn yes() -> bool {
    true
}
fn empty_arr() -> serde_json::Value {
    json!([])
}

fn validate_trigger(b: &TriggerBody) -> ApiResult<()> {
    if b.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name required".into()));
    }
    if !matches!(b.op.as_str(), ">" | ">=" | "<" | "<=" | "=" | "!=") {
        return Err(ApiError::BadRequest("op must be one of > >= < <= = !=".into()));
    }
    let q = galileo_query::Query::from_json(b.query.clone()).map_err(|e| ApiError::Query(e.to_string()))?;
    if q.calculations.is_empty() {
        return Err(ApiError::BadRequest("trigger query needs exactly one calculation".into()));
    }
    if b.frequency_secs < 30 || b.window_secs < 30 {
        return Err(ApiError::BadRequest("frequency and window must be at least 30 seconds".into()));
    }
    if !b.recipients.is_array() {
        return Err(ApiError::BadRequest("recipients must be an array".into()));
    }
    for r in b.recipients.as_array().unwrap() {
        let ty = r.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let url = r.get("url").and_then(|u| u.as_str()).unwrap_or("");
        let ok = match ty { "channel" | "oncall" => r.get("id").and_then(|i| i.as_str()).map(|i| Uuid::parse_str(i).is_ok()).unwrap_or(false), "webhook" | "slack" => url.starts_with("http://") || url.starts_with("https://"), _ => false };
        if !ok { return Err(ApiError::BadRequest("each recipient is {type: channel|oncall, id} or {type: webhook|slack, url}".into())); }
    }
    if !matches!(b.mode.as_str(), "threshold" | "baseline" | "anomaly" | "outlier") { return Err(ApiError::BadRequest("mode must be threshold, baseline, anomaly or outlier".into())); }
    if b.for_secs < 0 || b.baseline_factor <= 0.0 { return Err(ApiError::BadRequest("for_secs must be >= 0 and baseline_factor > 0".into())); }
    Ok(())
}

pub async fn list_triggers(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<TriggerRow> = sqlx::query_as("SELECT * FROM triggers WHERE project_id = $1 ORDER BY name").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "triggers": rows })))
}

pub async fn create_trigger(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<TriggerBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate_trigger(&b)?;
    let mute_until = b.mute_hours.filter(|h| *h > 0.0).map(|h| Utc::now() + chrono::Duration::seconds((h * 3600.0) as i64));
    let row: TriggerRow = sqlx::query_as(
        "INSERT INTO triggers (id, project_id, name, description, query, op, threshold, frequency_secs, window_secs, enabled, recipients, warn_threshold, for_secs, mute_until, per_group, mode, baseline_factor, baseline_min_delta, sensitivity, min_value, composite) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21) RETURNING *",
    )
    .bind(Uuid::now_v7()).bind(pa.project.id).bind(b.name.trim()).bind(&b.description).bind(&b.query).bind(&b.op).bind(b.threshold)
    .bind(b.frequency_secs).bind(b.window_secs).bind(b.enabled).bind(&b.recipients).bind(b.warn_threshold).bind(b.for_secs).bind(mute_until).bind(b.per_group).bind(&b.mode).bind(b.baseline_factor).bind(b.baseline_min_delta).bind(b.sensitivity.clamp(1, 5)).bind(b.min_value).bind(&b.composite)
    .fetch_one(&st.pg)
    .await?;
    audit::project(&st.pg, &pa, "trigger.create", "trigger", row.id, json!({ "name": row.name, "op": row.op, "threshold": row.threshold })).await;
    Ok(Json(json!({ "trigger": row })))
}

#[derive(Deserialize)]
pub struct TriggerPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub trigger_id: Uuid,
}

async fn load_trigger(st: &AppState, project: Uuid, id: Uuid) -> ApiResult<TriggerRow> {
    sqlx::query_as("SELECT * FROM triggers WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project)
        .fetch_optional(&st.pg)
        .await?
        .ok_or(ApiError::NotFound("trigger"))
}

#[derive(Debug, FromRow, serde::Serialize)]
pub struct TriggerEvent {
    pub id: Uuid,
    pub trigger_id: Uuid,
    pub fired_at: DateTime<Utc>,
    pub state: String,
    pub value: Option<f64>,
    pub message: String,
}

pub async fn get_trigger(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<TriggerPath>) -> ApiResult<Json<serde_json::Value>> {
    let t = load_trigger(&st, pa.project.id, p.trigger_id).await?;
    let events: Vec<TriggerEvent> = sqlx::query_as("SELECT * FROM trigger_events WHERE trigger_id = $1 ORDER BY fired_at DESC LIMIT 100")
        .bind(t.id)
        .fetch_all(&st.pg)
        .await?;
    #[derive(FromRow, serde::Serialize)]
    struct GroupState { group_key: String, severity: String, last_value: Option<f64>, breaching_since: Option<DateTime<Utc>>, updated_at: DateTime<Utc> }
    let groups: Vec<GroupState> = sqlx::query_as("SELECT group_key, severity, last_value, breaching_since, updated_at FROM trigger_group_states WHERE trigger_id = $1 ORDER BY severity DESC, last_value DESC NULLS LAST LIMIT 200").bind(t.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "trigger": t, "events": events, "groups": groups })))
}

pub async fn update_trigger(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<TriggerPath>, Json(b): Json<TriggerBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate_trigger(&b)?;
    let mute_until = b.mute_hours.map(|h| if h > 0.0 { Some(Utc::now() + chrono::Duration::seconds((h * 3600.0) as i64)) } else { None });
    let row: Option<TriggerRow> = sqlx::query_as(
        "UPDATE triggers SET name = $3, description = $4, query = $5, op = $6, threshold = $7, frequency_secs = $8, window_secs = $9, enabled = $10, recipients = $11, \
         warn_threshold = $12, for_secs = $13, mute_until = CASE WHEN $14::boolean THEN $15 ELSE mute_until END, per_group = $16, mode = $17, baseline_factor = $18, baseline_min_delta = $19, \
         sensitivity = $20, min_value = $21, composite = $22, updated_at = now(), last_evaluated_at = NULL WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(p.trigger_id).bind(pa.project.id).bind(b.name.trim()).bind(&b.description).bind(&b.query).bind(&b.op).bind(b.threshold)
    .bind(b.frequency_secs).bind(b.window_secs).bind(b.enabled).bind(&b.recipients).bind(b.warn_threshold).bind(b.for_secs).bind(mute_until.is_some()).bind(mute_until.flatten()).bind(b.per_group).bind(&b.mode).bind(b.baseline_factor).bind(b.baseline_min_delta).bind(b.sensitivity.clamp(1, 5)).bind(b.min_value).bind(&b.composite)
    .fetch_optional(&st.pg)
    .await?;
    audit::project(&st.pg, &pa, "trigger.update", "trigger", p.trigger_id, json!({ "name": b.name, "op": b.op, "threshold": b.threshold, "mode": b.mode })).await;
    Ok(Json(json!({ "trigger": row.ok_or(ApiError::NotFound("trigger"))? })))
}

pub async fn delete_trigger(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<TriggerPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let r = sqlx::query("DELETE FROM triggers WHERE id = $1 AND project_id = $2").bind(p.trigger_id).bind(pa.project.id).execute(&st.pg).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound("trigger"));
    }
    audit::project(&st.pg, &pa, "trigger.delete", "trigger", p.trigger_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

/// Evaluate right now without touching state: what would this trigger do?
pub async fn test_trigger(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<TriggerPath>) -> ApiResult<Json<serde_json::Value>> {
    let t = load_trigger(&st, pa.project.id, p.trigger_id).await?;
    let e = galileo_alerts::triggers::evaluate(&st.alerts, &t).await;
    Ok(Json(json!({ "evaluation": e })))
}

/// Dry-run an unsaved trigger definition.
pub async fn preview_trigger(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<TriggerBody>) -> ApiResult<Json<serde_json::Value>> {
    validate_trigger(&b)?;
    let t = TriggerRow {
        id: Uuid::nil(),
        project_id: pa.project.id,
        name: b.name,
        description: b.description,
        query: b.query,
        op: b.op,
        threshold: b.threshold,
        frequency_secs: b.frequency_secs,
        window_secs: b.window_secs,
        enabled: true,
        recipients: b.recipients,
        state: "ok".into(),
        last_value: None,
        last_evaluated_at: None,
        last_triggered_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        warn_threshold: b.warn_threshold,
        for_secs: b.for_secs,
        mute_until: None,
        per_group: b.per_group,
        mode: b.mode,
        baseline_factor: b.baseline_factor,
        baseline_min_delta: b.baseline_min_delta,
        breaching_since: None,
        severity: "ok".into(), sensitivity: Some(b.sensitivity), min_value: Some(b.min_value), composite: b.composite.clone(), mutes: None, last_baseline: None, last_band: None };
    let e = galileo_alerts::triggers::evaluate(&st.alerts, &t).await;
    Ok(Json(json!({ "evaluation": e })))
}

// ---------------------------------------------------------------- slos

#[derive(Deserialize)]
pub struct SloBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "d_ds")]
    pub dataset: String,
    #[serde(default = "empty_arr")]
    pub total_filters: serde_json::Value,
    #[serde(default = "empty_arr")]
    pub good_filters: serde_json::Value,
    pub target_pct: f64,
    #[serde(default = "d_days")]
    pub window_days: i32,
    #[serde(default = "empty_arr")]
    pub burn_alerts: serde_json::Value,
}
fn d_ds() -> String {
    "spans".into()
}
fn d_days() -> i32 {
    30
}

fn validate_slo(b: &SloBody) -> ApiResult<()> {
    if b.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name required".into()));
    }
    if !matches!(b.dataset.as_str(), "spans" | "logs" | "metrics") {
        return Err(ApiError::BadRequest("dataset must be spans, logs or metrics".into()));
    }
    if !(0.0 < b.target_pct && b.target_pct < 100.0) {
        return Err(ApiError::BadRequest("target_pct must be between 0 and 100 (exclusive)".into()));
    }
    if !(1..=90).contains(&b.window_days) {
        return Err(ApiError::BadRequest("window_days must be 1..90".into()));
    }
    for (label, v) in [("total_filters", &b.total_filters), ("good_filters", &b.good_filters)] {
        let f: Vec<galileo_query::Filter> = serde_json::from_value(v.clone()).map_err(|e| ApiError::BadRequest(format!("{label}: {e}")))?;
        for x in &f {
            galileo_query::fields::validate_name(&x.field).map_err(|e| ApiError::BadRequest(format!("{label}: {e}")))?;
        }
    }
    if serde_json::from_value::<Vec<galileo_alerts::slos::BurnAlert>>(b.burn_alerts.clone()).is_err() {
        return Err(ApiError::BadRequest("burn_alerts must be an array of {name, long_window_mins, short_window_mins, burn_rate, recipients}".into()));
    }
    let good: Vec<galileo_query::Filter> = serde_json::from_value(b.good_filters.clone()).unwrap_or_default();
    if good.is_empty() {
        return Err(ApiError::BadRequest("good_filters cannot be empty (every event would be good)".into()));
    }
    Ok(())
}

pub async fn list_slos(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<SloRow> = sqlx::query_as("SELECT * FROM slos WHERE project_id = $1 ORDER BY name").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "slos": rows })))
}

pub async fn create_slo(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<SloBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate_slo(&b)?;
    let row: SloRow = sqlx::query_as(
        "INSERT INTO slos (id, project_id, name, description, dataset, total_filters, good_filters, target_pct, window_days, burn_alerts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING *",
    )
    .bind(Uuid::now_v7()).bind(pa.project.id).bind(b.name.trim()).bind(&b.description).bind(&b.dataset).bind(&b.total_filters).bind(&b.good_filters)
    .bind(b.target_pct).bind(b.window_days).bind(&b.burn_alerts)
    .fetch_one(&st.pg)
    .await?;
    audit::project(&st.pg, &pa, "slo.create", "slo", row.id, json!({ "name": row.name, "target_pct": row.target_pct })).await;
    Ok(Json(json!({ "slo": row })))
}

#[derive(Deserialize)]
pub struct SloPath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub slo_id: Uuid,
}

async fn load_slo(st: &AppState, project: Uuid, id: Uuid) -> ApiResult<SloRow> {
    sqlx::query_as("SELECT * FROM slos WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project)
        .fetch_optional(&st.pg)
        .await?
        .ok_or(ApiError::NotFound("slo"))
}

/// Fresh evaluation with the daily SLI series.
pub async fn get_slo(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<SloPath>) -> ApiResult<Json<serde_json::Value>> {
    let s = load_slo(&st, pa.project.id, p.slo_id).await?;
    let result = galileo_alerts::slos::evaluate(&st.alerts, &s, true).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(json!({ "slo": s, "result": result })))
}

pub async fn update_slo(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<SloPath>, Json(b): Json<SloBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    validate_slo(&b)?;
    let row: Option<SloRow> = sqlx::query_as(
        "UPDATE slos SET name = $3, description = $4, dataset = $5, total_filters = $6, good_filters = $7, target_pct = $8, window_days = $9, burn_alerts = $10, \
         updated_at = now(), last_evaluated_at = NULL WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(p.slo_id).bind(pa.project.id).bind(b.name.trim()).bind(&b.description).bind(&b.dataset).bind(&b.total_filters).bind(&b.good_filters)
    .bind(b.target_pct).bind(b.window_days).bind(&b.burn_alerts)
    .fetch_optional(&st.pg)
    .await?;
    Ok(Json(json!({ "slo": row.ok_or(ApiError::NotFound("slo"))? })))
}

pub async fn delete_slo(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<SloPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let r = sqlx::query("DELETE FROM slos WHERE id = $1 AND project_id = $2").bind(p.slo_id).bind(pa.project.id).execute(&st.pg).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound("slo"));
    }
    audit::project(&st.pg, &pa, "slo.delete", "slo", p.slo_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- monitoring v2: incidents, mutes, windows, on-call

#[derive(Deserialize)]
pub struct IncidentParams { #[serde(default)] pub open: Option<bool>, #[serde(default = "d_limit")] pub limit: i64 }
fn d_limit() -> i64 { 50 }

pub async fn list_incidents(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<IncidentParams>) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT row_to_json(x) FROM (SELECT i.id, i.trigger_id, t.name AS trigger_name, i.group_key, i.severity, i.fired_at, i.acknowledged_at, i.acknowledged_by, i.resolved_at, i.peak_value, i.last_value, i.notified, i.escalated, i.notes FROM trigger_incidents i JOIN triggers t ON t.id = i.trigger_id WHERE i.project_id = $1 AND ($2::boolean IS NULL OR ($2 AND i.resolved_at IS NULL) OR (NOT $2 AND i.resolved_at IS NOT NULL)) ORDER BY i.fired_at DESC LIMIT $3) x")
        .bind(pa.project.id).bind(p.open).bind(p.limit.clamp(1, 500)).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "incidents": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}

pub async fn trigger_incidents(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<TriggerPath>) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT row_to_json(i) FROM (SELECT id, trigger_id, group_key, severity, fired_at, acknowledged_at, acknowledged_by, resolved_at, peak_value, last_value, notified, escalated, notes FROM trigger_incidents WHERE project_id = $1 AND trigger_id = $2 ORDER BY fired_at DESC LIMIT 100) i").bind(pa.project.id).bind(p.trigger_id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "incidents": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
pub struct IncidentPath { #[allow(dead_code)] pub project_id: Uuid, #[allow(dead_code)] pub trigger_id: Uuid, pub incident_id: Uuid }
#[derive(Deserialize)]
pub struct AckBody { #[serde(default)] pub note: String }

pub async fn ack_incident(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<IncidentPath>, Json(b): Json<AckBody>) -> ApiResult<Json<serde_json::Value>> {
    let n = sqlx::query("UPDATE trigger_incidents SET acknowledged_at = now(), acknowledged_by = $3, notes = CASE WHEN $4 = '' THEN notes ELSE notes || $4 END WHERE id = $1 AND project_id = $2 AND acknowledged_at IS NULL").bind(p.incident_id).bind(pa.project.id).bind(&pa.user.email).bind(&b.note).execute(&st.pg).await?.rows_affected();
    Ok(Json(json!({ "ok": n > 0 })))
}

/// Unauthenticated acknowledgement from a notification link (signed by the per-incident token).
pub async fn ack_by_token(State(st): State<AppState>, Path(token): Path<String>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let row: Option<(Uuid, String, Option<chrono::DateTime<Utc>>)> = sqlx::query_as("SELECT i.id, t.name, i.acknowledged_at FROM trigger_incidents i JOIN triggers t ON t.id = i.trigger_id WHERE i.ack_token = $1 AND length($1) >= 32").bind(&token).fetch_optional(&st.pg).await.unwrap_or(None);
    let body = match row {
        None => "<h2>Unknown or expired acknowledgement link</h2>".to_string(),
        Some((_, name, Some(at))) => format!("<h2>{name}</h2><p>Already acknowledged at {at}.</p>"),
        Some((id, name, None)) => { let _ = sqlx::query("UPDATE trigger_incidents SET acknowledged_at = now(), acknowledged_by = 'link' WHERE id = $1").bind(id).execute(&st.pg).await; format!("<h2>{name}</h2><p>Acknowledged. Repeat notifications and escalation stop; the incident closes when the trigger returns to ok.</p>") }
    };
    axum::response::Html(format!("<!doctype html><meta charset=utf-8><title>Galileo</title><body style=\"font-family:system-ui;background:#0d1017;color:#e6e9ef;padding:2rem\">{body}</body>")).into_response()
}

#[derive(Deserialize)]
pub struct MuteGroupBody { pub group_key: String, #[serde(default = "d_hours")] pub hours: f64, #[serde(default)] pub clear: bool }
fn d_hours() -> f64 { 24.0 }

pub async fn mute_group(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<TriggerPath>, Json(b): Json<MuteGroupBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    let cur: Option<(serde_json::Value,)> = sqlx::query_as("SELECT mutes FROM triggers WHERE id = $1 AND project_id = $2").bind(p.trigger_id).bind(pa.project.id).fetch_optional(&st.pg).await?;
    let Some((cur,)) = cur else { return Err(ApiError::NotFound("trigger")); };
    let mut list: Vec<serde_json::Value> = cur.as_array().cloned().unwrap_or_default().into_iter().filter(|m| m.get("group_key").and_then(|g| g.as_str()) != Some(b.group_key.as_str())).collect();
    if !b.clear { list.push(json!({ "group_key": b.group_key, "until": Utc::now() + chrono::Duration::seconds((b.hours * 3600.0) as i64) })); }
    sqlx::query("UPDATE triggers SET mutes = $3 WHERE id = $1 AND project_id = $2").bind(p.trigger_id).bind(pa.project.id).bind(json!(list)).execute(&st.pg).await?;
    Ok(Json(json!({ "mutes": list })))
}

#[derive(Deserialize)]
pub struct WindowBody { pub name: String, pub starts_at: chrono::DateTime<Utc>, pub ends_at: chrono::DateTime<Utc>, #[serde(default)] pub trigger_ids: Vec<Uuid> }

pub async fn list_windows(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT row_to_json(w) FROM (SELECT id, name, starts_at, ends_at, trigger_ids, created_at FROM maintenance_windows WHERE project_id = $1 AND ends_at > now() - interval '7 days' ORDER BY starts_at DESC) w").bind(pa.project.id).fetch_all(&st.pg).await?;
    Ok(Json(json!({ "windows": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() })))
}
pub async fn create_window(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<WindowBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if b.ends_at <= b.starts_at { return Err(ApiError::BadRequest("ends_at must be after starts_at".into())); }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO maintenance_windows (id, project_id, name, starts_at, ends_at, trigger_ids, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7)").bind(id).bind(pa.project.id).bind(b.name.trim()).bind(b.starts_at).bind(b.ends_at).bind(&b.trigger_ids).bind(pa.user.id).execute(&st.pg).await?;
    Ok(Json(json!({ "id": id })))
}
#[derive(Deserialize)]
pub struct WindowPath { #[allow(dead_code)] pub project_id: Uuid, pub window_id: Uuid }
pub async fn delete_window(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<WindowPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    sqlx::query("DELETE FROM maintenance_windows WHERE id = $1 AND project_id = $2").bind(p.window_id).bind(pa.project.id).execute(&st.pg).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct OncallBody { pub name: String, pub members: Vec<Uuid>, #[serde(default = "d_rot")] pub rotation_days: i32, pub starts_on: chrono::NaiveDate, #[serde(default)] pub escalation: serde_json::Value }
fn d_rot() -> i32 { 7 }

pub async fn list_oncall(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    let rows: Vec<(Uuid, String, Vec<Uuid>, i32, chrono::NaiveDate, serde_json::Value)> = sqlx::query_as("SELECT id, name, members, rotation_days, starts_on, escalation FROM oncall_schedules WHERE project_id = $1 ORDER BY name").bind(pa.project.id).fetch_all(&st.pg).await?;
    let mut out = vec![];
    for (id, name, members, days, starts, esc) in rows {
        let now = galileo_alerts::incidents::current_oncall(&st.pg, id, Utc::now()).await;
        let emails: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, email FROM users WHERE id = ANY($1)").bind(&members).fetch_all(&st.pg).await.unwrap_or_default();
        out.push(json!({ "id": id, "name": name, "members": members, "member_emails": members.iter().map(|m| emails.iter().find(|e| e.0 == *m).map(|e| e.1.clone()).unwrap_or_default()).collect::<Vec<_>>(), "rotation_days": days, "starts_on": starts, "escalation": esc, "now": now.map(|(u, e)| json!({ "user_id": u, "email": e })) }));
    }
    Ok(Json(json!({ "schedules": out })))
}
pub async fn create_oncall(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<OncallBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if b.members.is_empty() { return Err(ApiError::BadRequest("at least one member".into())); }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO oncall_schedules (id, project_id, name, members, rotation_days, starts_on, escalation) VALUES ($1, $2, $3, $4, $5, $6, $7)").bind(id).bind(pa.project.id).bind(b.name.trim()).bind(&b.members).bind(b.rotation_days.max(1)).bind(b.starts_on).bind(if b.escalation.is_array() { b.escalation.clone() } else { json!([]) }).execute(&st.pg).await?;
    Ok(Json(json!({ "id": id })))
}
#[derive(Deserialize)]
pub struct OncallPath { #[allow(dead_code)] pub project_id: Uuid, pub schedule_id: Uuid }
pub async fn update_oncall(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<OncallPath>, Json(b): Json<OncallBody>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    sqlx::query("UPDATE oncall_schedules SET name = $3, members = $4, rotation_days = $5, starts_on = $6, escalation = $7 WHERE id = $1 AND project_id = $2").bind(p.schedule_id).bind(pa.project.id).bind(b.name.trim()).bind(&b.members).bind(b.rotation_days.max(1)).bind(b.starts_on).bind(if b.escalation.is_array() { b.escalation.clone() } else { json!([]) }).execute(&st.pg).await?;
    Ok(Json(json!({ "ok": true })))
}
pub async fn delete_oncall(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<OncallPath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    sqlx::query("DELETE FROM oncall_schedules WHERE id = $1 AND project_id = $2").bind(p.schedule_id).bind(pa.project.id).execute(&st.pg).await?;
    Ok(Json(json!({ "ok": true })))
}
