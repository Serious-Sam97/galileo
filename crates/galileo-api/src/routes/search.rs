//! ⌘K search: one call that looks across issues, boards, routes, triggers, prompts, users and
//! recent queries of a project.

use axum::extract::{Query as QueryParams, State};
use axum::Json;
use galileo_storage::SqlQuery;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::ProjectAccess;
use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SearchParams { #[serde(default)] pub q: String }

async fn pg_rows(pg: &sqlx::PgPool, sql: &str, project: Uuid, like: &str) -> Vec<Value> {
    let r: Vec<(Value,)> = sqlx::query_as(sql).bind(project).bind(like).fetch_all(pg).await.unwrap_or_default();
    r.into_iter().map(|x| x.0).collect()
}

pub async fn search(State(st): State<AppState>, pa: ProjectAccess, QueryParams(p): QueryParams<SearchParams>) -> ApiResult<Json<Value>> {
    let q = p.q.trim().to_string();
    if q.is_empty() { return Ok(Json(json!({ "issues": [], "boards": [], "routes": [], "triggers": [], "prompts": [], "users": [], "queries": [] }))); }
    // a single character only makes sense for identities (melea user ids are short integers)
    let pg_ok = q.chars().count() >= 2;
    let like = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
    let pid = pa.project.id; let pg = &st.pg;
    let issues = if !pg_ok { vec![] } else { pg_rows(pg, "SELECT row_to_json(i) FROM (SELECT id, title, status FROM issues WHERE project_id = $1 AND (title ILIKE $2 OR culprit ILIKE $2) ORDER BY last_seen DESC LIMIT 6) i", pid, &like).await };
    let boards = if !pg_ok { vec![] } else { pg_rows(pg, "SELECT row_to_json(b) FROM (SELECT id, name FROM boards WHERE project_id = $1 AND name ILIKE $2 ORDER BY name LIMIT 5) b", pid, &like).await };
    let routes = if !pg_ok { vec![] } else { pg_rows(pg, "SELECT row_to_json(r) FROM (SELECT id, alias FROM gateway_routes WHERE project_id = $1 AND alias ILIKE $2 ORDER BY alias LIMIT 5) r", pid, &like).await };
    let triggers = if !pg_ok { vec![] } else { pg_rows(pg, "SELECT row_to_json(t) FROM (SELECT id, name, state FROM triggers WHERE project_id = $1 AND name ILIKE $2 ORDER BY name LIMIT 5) t", pid, &like).await };
    let prompts = if !pg_ok { vec![] } else { pg_rows(pg, "SELECT row_to_json(p) FROM (SELECT id, name FROM prompts WHERE project_id = $1 AND name ILIKE $2 ORDER BY name LIMIT 5) p", pid, &like).await };
    let queries = if !pg_ok { vec![] } else { pg_rows(pg, "SELECT row_to_json(h) FROM (SELECT DISTINCT ON (text) id, text FROM query_history WHERE project_id = $1 AND text ILIKE $2 ORDER BY text, created_at DESC LIMIT 5) h", pid, &like).await };
    // users: identity seen in spans in the last 7 days
    let since = (chrono::Utc::now() - chrono::Duration::days(7)).timestamp_nanos_opt().unwrap_or(0);
    let users = st.storage.query(&SqlQuery { sql: "SELECT user_id, count() AS c FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND user_id != '' AND positionCaseInsensitive(user_id, ?) > 0 GROUP BY user_id ORDER BY c DESC LIMIT 5".into(), params: vec![galileo_core::ProjectId(pid).into(), since.into(), q.clone().into()] }).await
        .map(|r| r.rows.iter().map(|row| json!({ "user_id": row.first().and_then(|v| v.as_str()).unwrap_or(""), "count": row.get(1).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0) })).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(Json(json!({ "issues": issues, "boards": boards, "routes": routes, "triggers": triggers, "prompts": prompts, "users": users, "queries": queries })))
}
