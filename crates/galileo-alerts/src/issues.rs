//! Issues: exceptions grouped by fingerprint. Every tick, new occurrences since the last run
//! are rolled up from ClickHouse and applied to the Postgres issue rows. State machine:
//!
//!   (none) --occurrence--> open            → "new"       (notify)
//!   open   --occurrence--> open            → counts only
//!   resolved --occurrence--> open          → "regressed" (notify)
//!   ignored --occurrence--> ignored        → counts only, no noise
//!
//! Resolve/ignore/reopen come from the API and write their own events.

use chrono::{DateTime, Duration, Utc};
use galileo_core::ProjectId;
use galileo_storage::SqlQuery;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::notify::Notification;
use crate::Evaluator;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct IssueRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub fingerprint: String,
    pub title: String,
    pub exception_type: String,
    pub culprit: String,
    pub route: String,
    pub service_name: String,
    pub status: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub count: i64,
    pub users: i64,
    pub last_trace_id: String,
    pub last_version: String,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_version: String,
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One fingerprint's occurrences in a window.
#[derive(Debug, Clone)]
pub struct Occurrence {
    pub fingerprint: String,
    pub exception_type: String,
    pub message: String,
    pub culprit: String,
    pub route: String,
    pub service_name: String,
    pub count: i64,
    pub users: i64,
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
    pub trace_id: String,
    pub version: String,
}

/// What happened to an issue after applying occurrences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Created,
    Updated,
    Regressed,
    IgnoredUpdate,
}

/// Pure decision: given the current status, what does a new occurrence do?
pub fn transition(current: Option<&str>) -> Transition {
    match current {
        None => Transition::Created,
        Some("resolved") => Transition::Regressed,
        Some("ignored") => Transition::IgnoredUpdate,
        _ => Transition::Updated,
    }
}

pub async fn occurrences(ev: &Evaluator, project: Uuid, since: DateTime<Utc>, until: DateTime<Utc>) -> anyhow::Result<Vec<Occurrence>> {
    let q = SqlQuery {
        sql: "SELECT exception_fingerprint, any(exception_type), any(exception_message), any(exception_culprit), any(http_route), any(service_name), \
              count(), uniqIf(user_id, user_id != ''), min(timestamp), max(timestamp), argMax(trace_id, timestamp), argMax(service_version, timestamp) \
              FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?) AND exception_fingerprint != '' \
              GROUP BY exception_fingerprint ORDER BY count() DESC LIMIT 2000"
            .into(),
        params: vec![ProjectId(project).into(), since.into(), until.into()],
    };
    let res = ev.storage.query(&q).await?;
    let st = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
    let n = |v: &serde_json::Value| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
    let ts = |v: &serde_json::Value| v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc)).unwrap_or(until);
    Ok(res
        .rows
        .iter()
        .map(|r| Occurrence {
            fingerprint: st(&r[0]),
            exception_type: st(&r[1]),
            message: st(&r[2]),
            culprit: st(&r[3]),
            route: st(&r[4]),
            service_name: st(&r[5]),
            count: n(&r[6]),
            users: n(&r[7]),
            first: ts(&r[8]),
            last: ts(&r[9]),
            trace_id: st(&r[10]),
            version: st(&r[11]),
        })
        .collect())
}

fn title_of(o: &Occurrence) -> String {
    let ty = o.exception_type.rsplit('.').next().unwrap_or(&o.exception_type);
    let msg: String = o.message.chars().take(140).collect();
    if msg.is_empty() { ty.to_string() } else { format!("{ty}: {msg}") }
}

/// Apply one occurrence batch to the issues table. Returns the transition and the row.
pub async fn apply(pg: &sqlx::PgPool, project: Uuid, o: &Occurrence) -> anyhow::Result<(Transition, IssueRow)> {
    let existing: Option<IssueRow> = sqlx::query_as("SELECT * FROM issues WHERE project_id = $1 AND fingerprint = $2")
        .bind(project)
        .bind(&o.fingerprint)
        .fetch_optional(pg)
        .await?;
    let t = transition(existing.as_ref().map(|e| e.status.as_str()));
    let row: IssueRow = match (&existing, t) {
        (None, _) => {
            sqlx::query_as(
                "INSERT INTO issues (id, project_id, fingerprint, title, exception_type, culprit, route, service_name, status, first_seen, last_seen, count, users, last_trace_id, last_version) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'open', $9, $10, $11, $12, $13, $14) RETURNING *",
            )
            .bind(Uuid::now_v7()).bind(project).bind(&o.fingerprint).bind(title_of(o)).bind(&o.exception_type).bind(&o.culprit).bind(&o.route).bind(&o.service_name)
            .bind(o.first).bind(o.last).bind(o.count).bind(o.users).bind(&o.trace_id).bind(&o.version)
            .fetch_one(pg).await?
        }
        (Some(e), Transition::Regressed) => {
            sqlx::query_as(
                "UPDATE issues SET status = 'open', resolved_at = NULL, resolved_version = '', last_seen = $2, count = count + $3, users = greatest(users, $4), \
                 last_trace_id = $5, last_version = $6, title = $7, updated_at = now() WHERE id = $1 RETURNING *",
            )
            .bind(e.id).bind(o.last).bind(o.count).bind(o.users).bind(&o.trace_id).bind(&o.version).bind(title_of(o))
            .fetch_one(pg).await?
        }
        (Some(e), _) => {
            sqlx::query_as(
                "UPDATE issues SET last_seen = greatest(last_seen, $2), count = count + $3, users = greatest(users, $4), last_trace_id = $5, last_version = $6, updated_at = now() \
                 WHERE id = $1 RETURNING *",
            )
            .bind(e.id).bind(o.last).bind(o.count).bind(o.users).bind(&o.trace_id).bind(&o.version)
            .fetch_one(pg).await?
        }
    };
    match t {
        Transition::Created => add_event(pg, row.id, "new", &format!("first seen on {} ({} times, {} users)", o.route, o.count, o.users), None).await?,
        Transition::Regressed => add_event(pg, row.id, "regressed", &format!("seen again after being resolved{}", if o.version.is_empty() { String::new() } else { format!(" (version {})", o.version) }), None).await?,
        _ => {}
    }
    Ok((t, row))
}

pub async fn add_event(pg: &sqlx::PgPool, issue: Uuid, kind: &str, message: &str, user: Option<Uuid>) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO issue_events (id, issue_id, kind, message, user_id) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::now_v7()).bind(issue).bind(kind).bind(message).bind(user)
        .execute(pg).await?;
    Ok(())
}

pub async fn evaluate_due(ev: &Evaluator) -> anyhow::Result<()> {
    let projects: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM projects").fetch_all(&ev.pg).await?;
    let now = Utc::now() - Duration::seconds(5); // let the ingest batch settle
    for (project,) in projects {
        let last: Option<(Option<DateTime<Utc>>, serde_json::Value)> =
            sqlx::query_as("SELECT issues_last_run, issue_recipients FROM project_settings WHERE project_id = $1")
                .bind(project).fetch_optional(&ev.pg).await?;
        let (since, recipients_json) = match last {
            Some((Some(t), r)) => (t, r),
            Some((None, r)) => (now - Duration::hours(24), r),
            None => (now - Duration::hours(24), serde_json::json!([])),
        };
        if since >= now {
            continue;
        }
        let occ = occurrences(ev, project, since, now).await?;
        let has_recipients = recipients_json.as_array().map(|a| !a.is_empty()).unwrap_or(false);
        for o in &occ {
            let (t, row) = apply(&ev.pg, project, o).await?;
            if has_recipients && matches!(t, Transition::Created | Transition::Regressed) {
                ev.notify(project, &recipients_json, &Notification {
                    kind: "issue",
                    state: if t == Transition::Created { "new".into() } else { "regressed".into() },
                    name: row.title.clone(),
                    project_id: project,
                    title: format!("{} issue: {}", if t == Transition::Created { "New" } else { "Regressed" }, row.title),
                    message: format!("{} · {} · {} occurrences, {} users · trace {}", row.culprit, row.route, o.count, o.users, o.trace_id),
                    value: Some(o.count as f64),
                    threshold: None,
                    url: Some(format!("{}/p/{}/issues/{}", ev.public_url, project, row.id)),
                    at: Utc::now(),
                }).await;
            }
        }
        sqlx::query("INSERT INTO project_settings (project_id, issues_last_run) VALUES ($1, $2) ON CONFLICT (project_id) DO UPDATE SET issues_last_run = $2")
            .bind(project).bind(now).execute(&ev.pg).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine() {
        assert_eq!(transition(None), Transition::Created);
        assert_eq!(transition(Some("open")), Transition::Updated);
        assert_eq!(transition(Some("resolved")), Transition::Regressed);
        assert_eq!(transition(Some("ignored")), Transition::IgnoredUpdate);
    }

    #[test]
    fn titles_are_short_and_readable() {
        let o = Occurrence { fingerprint: "f".into(), exception_type: "config.chaos.InvoiceSyncError".into(), message: "Asaas secret missing".into(), culprit: "".into(), route: "".into(), service_name: "".into(), count: 1, users: 0, first: Utc::now(), last: Utc::now(), trace_id: "".into(), version: "".into() };
        assert_eq!(title_of(&o), "InvoiceSyncError: Asaas secret missing");
    }
}
