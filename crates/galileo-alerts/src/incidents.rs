//! Incidents (fired → acknowledged → resolved), composite triggers, escalation and repeats.

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::notify::Notification;
use crate::triggers::TriggerRow;
use crate::Evaluator;

pub const RENOTIFY_SECS: i64 = 1800;

fn token() -> String { let u = Uuid::new_v4(); u.simple().to_string() }

/// Open or close an incident for (trigger, group) on a severity change.
pub async fn transition(ev: &Evaluator, t: &TriggerRow, group_key: &str, severity: &str, value: Option<f64>, now: DateTime<Utc>) {
    let open: Option<(Uuid, Option<f64>)> = sqlx::query_as("SELECT id, peak_value FROM trigger_incidents WHERE trigger_id = $1 AND group_key = $2 AND resolved_at IS NULL").bind(t.id).bind(group_key).fetch_optional(&ev.pg).await.unwrap_or(None);
    match (severity, open) {
        ("ok", Some((id, _))) => { let _ = sqlx::query("UPDATE trigger_incidents SET resolved_at = $2, last_value = $3 WHERE id = $1").bind(id).bind(now).bind(value).execute(&ev.pg).await; }
        ("ok", None) => {}
        (sev, Some((id, peak))) => { let _ = sqlx::query("UPDATE trigger_incidents SET severity = $2, last_value = $3, peak_value = $4 WHERE id = $1").bind(id).bind(sev).bind(value).bind(peak.map(|p| value.map(|v| v.max(p)).unwrap_or(p)).or(value)).execute(&ev.pg).await; }
        (sev, None) => {
            let _ = sqlx::query("INSERT INTO trigger_incidents (id, project_id, trigger_id, group_key, severity, fired_at, peak_value, last_value, notified, ack_token) VALUES ($1, $2, $3, $4, $5, $6, $7, $7, 1, $8)")
                .bind(Uuid::now_v7()).bind(t.project_id).bind(t.id).bind(group_key).bind(sev).bind(now).bind(value).bind(token()).execute(&ev.pg).await;
        }
    }
}

pub fn ack_url(ev: &Evaluator, token: &str) -> String { format!("{}/api/ack/{token}", ev.public_url) }

/// Repeats for unacknowledged incidents every RENOTIFY_SECS and escalation through on-call
/// schedules named in the trigger's recipients.
type OpenIncident = (Uuid, Uuid, Uuid, String, String, DateTime<Utc>, Option<f64>, i32, i32, String);

pub async fn escalate_due(ev: &Evaluator, now: DateTime<Utc>) {
    let rows: Vec<OpenIncident> = sqlx::query_as(
        "SELECT i.id, i.project_id, i.trigger_id, i.group_key, i.severity, i.fired_at, i.last_value, i.notified, i.escalated, i.ack_token FROM trigger_incidents i WHERE i.resolved_at IS NULL AND i.acknowledged_at IS NULL")
        .fetch_all(&ev.pg).await.unwrap_or_default();
    for (id, project, trigger_id, group_key, severity, fired_at, value, notified, escalated, tok) in rows {
        let t: Option<TriggerRow> = sqlx::query_as("SELECT * FROM triggers WHERE id = $1").bind(trigger_id).fetch_optional(&ev.pg).await.unwrap_or(None);
        let Some(t) = t else { continue };
        let age = (now - fired_at).num_seconds();
        // escalation steps come from on-call schedules referenced by the trigger's recipients
        let schedules: Vec<Uuid> = t.recipients.as_array().map(|a| a.iter().filter(|r| r.get("type").and_then(|x| x.as_str()) == Some("oncall")).filter_map(|r| r.get("id").and_then(|x| x.as_str()).and_then(|s| Uuid::parse_str(s).ok())).collect()).unwrap_or_default();
        let mut steps: Vec<(i64, Uuid)> = vec![];
        for sid in &schedules {
            let esc: Option<(serde_json::Value,)> = sqlx::query_as("SELECT escalation FROM oncall_schedules WHERE id = $1").bind(sid).fetch_optional(&ev.pg).await.unwrap_or(None);
            if let Some((e,)) = esc { for st in e.as_array().cloned().unwrap_or_default() { if let (Some(a), Some(c)) = (st.get("after_secs").and_then(|x| x.as_i64()), st.get("channel_id").and_then(|x| x.as_str()).and_then(|s| Uuid::parse_str(s).ok())) { steps.push((a, c)); } } }
        }
        steps.sort();
        let title = format!("{}{} · unacknowledged for {}m", t.name, if group_key.is_empty() { String::new() } else { format!(" · {group_key}") }, age / 60);
        let n = |state: &str, msg: String| Notification { kind: "incident", state: state.into(), name: t.name.clone(), project_id: project, title: title.clone(), message: format!("{msg}\nAcknowledge: {}", ack_url(ev, &tok)), value, threshold: Some(t.threshold), url: Some(format!("{}/p/{}/triggers/{}", ev.public_url, project, t.id)), at: now };
        if let Some((after, channel)) = steps.get(escalated as usize) {
            if age >= *after {
                ev.notify(project, &serde_json::json!([{ "type": "channel", "id": channel }]), &n(&severity, format!("Escalation step {} after {}s without acknowledgement.", escalated + 1, after))).await;
                let _ = sqlx::query("UPDATE trigger_incidents SET escalated = escalated + 1 WHERE id = $1").bind(id).execute(&ev.pg).await;
                continue;
            }
        }
        if age >= RENOTIFY_SECS * notified as i64 {
            ev.notify(project, &t.recipients, &n(&severity, format!("Still {severity} (value {}). Repeats every {} min until acknowledged.", value.map(|v| format!("{v:.3}")).unwrap_or_else(|| "n/a".into()), RENOTIFY_SECS / 60))).await;
            let _ = sqlx::query("UPDATE trigger_incidents SET notified = notified + 1 WHERE id = $1").bind(id).execute(&ev.pg).await;
        }
    }
}

/// Composite trigger: fires when member triggers are in the required state within `within_secs`.
pub async fn evaluate_composite(ev: &Evaluator, t: &TriggerRow, now: DateTime<Utc>) {
    let Some(c) = t.composite.as_ref() else { return };
    let within = c.get("within_secs").and_then(|x| x.as_i64()).unwrap_or(600);
    let (mode, ids): (&str, Vec<Uuid>) = if let Some(a) = c.get("all_of").and_then(|x| x.as_array()) { ("all_of", a.iter().filter_map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok())).collect()) } else { ("any_of", c.get("any_of").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok())).collect()).unwrap_or_default()) };
    if ids.is_empty() { return; }
    let members: Vec<(Uuid, String, Option<DateTime<Utc>>, String)> = sqlx::query_as("SELECT id, state, last_triggered_at, name FROM triggers WHERE id = ANY($1)").bind(&ids).fetch_all(&ev.pg).await.unwrap_or_default();
    let hot = |m: &(Uuid, String, Option<DateTime<Utc>>, String)| m.1 == "triggered" || m.2.map(|at| (now - at) < Duration::seconds(within)).unwrap_or(false);
    let fired = if mode == "all_of" { members.len() == ids.len() && members.iter().all(hot) } else { members.iter().any(hot) };
    let new_state = if fired { "triggered" } else { "ok" };
    if new_state != t.state {
        let names: Vec<String> = members.iter().filter(|m| hot(m)).map(|m| m.3.clone()).collect();
        let msg = format!("composite ({mode} within {within}s): {}", if fired { format!("firing members: {}", names.join(", ")) } else { "members back to ok".into() });
        let _ = sqlx::query("UPDATE triggers SET state = $2, severity = $3, last_evaluated_at = now(), last_triggered_at = CASE WHEN $2 = 'triggered' THEN now() ELSE last_triggered_at END WHERE id = $1").bind(t.id).bind(new_state).bind(if fired { "critical" } else { "ok" }).execute(&ev.pg).await;
        let _ = sqlx::query("INSERT INTO trigger_events (id, trigger_id, state, value, message) VALUES ($1, $2, $3, NULL, $4)").bind(Uuid::now_v7()).bind(t.id).bind(new_state).bind(&msg).execute(&ev.pg).await;
        transition(ev, t, "", if fired { "critical" } else { "ok" }, None, now).await;
        ev.notify(t.project_id, &t.recipients, &Notification { kind: "trigger", state: if fired { "critical".into() } else { "ok".into() }, name: t.name.clone(), project_id: t.project_id, title: format!("Composite: {}", t.name), message: msg, value: None, threshold: None, url: Some(format!("{}/p/{}/triggers/{}", ev.public_url, t.project_id, t.id)), at: now }).await;
    } else {
        let _ = sqlx::query("UPDATE triggers SET last_evaluated_at = now() WHERE id = $1").bind(t.id).execute(&ev.pg).await;
    }
}

/// Who is on call now for a schedule: rotation over members every `rotation_days` from `starts_on`.
pub async fn current_oncall(pg: &sqlx::PgPool, schedule: Uuid, now: DateTime<Utc>) -> Option<(Uuid, String)> {
    let row: Option<(Vec<Uuid>, i32, chrono::NaiveDate)> = sqlx::query_as("SELECT members, rotation_days, starts_on FROM oncall_schedules WHERE id = $1").bind(schedule).fetch_optional(pg).await.ok().flatten();
    let (members, days, starts) = row?;
    if members.is_empty() { return None; }
    let elapsed = (now.date_naive() - starts).num_days().max(0);
    let idx = ((elapsed / days.max(1) as i64) as usize) % members.len();
    let uid = members[idx];
    let email: Option<(String,)> = sqlx::query_as("SELECT email FROM users WHERE id = $1").bind(uid).fetch_optional(pg).await.ok().flatten();
    Some((uid, email.map(|e| e.0).unwrap_or_default()))
}
