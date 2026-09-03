use std::sync::Arc;

use galileo_core::Config;
use galileo_storage::DynStorage;
use sqlx::PgPool;

use crate::resolver::PgResolver;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pg: PgPool,
    pub storage: DynStorage,
    pub resolver: Arc<PgResolver>,
    pub secret: [u8; 32],
    /// Ingest counters, exposed at /api/system/stats.
    pub ingest_stats: Arc<galileo_otlp::IngestStats>,
    pub gateway: Arc<galileo_gateway::Gateway>,
    pub alerts: Arc<galileo_alerts::Evaluator>,
    /// Process start, for uptime on the health page.
    pub started_at: std::time::Instant,
}
