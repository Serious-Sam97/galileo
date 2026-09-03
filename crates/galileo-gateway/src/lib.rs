//! LLM gateway. Apps point their Anthropic or OpenAI SDK at `/gw` with a Galileo API key and
//! ask for a *route alias* as the model name. The gateway resolves the alias to a provider +
//! model, enforces rate limits and budgets, forwards the call (streaming or not), translates
//! between the Anthropic Messages and OpenAI Chat formats when the provider speaks the other
//! one, and records the whole thing as a `gen_ai.*` span in the caller's trace.

pub mod cache;
pub mod health;
pub mod semcache;
pub mod guardrails;
pub mod crypto;
pub mod formats;
pub mod limits;
pub mod pricing;
pub mod providers;
pub mod record;
pub mod routing;
pub mod server;

use std::sync::Arc;

use galileo_otlp::auth::DynResolver;
use galileo_otlp::WriterHandle;
use galileo_storage::DynStorage;
use sqlx::PgPool;

pub use server::router;

pub struct Gateway {
    pub pg: PgPool,
    pub storage: DynStorage,
    pub resolver: DynResolver,
    pub writer: WriterHandle,
    pub http: reqwest::Client,
    pub secret: [u8; 32],
    pub routes: routing::RouteCache,
    pub limits: limits::Limiter,
    pub cache: cache::ResponseCache,
    pub semcache: semcache::Store,
    pub health: health::Health,
}

impl Gateway {
    pub fn new(pg: PgPool, storage: DynStorage, resolver: DynResolver, writer: WriterHandle, secret: [u8; 32]) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            // Long generations are normal; the per-request timeout is enforced by upstream.
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .expect("reqwest client");
        Arc::new(Self {
            pg: pg.clone(),
            storage,
            resolver,
            writer,
            http,
            secret,
            routes: routing::RouteCache::new(pg),
            limits: limits::Limiter::default(),
            cache: cache::ResponseCache::default(),
            semcache: semcache::Store::default(),
            health: health::Health::default(),
        })
    }

    /// Call after providers/routes change so the next request sees them.
    pub fn invalidate(&self) {
        self.routes.invalidate_all();
    }
}

impl Gateway {
    /// One non-streaming chat call through a route, without recording a span. Used by the eval
    /// judge and other internal callers. Returns the text of the first choice.
    pub async fn simple_chat(&self, project: uuid::Uuid, alias: &str, system: Option<&str>, user: &str, max_tokens: u64) -> anyhow::Result<String> {
        let cfg = self.routes.get(project, &self.secret).await;
        let route = cfg.route(alias).ok_or_else(|| anyhow::anyhow!("unknown route '{alias}'"))?;
        let mut last_err = None;
        for t in &route.targets {
            let Some(p) = cfg.provider(t.provider_id) else { continue };
            let mut req = formats::NormRequest { model: t.model.clone(), system: system.map(str::to_owned), messages: vec![formats::NormMessage::text("user", user)], max_tokens: Some(max_tokens), temperature: Some(0.0), ..Default::default() };
            req.stream = false;
            let fmt = p.kind.format();
            let body = formats::build_request(fmt, &req, &t.model, providers::openai_strict(p.kind));
            let up = providers::build(p, body, &axum::http::HeaderMap::new());
            match self.http.post(&up.url).headers(up.headers).json(&up.body).send().await {
                Ok(r) if r.status().is_success() => {
                    let v: serde_json::Value = r.json().await?;
                    let mut text = formats::observe_response(fmt, &v).text;
                    if text.trim().is_empty() {
                        // thinking models may put the answer in `reasoning` when the content is empty
                        text = v.pointer("/choices/0/message/reasoning").or_else(|| v.pointer("/choices/0/message/reasoning_content")).and_then(|x| x.as_str()).unwrap_or("").to_string();
                        tracing::warn!(alias, model = %t.model, body = %v.to_string().chars().take(300).collect::<String>(), "simple_chat: empty completion");
                    }
                    return Ok(text);
                }
                Ok(r) => last_err = Some(anyhow::anyhow!("{} returned {}", p.name, r.status())),
                Err(e) => last_err = Some(e.into()),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("route has no reachable target")))
    }
}

/// Result of a recorded internal chat call.
#[derive(Debug, Clone)]
pub struct RecordedChat {
    pub text: String,
    pub span_id: String,
    pub trace_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub model: String,
}

impl Gateway {
    /// Like `simple_chat`, but records the call as a gen_ai span in `project` (route, tokens, cost,
    /// `gen_ai.galileo.caller = <caller>`), so internal users of the gateway — the assistant — show
    /// up in AI → Calls and can receive feedback. `messages` are (role, content) pairs; a "system"
    /// role becomes the system prompt.
    #[allow(clippy::too_many_arguments)]
    pub async fn chat_recorded(&self, project: uuid::Uuid, alias: &str, caller: &str, messages: &[(String, String)], max_tokens: u64, temperature: f64, user_id: Option<String>) -> anyhow::Result<RecordedChat> {
        use crate::record::{CallRecord, RecordGuard, TraceContext};
        use galileo_core::SpanId;
        let cfg = self.routes.get(project, &self.secret).await;
        let route = cfg.route(alias).ok_or_else(|| anyhow::anyhow!("unknown route '{alias}'"))?;
        let system: Option<String> = messages.iter().find(|(r, _)| r == "system").map(|(_, c)| c.clone());
        let msgs: Vec<formats::NormMessage> = messages.iter().filter(|(r, _)| r != "system").map(|(r, c)| formats::NormMessage::text(r, c)).collect();
        let prompt_text: String = messages.iter().map(|(r, c)| format!("[{r}] {c}\n")).collect();
        let redactor = self.resolver_redactor(project).await;
        let mut last_err = None;
        for (i, t) in route.targets.iter().enumerate() {
            let Some(p) = cfg.provider(t.provider_id) else { continue };
            let mut req = formats::NormRequest { model: t.model.clone(), system: system.clone(), messages: msgs.clone(), max_tokens: Some(max_tokens), temperature: Some(temperature), ..Default::default() };
            req.stream = false;
            let fmt = p.kind.format();
            let body = formats::build_request(fmt, &req, &t.model, providers::openai_strict(p.kind));
            let up = providers::build(p, body, &axum::http::HeaderMap::new());
            let mut record = CallRecord {
                operation: "chat", caller: Some(caller.to_string()), project_id: galileo_core::ProjectId(project), ctx: TraceContext::from_header(None), span_id: SpanId::random(),
                started: chrono::Utc::now(), started_at: std::time::Instant::now(), client_format: fmt, route_alias: route.alias.clone(),
                route_id: (route.id != uuid::Uuid::nil()).then_some(route.id), provider_name: p.name.clone(), provider_system: p.kind.gen_ai_system().into(),
                request_model: t.model.clone(), fallback_index: i as u32, streaming: false, max_tokens: Some(max_tokens), temperature: Some(temperature),
                prompt: if route.record_content { prompt_text.clone() } else { String::new() }, prompt_name: None, prompt_version: None, experiment: None, cache_hit: false, cache_kind: None, guardrail: None, routing: None,
                user_id: user_id.clone(), tenant_id: None, conversation_id: None, record_content: route.record_content, attempts: vec![],
                observed: formats::Observed::default(), cost_usd: 0.0, cost_known: false, http_status: 0, error: None, ttft_ms: None, done: false,
            };
            match self.http.post(&up.url).headers(up.headers).json(&up.body).send().await {
                Ok(r) if r.status().is_success() => {
                    let v: serde_json::Value = r.json().await?;
                    let mut observed = formats::observe_response(fmt, &v);
                    if observed.text.trim().is_empty() {
                        observed.text = v.pointer("/choices/0/message/reasoning").or_else(|| v.pointer("/choices/0/message/reasoning_content")).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    }
                    let price = match (t.price_input, t.price_output) { (Some(i), Some(o)) => (crate::pricing::Price::simple(i, o), true), _ => { let l = crate::pricing::lookup(&t.model); (l.price, l.known) } };
                    record.cost_usd = price.0.cost(observed.input_tokens, observed.output_tokens, observed.cache_read_tokens, observed.cache_write_tokens);
                    record.cost_known = price.1;
                    record.http_status = 200;
                    record.done = true;
                    let out = RecordedChat { text: observed.text.clone(), span_id: record.span_id.to_hex(), trace_id: record.ctx.trace_id.to_hex(), input_tokens: observed.input_tokens, output_tokens: observed.output_tokens, cost_usd: record.cost_usd, model: if observed.model.is_empty() { t.model.clone() } else { observed.model.clone() } };
                    record.observed = observed;
                    let mut guard = RecordGuard { record: Some(record), writer: self.writer.clone(), redactor };
                    guard.emit();
                    return Ok(out);
                }
                Ok(r) => {
                    let status = r.status();
                    record.http_status = status.as_u16(); record.error = Some(format!("upstream_{}", status.as_u16())); record.done = true;
                    let mut guard = RecordGuard { record: Some(record), writer: self.writer.clone(), redactor: redactor.clone() };
                    guard.emit();
                    last_err = Some(anyhow::anyhow!("{} returned {}", p.name, status));
                }
                Err(e) => {
                    record.error = Some("connection_error".into()); record.done = true;
                    let mut guard = RecordGuard { record: Some(record), writer: self.writer.clone(), redactor: redactor.clone() };
                    guard.emit();
                    last_err = Some(e.into());
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("route has no reachable target")))
    }

    /// Redactor for a project (redaction rules apply to recorded prompts); defaults when unknown.
    async fn resolver_redactor(&self, project: uuid::Uuid) -> std::sync::Arc<galileo_core::Redactor> {
        let rules: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT rule FROM redaction_rules WHERE project_id = $1 ORDER BY created_at").bind(project).fetch_all(&self.pg).await.unwrap_or_default();
        let parsed: Vec<galileo_core::RedactionRule> = rules.into_iter().filter_map(|(v,)| serde_json::from_value(v).ok()).collect();
        std::sync::Arc::new(galileo_core::Redactor::with_defaults(&parsed).unwrap_or_else(|_| galileo_core::Redactor::with_defaults(&[]).expect("defaults compile")))
    }
}
