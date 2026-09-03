use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use galileo_core::{ProjectId, Redactor};

/// What an authenticated ingest request runs as.
#[derive(Clone)]
pub struct ProjectContext {
    pub project_id: ProjectId,
    pub redactor: Arc<Redactor>,
    /// Key scopes: "ingest", "gateway" and/or "rum".
    pub scopes: Vec<String>,
    /// Tail-sampling policy of the project (rate 1.0 = keep everything).
    pub sampling: galileo_core::Sampling,
    /// Log pipeline and log-based metrics applied at ingest.
    pub pipeline: Arc<galileo_core::LogPipeline>,
    pub log_metrics: Arc<Vec<galileo_core::LogMetric>>,
    /// Ingest quotas per day.
    pub quotas: galileo_core::Quotas,
}

impl ProjectContext {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

impl std::fmt::Debug for ProjectContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProjectContext({})", self.project_id)
    }
}

/// Resolves a raw API key to a project. The production implementation lives in `galileo-api`
/// (Postgres + cache); this crate only needs the trait.
#[async_trait]
pub trait ApiKeyResolver: Send + Sync + 'static {
    async fn resolve(&self, raw_key: &str) -> Option<ProjectContext>;
}

pub type DynResolver = Arc<dyn ApiKeyResolver>;

/// Fixed key → project mapping. Used by tests and by single-user deployments that configure a
/// key statically.
pub struct StaticResolver {
    keys: HashMap<String, ProjectContext>,
}

impl StaticResolver {
    pub fn new(entries: impl IntoIterator<Item = (String, ProjectContext)>) -> Self {
        Self { keys: entries.into_iter().collect() }
    }
}

#[async_trait]
impl ApiKeyResolver for StaticResolver {
    async fn resolve(&self, raw_key: &str) -> Option<ProjectContext> {
        self.keys.get(raw_key).cloned()
    }
}
