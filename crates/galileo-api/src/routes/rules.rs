use axum::extract::{Path, State};
use axum::Json;
use galileo_core::{Attributes, RedactionRule, Redactor};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::auth::ProjectAccess;
use crate::db::rules;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub async fn list(State(st): State<AppState>, pa: ProjectAccess) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({
        "rules": rules::list(&st.pg, pa.project.id).await?,
        "defaults": Redactor::default_rules(),
    })))
}

#[derive(Deserialize)]
pub struct CreateRule {
    pub rule: RedactionRule,
    #[serde(default)]
    pub description: String,
}

pub async fn create(State(st): State<AppState>, pa: ProjectAccess, Json(b): Json<CreateRule>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    Redactor::compile(std::slice::from_ref(&b.rule)).map_err(|e| ApiError::BadRequest(format!("invalid rule: {e}")))?;
    let row = rules::create(&st.pg, pa.project.id, serde_json::to_value(&b.rule).unwrap(), &b.description).await?;
    st.resolver.invalidate_all();
    audit::project(&st.pg, &pa, "redaction_rule.create", "redaction_rule", row.id, json!({ "rule": row.rule })).await;
    Ok(Json(json!({ "rule": row })))
}

#[derive(Deserialize)]
pub struct RulePath {
    #[allow(dead_code)]
    pub project_id: Uuid,
    pub rule_id: Uuid,
}

pub async fn delete(State(st): State<AppState>, pa: ProjectAccess, Path(p): Path<RulePath>) -> ApiResult<Json<serde_json::Value>> {
    pa.require_write()?;
    if !rules::delete(&st.pg, pa.project.id, p.rule_id).await? {
        return Err(ApiError::NotFound("rule"));
    }
    st.resolver.invalidate_all();
    audit::project(&st.pg, &pa, "redaction_rule.delete", "redaction_rule", p.rule_id, json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct TestBody {
    #[serde(default)]
    pub rules: Vec<RedactionRule>,
    #[serde(default)]
    pub include_defaults: bool,
    #[serde(default)]
    pub attributes: Attributes,
    #[serde(default)]
    pub text: String,
}

/// Dry-run redaction so users can see what a rule does before saving it.
pub async fn test_rules(_pa: ProjectAccess, Json(b): Json<TestBody>) -> ApiResult<Json<serde_json::Value>> {
    let r = if b.include_defaults { Redactor::with_defaults(&b.rules) } else { Redactor::compile(&b.rules) }
        .map_err(|e| ApiError::BadRequest(format!("invalid rule: {e}")))?;
    let mut attrs = b.attributes;
    r.redact_attrs(&mut attrs);
    Ok(Json(json!({ "attributes": attrs, "text": r.redact_text(&b.text) })))
}
