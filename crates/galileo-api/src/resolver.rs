//! API key → project resolution backed by Postgres, with a short in-memory cache so ingest
//! never waits on the database per request. Redaction rules ride along in the cache so the
//! receivers get a ready-to-use `Redactor`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use galileo_core::{ProjectId, RedactionRule, Redactor};
use galileo_otlp::{ApiKeyResolver, ProjectContext};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use crate::auth::hash_api_key;

const TTL: Duration = Duration::from_secs(60);
const NEGATIVE_TTL: Duration = Duration::from_secs(10);

struct Entry {
    ctx: Option<ProjectContext>,
    at: Instant,
}

pub struct PgResolver {
    pool: PgPool,
    cache: DashMap<String, Entry>,
}

impl PgResolver {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, cache: DashMap::new() }
    }

    /// Call after keys or redaction rules change.
    pub fn invalidate_all(&self) {
        self.cache.clear();
    }

    pub async fn redactor_for(pool: &PgPool, project_id: Uuid) -> Arc<Redactor> {
        let rules: Vec<(serde_json::Value,)> =
            sqlx::query_as("SELECT rule FROM redaction_rules WHERE project_id = $1 ORDER BY created_at")
                .bind(project_id)
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        let parsed: Vec<RedactionRule> = rules
            .into_iter()
            .filter_map(|(v,)| serde_json::from_value(v).ok())
            .collect();
        match Redactor::with_defaults(&parsed) {
            Ok(r) => Arc::new(r),
            Err(e) => {
                warn!(%project_id, error = %e, "invalid redaction rule, falling back to defaults");
                Arc::new(Redactor::with_defaults(&[]).expect("defaults compile"))
            }
        }
    }

    async fn lookup(&self, raw: &str) -> (Option<ProjectContext>, Option<Uuid>) {
        let hash = hash_api_key(raw);
        let row: Option<(Uuid, Uuid, Vec<String>)> = sqlx::query_as(
            "SELECT id, project_id, scopes FROM api_keys WHERE key_hash = $1 AND revoked_at IS NULL",
        )
        .bind(&hash)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);
        match row {
            Some((key_id, project_id, scopes)) => {
                let redactor = Self::redactor_for(&self.pool, project_id).await;
                let sampling: galileo_core::Sampling = sqlx::query_as::<_, (serde_json::Value,)>("SELECT sampling FROM project_settings WHERE project_id = $1")
                    .bind(project_id).fetch_optional(&self.pool).await.ok().flatten()
                    .and_then(|(v,)| serde_json::from_value(v).ok()).unwrap_or_default();
                let pipeline: galileo_core::LogPipeline = sqlx::query_as::<_, (serde_json::Value,)>("SELECT pipeline FROM log_pipelines WHERE project_id = $1").bind(project_id).fetch_optional(&self.pool).await.ok().flatten().and_then(|(v,)| serde_json::from_value(v).ok()).unwrap_or_default();
                let log_metrics: Vec<galileo_core::LogMetric> = sqlx::query_as::<_, (serde_json::Value,)>("SELECT rule FROM log_metrics WHERE project_id = $1").bind(project_id).fetch_all(&self.pool).await.unwrap_or_default().into_iter().filter_map(|(v,)| serde_json::from_value(v).ok()).collect();
                let quotas: galileo_core::Quotas = sqlx::query_as::<_, (serde_json::Value,)>("SELECT quotas FROM project_settings WHERE project_id = $1").bind(project_id).fetch_optional(&self.pool).await.ok().flatten().and_then(|(v,)| serde_json::from_value(v).ok()).unwrap_or_default();
                (Some(ProjectContext { project_id: ProjectId(project_id), redactor, scopes, sampling: sampling.normalized(), pipeline: Arc::new(pipeline), log_metrics: Arc::new(log_metrics), quotas }), Some(key_id))
            }
            None => (None, None),
        }
    }
}

#[async_trait]
impl ApiKeyResolver for PgResolver {
    async fn resolve(&self, raw_key: &str) -> Option<ProjectContext> {
        if let Some(e) = self.cache.get(raw_key) {
            let ttl = if e.ctx.is_some() { TTL } else { NEGATIVE_TTL };
            if e.at.elapsed() < ttl {
                return e.ctx.clone();
            }
        }
        let (ctx, key_id) = self.lookup(raw_key).await;
        if let Some(id) = key_id {
            // Best-effort, throttled by the cache TTL: at most one update per key per minute.
            let pool = self.pool.clone();
            tokio::spawn(async move {
                let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
                    .bind(id)
                    .execute(&pool)
                    .await;
            });
        }
        self.cache.insert(raw_key.to_owned(), Entry { ctx: ctx.clone(), at: Instant::now() });
        ctx
    }
}
