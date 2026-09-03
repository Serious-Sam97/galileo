//! Audit log: who changed what. Called from mutating handlers; failures never fail the request.

use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use crate::auth::{CurrentUser, ProjectAccess};

#[allow(clippy::too_many_arguments)]
pub async fn record(pg: &PgPool, org_id: Uuid, project_id: Option<Uuid>, user: Option<(Uuid, &str)>, action: &str, target_type: &str, target_id: &str, details: serde_json::Value) {
    let res = sqlx::query("INSERT INTO audit_log (id, org_id, project_id, user_id, user_email, action, target_type, target_id, details) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
        .bind(Uuid::now_v7()).bind(org_id).bind(project_id).bind(user.map(|u| u.0)).bind(user.map(|u| u.1).unwrap_or(""))
        .bind(action).bind(target_type).bind(target_id).bind(details)
        .execute(pg).await;
    if let Err(e) = res {
        warn!(error = %e, action, "audit write failed");
    }
}

/// Project-scoped convenience.
pub async fn project(pg: &PgPool, pa: &ProjectAccess, action: &str, target_type: &str, target_id: impl ToString, details: serde_json::Value) {
    record(pg, pa.project.org_id, Some(pa.project.id), Some((pa.user.id, &pa.user.email)), action, target_type, &target_id.to_string(), details).await;
}

/// Org-scoped convenience.
pub async fn org(pg: &PgPool, org_id: Uuid, cu: &CurrentUser, action: &str, target_type: &str, target_id: impl ToString, details: serde_json::Value) {
    record(pg, org_id, None, Some((cu.user.id, &cu.user.email)), action, target_type, &target_id.to_string(), details).await;
}
