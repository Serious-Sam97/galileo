//! Route + provider configuration per project, cached briefly so the hot path never queries
//! Postgres.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

const TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    Openai,
    Ollama,
    OpenaiCompatible,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "anthropic" => ProviderKind::Anthropic,
            "openai" => ProviderKind::Openai,
            "ollama" => ProviderKind::Ollama,
            "openai_compatible" => ProviderKind::OpenaiCompatible,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Openai => "openai",
            ProviderKind::Ollama => "ollama",
            ProviderKind::OpenaiCompatible => "openai_compatible",
        }
    }
    /// The wire format the provider speaks.
    pub fn format(&self) -> crate::formats::Format {
        match self {
            ProviderKind::Anthropic => crate::formats::Format::Anthropic,
            _ => crate::formats::Format::Openai,
        }
    }
    /// gen_ai.system value.
    pub fn gen_ai_system(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Openai => "openai",
            ProviderKind::Ollama => "ollama",
            ProviderKind::OpenaiCompatible => "openai_compatible",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub id: Uuid,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    /// Decrypted at load time; lives only in memory.
    pub api_key: Option<String>,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub provider_id: Uuid,
    pub model: String,
    /// Optional price override, USD per 1M tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_output: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Budget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly_usd: Option<f64>,
    /// Exact-match response cache TTL for non-streaming calls (0/absent = off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_ttl_secs: Option<u64>,
    /// Who hears about 80% and exhausted budgets (same recipient shapes as triggers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert_recipients: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_content: Option<bool>,
    /// A/B test between two versions of a registered prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<Experiment>,
    /// Prompt guardrails (PII, injection, denylist, per-user caps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guardrails: Option<crate::guardrails::Guardrails>,
    /// Near-duplicate prompt cache on top of the exact-match cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_cache: Option<crate::semcache::SemanticCache>,
    /// Order targets by live health instead of the configured order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart_routing: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub name: String,
    pub prompt_name: String,
    pub version_a: i64,
    pub version_b: i64,
    /// 0–100 share of calls that get version B.
    pub percent_b: u8,
    /// Stick a user (x-galileo-user-id) to one arm.
    #[serde(default = "yes")]
    pub sticky: bool,
}
fn yes() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RateLimit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests_per_minute: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub id: Uuid,
    pub alias: String,
    pub targets: Vec<Target>,
    pub budget: Budget,
    pub rate_limit: RateLimit,
    pub enabled: bool,
    /// Record prompt/completion text on spans (redacted). Off for sensitive routes.
    pub record_content: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectRoutes {
    pub providers: Vec<Provider>,
    pub routes: Vec<Route>,
}

impl ProjectRoutes {
    pub fn route(&self, alias: &str) -> Option<&Route> {
        self.routes.iter().find(|r| r.alias == alias)
    }
    pub fn provider(&self, id: Uuid) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }
}

struct Entry {
    cfg: Arc<ProjectRoutes>,
    at: Instant,
}

pub struct RouteCache {
    pg: PgPool,
    cache: DashMap<Uuid, Entry>,
}

#[derive(sqlx::FromRow)]
struct ProviderRow {
    id: Uuid,
    name: String,
    kind: String,
    base_url: String,
    api_key_enc: Option<Vec<u8>>,
    headers: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct RouteRow {
    id: Uuid,
    alias: String,
    targets: serde_json::Value,
    budget: serde_json::Value,
    rate_limit: serde_json::Value,
    enabled: bool,
}

impl RouteCache {
    pub fn new(pg: PgPool) -> Self {
        Self { pg, cache: DashMap::new() }
    }

    pub fn invalidate_all(&self) {
        self.cache.clear();
    }

    pub async fn get(&self, project_id: Uuid, secret: &[u8; 32]) -> Arc<ProjectRoutes> {
        if let Some(e) = self.cache.get(&project_id) {
            if e.at.elapsed() < TTL {
                return e.cfg.clone();
            }
        }
        let cfg = Arc::new(self.load(project_id, secret).await.unwrap_or_default());
        self.cache.insert(project_id, Entry { cfg: cfg.clone(), at: Instant::now() });
        cfg
    }

    async fn load(&self, project_id: Uuid, secret: &[u8; 32]) -> sqlx::Result<ProjectRoutes> {
        let prows: Vec<ProviderRow> =
            sqlx::query_as("SELECT id, name, kind, base_url, api_key_enc, headers FROM gateway_providers WHERE project_id = $1")
                .bind(project_id)
                .fetch_all(&self.pg)
                .await?;
        let providers = prows
            .into_iter()
            .filter_map(|r| {
                Some(Provider {
                    id: r.id,
                    name: r.name,
                    kind: ProviderKind::parse(&r.kind)?,
                    base_url: r.base_url.trim_end_matches('/').to_string(),
                    api_key: r.api_key_enc.as_deref().and_then(|b| crate::crypto::decrypt(secret, b)),
                    headers: r
                        .headers
                        .as_object()
                        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
                        .unwrap_or_default(),
                })
            })
            .collect();
        let rrows: Vec<RouteRow> =
            sqlx::query_as("SELECT id, alias, targets, budget, rate_limit, enabled FROM gateway_routes WHERE project_id = $1")
                .bind(project_id)
                .fetch_all(&self.pg)
                .await?;
        let routes = rrows
            .into_iter()
            .map(|r| {
                let record_content = r.budget.get("record_content").and_then(|v| v.as_bool()).unwrap_or(true);
                Route {
                    id: r.id,
                    alias: r.alias,
                    targets: serde_json::from_value(r.targets).unwrap_or_default(),
                    budget: serde_json::from_value(r.budget).unwrap_or_default(),
                    rate_limit: serde_json::from_value(r.rate_limit).unwrap_or_default(),
                    enabled: r.enabled,
                    record_content,
                }
            })
            .collect();
        Ok(ProjectRoutes { providers, routes })
    }
}
