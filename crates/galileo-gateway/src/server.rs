//! The request handler: auth → route → limits → prompt registry → upstream (with fallbacks)
//! → response (passthrough or translated, streaming or not) → span.

use std::sync::Arc;
use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use futures::StreamExt;
use galileo_core::{ProjectId, SpanId};
use galileo_otlp::extract_key;
use serde_json::{json, Value};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::formats::{self, Format, Observed, SseParser, StreamObserver, StreamTranslator};
use crate::providers;
use crate::record::{CallRecord, RecordGuard, TraceContext};
use crate::routing::{Provider, Route, Target};
use crate::Gateway;

pub fn router(gw: Arc<Gateway>) -> Router {
    Router::new()
        .route("/v1/messages", post(anthropic_messages))
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/models", get(list_models))
        .route("/v1/feedback", post(feedback))
        .route("/v1/embeddings", post(embeddings))
        .route("/v1/audio/transcriptions", post(transcriptions))
        .route("/healthz", get(|| async { "ok" }))
        .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(gw)
}

struct GwError {
    status: StatusCode,
    kind: &'static str,
    message: String,
}

fn gw_err(status: StatusCode, kind: &'static str, message: impl Into<String>) -> GwError {
    GwError { status, kind, message: message.into() }
}

fn error_response(format: Format, e: GwError) -> Response {
    let body = formats::error_body(format, e.status.as_u16(), e.kind, &e.message);
    (e.status, [(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

async fn anthropic_messages(State(gw): State<Arc<Gateway>>, headers: HeaderMap, body: Bytes) -> Response {
    handle(gw, Format::Anthropic, headers, body).await
}

async fn openai_chat(State(gw): State<Arc<Gateway>>, headers: HeaderMap, body: Bytes) -> Response {
    handle(gw, Format::Openai, headers, body).await
}

async fn authenticate(gw: &Gateway, headers: &HeaderMap) -> Result<galileo_otlp::ProjectContext, GwError> {
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok());
    let raw = h("x-galileo-key")
        .map(str::to_owned)
        .or_else(|| h("x-api-key").map(str::to_owned))
        .or_else(|| extract_key(h("authorization"), None))
        .ok_or_else(|| gw_err(StatusCode::UNAUTHORIZED, "authentication_error", "missing API key: send x-api-key or Authorization: Bearer <galileo key>"))?;
    match gw.resolver.resolve(&raw).await {
        Some(ctx) if ctx.has_scope("gateway") => Ok(ctx),
        Some(_) => Err(gw_err(StatusCode::FORBIDDEN, "permission_error", "API key lacks the 'gateway' scope")),
        None => Err(gw_err(StatusCode::UNAUTHORIZED, "authentication_error", "invalid API key")),
    }
}

/// OpenAI-style model list: the project's route aliases.
async fn list_models(State(gw): State<Arc<Gateway>>, headers: HeaderMap) -> Response {
    let ctx = match authenticate(&gw, &headers).await {
        Ok(c) => c,
        Err(e) => return error_response(Format::Openai, e),
    };
    let cfg = gw.routes.get(ctx.project_id.0, &gw.secret).await;
    let data: Vec<Value> = cfg
        .routes
        .iter()
        .filter(|r| r.enabled)
        .map(|r| json!({ "id": r.alias, "object": "model", "owned_by": "galileo", "created": 0 }))
        .collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

struct Resolved {
    route: Route,
    targets: Vec<(Provider, Target)>,
}

fn resolve_route(cfg: &crate::routing::ProjectRoutes, alias: &str) -> Result<Resolved, GwError> {
    if let Some(r) = cfg.route(alias) {
        if !r.enabled {
            return Err(gw_err(StatusCode::FORBIDDEN, "permission_error", format!("route '{alias}' is disabled")));
        }
        let targets: Vec<(Provider, Target)> = r
            .targets
            .iter()
            .filter_map(|t| cfg.provider(t.provider_id).map(|p| (p.clone(), t.clone())))
            .collect();
        if targets.is_empty() {
            return Err(gw_err(StatusCode::BAD_GATEWAY, "api_error", format!("route '{alias}' has no valid targets")));
        }
        return Ok(Resolved { route: r.clone(), targets });
    }
    // Direct addressing: "<provider name>/<model>" bypasses routes (still recorded/limited by project).
    if let Some((pname, model)) = alias.split_once('/') {
        if let Some(p) = cfg.providers.iter().find(|p| p.name == pname) {
            let route = Route {
                id: Uuid::nil(),
                alias: alias.to_string(),
                targets: vec![],
                budget: Default::default(),
                rate_limit: Default::default(),
                enabled: true,
                record_content: true,
            };
            return Ok(Resolved { route, targets: vec![(p.clone(), Target { provider_id: p.id, model: model.to_string(), price_input: None, price_output: None })] });
        }
    }
    Err(gw_err(
        StatusCode::NOT_FOUND,
        "not_found_error",
        format!("unknown model '{alias}': create a route with that alias in Galileo, or address a provider directly as '<provider>/<model>'"),
    ))
}

#[derive(Default)]
struct PromptRef {
    name: String,
    version: Option<i64>,
    variables: serde_json::Map<String, Value>,
}

/// `{"galileo": {"prompt": {"name": "...", "version": 3, "variables": {...}}}}` in the body asks
/// the gateway to inject a registered prompt. The whole `galileo` object is stripped upstream.
fn take_galileo_ext(body: &mut Value) -> Option<PromptRef> {
    let ext = body.as_object_mut()?.remove("galileo")?;
    let p = ext.get("prompt")?;
    Some(PromptRef {
        name: p.get("name")?.as_str()?.to_string(),
        version: p.get("version").and_then(|v| v.as_i64()),
        variables: p.get("variables").and_then(|v| v.as_object()).cloned().unwrap_or_default(),
    })
}

fn render_template(t: &str, vars: &serde_json::Map<String, Value>) -> String {
    let mut out = t.to_string();
    for (k, v) in vars {
        let val = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        out = out.replace(&format!("{{{{{k}}}}}"), &val);
    }
    out
}

/// Pick the prompt version for an experiment arm. Sticky by user id when present.
pub fn experiment_arm(exp: &crate::routing::Experiment, user: Option<&str>) -> (i64, &'static str) {
    let roll: u8 = match (exp.sticky, user) {
        (true, Some(u)) if !u.is_empty() => {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in exp.name.bytes().chain(u.bytes()) { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); }
            (h % 100) as u8
        }
        _ => (uuid::Uuid::new_v4().as_u128() % 100) as u8,
    };
    if roll < exp.percent_b.min(100) { (exp.version_b, "b") } else { (exp.version_a, "a") }
}

async fn apply_prompt(gw: &Gateway, project: Uuid, pr: &PromptRef, format: Format, body: &mut Value) -> Result<i64, GwError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        version: i32,
        content: Value,
    }
    let row: Option<Row> = if let Some(v) = pr.version {
        sqlx::query_as(
            "SELECT pv.version, pv.content FROM prompt_versions pv JOIN prompts p ON p.id = pv.prompt_id \
             WHERE p.project_id = $1 AND p.name = $2 AND pv.version = $3",
        )
        .bind(project)
        .bind(&pr.name)
        .bind(v as i32)
        .fetch_optional(&gw.pg)
        .await
    } else {
        sqlx::query_as(
            "SELECT pv.version, pv.content FROM prompt_versions pv JOIN prompts p ON p.id = pv.prompt_id \
             WHERE p.project_id = $1 AND p.name = $2 AND (p.promoted_version IS NULL OR pv.version = p.promoted_version) ORDER BY pv.version DESC LIMIT 1",
        )
        .bind(project)
        .bind(&pr.name)
        .fetch_optional(&gw.pg)
        .await
    }
    .map_err(|e| gw_err(StatusCode::INTERNAL_SERVER_ERROR, "api_error", format!("prompt lookup failed: {e}")))?;
    let row = row.ok_or_else(|| gw_err(StatusCode::NOT_FOUND, "not_found_error", format!("prompt '{}' not found", pr.name)))?;
    let system = row.content.get("system").and_then(|s| s.as_str()).map(|s| render_template(s, &pr.variables));
    let msgs: Vec<Value> = row
        .content
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .map(|m| {
                    let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let content = m.get("content").and_then(|c| c.as_str()).map(|c| render_template(c, &pr.variables)).unwrap_or_default();
                    json!({ "role": role, "content": content })
                })
                .collect()
        })
        .unwrap_or_default();
    let obj = body.as_object_mut().unwrap();
    match format {
        Format::Anthropic => {
            if let Some(s) = system {
                obj.insert("system".into(), json!(s));
            }
            let mut all = msgs;
            all.extend(obj.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default());
            obj.insert("messages".into(), Value::Array(all));
        }
        Format::Openai => {
            let mut all = Vec::new();
            if let Some(s) = system {
                all.push(json!({ "role": "system", "content": s }));
            }
            all.extend(msgs);
            all.extend(obj.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default());
            obj.insert("messages".into(), Value::Array(all));
        }
    }
    Ok(row.version as i64)
}

/// Rejections before any upstream call (unknown route, rate limit, budget) still produce a span,
/// otherwise the app sees 429s that Galileo cannot explain.
#[allow(clippy::too_many_arguments)]
fn record_rejection(gw: &Gateway, ctx: &galileo_otlp::ProjectContext, headers: &HeaderMap, client_format: Format, alias: &str, e: &GwError, kind: &str, body: Option<&Value>) {
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let (prompt, max_tokens, temperature, streaming) = match body {
        Some(b) => {
            let n = formats::parse_request(client_format, b);
            (formats::prompt_text(&n), n.max_tokens, n.temperature, n.stream)
        }
        None => (String::new(), None, None, false),
    };
    let record = CallRecord { operation: "chat", caller: None,
        project_id: ctx.project_id,
        ctx: TraceContext::from_header(h("traceparent").as_deref()),
        span_id: SpanId::random(),
        started: Utc::now(),
        started_at: Instant::now(),
        client_format,
        route_alias: alias.to_string(),
        route_id: None,
        provider_name: String::new(),
        provider_system: "galileo".into(),
        request_model: alias.to_string(),
        fallback_index: 0,
        streaming,
        max_tokens,
        temperature,
        prompt,
        prompt_name: None,
        prompt_version: None,
        experiment: None,
        cache_hit: false, cache_kind: None, guardrail: None, routing: None,
        user_id: h("x-galileo-user-id"),
        tenant_id: h("x-galileo-tenant-id"),
        conversation_id: h("x-galileo-session-id").or_else(|| h("x-galileo-conversation-id")),
        record_content: true,
        attempts: vec![e.message.clone()],
        observed: Observed::default(),
        cost_usd: 0.0,
        cost_known: true,
        http_status: e.status.as_u16(),
        error: Some(kind.to_string()),
        ttft_ms: None,
        done: true,
    };
    let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
    guard.emit();
}

async fn handle(gw: Arc<Gateway>, client_format: Format, headers: HeaderMap, raw: Bytes) -> Response {
    match handle_inner(gw, client_format, headers, raw).await {
        Ok(r) => r,
        Err(e) => error_response(client_format, e),
    }
}

async fn handle_inner(gw: Arc<Gateway>, client_format: Format, headers: HeaderMap, raw: Bytes) -> Result<Response, GwError> {
    let started_at = Instant::now();
    let started = Utc::now();
    let ctx = authenticate(&gw, &headers).await?;
    let project = ctx.project_id;

    let mut body: Value = serde_json::from_slice(&raw).map_err(|e| gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", format!("invalid JSON body: {e}")))?;
    if !body.is_object() {
        return Err(gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", "body must be a JSON object"));
    }
    let alias = body.get("model").and_then(|m| m.as_str()).unwrap_or("").trim().to_string();
    if alias.is_empty() {
        return Err(gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", "'model' is required"));
    }
    let streaming = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    let cfg = gw.routes.get(project.0, &gw.secret).await;
    let resolved = match resolve_route(&cfg, &alias) {
        Ok(r) => r,
        Err(e) => {
            record_rejection(&gw, &ctx, &headers, client_format, &alias, &e, "unknown_route", Some(&body));
            return Err(e);
        }
    };
    let route = resolved.route.clone();
    let route = &route;

    // --- limits -------------------------------------------------------------------------
    let reject = |e: GwError, kind: &str| {
        record_rejection(&gw, &ctx, &headers, client_format, &alias, &e, kind, Some(&body));
        e
    };
    if let Some(rpm) = route.rate_limit.requests_per_minute {
        if !gw.limits.allow(project.0, route.id, rpm) {
            return Err(reject(gw_err(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", format!("route '{alias}' exceeded {rpm} requests/minute")), "rate_limited"));
        }
    }
    let b = &route.budget;
    if b.daily_usd.is_some() || b.daily_tokens.is_some() || b.monthly_usd.is_some() {
        let spend = gw.limits.spend(&gw.storage, project, route.id, &route.alias).await;
        if let Some(cap) = b.daily_usd {
            if spend.day_usd >= cap {
                return Err(reject(gw_err(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", format!("route '{alias}' daily budget of ${cap:.2} exhausted (${:.4} spent)", spend.day_usd)), "budget_exceeded"));
            }
        }
        if let Some(cap) = b.daily_tokens {
            if spend.day_tokens >= cap {
                return Err(reject(gw_err(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", format!("route '{alias}' daily token budget of {cap} exhausted ({} used)", spend.day_tokens)), "budget_exceeded"));
            }
        }
        if let Some(cap) = b.monthly_usd {
            if spend.month_usd >= cap {
                return Err(reject(gw_err(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", format!("route '{alias}' monthly budget of ${cap:.2} exhausted (${:.4} spent)", spend.month_usd)), "budget_exceeded"));
            }
        }
    }

    // --- prompt registry (+ experiments) --------------------------------------------------
    let mut prompt_name = None;
    let mut prompt_version = None;
    let mut experiment: Option<(String, &'static str)> = None;
    if let Some(mut pr) = take_galileo_ext(&mut body) {
        if pr.version.is_none() {
            if let Some(exp) = route.budget.experiment.as_ref().filter(|e| e.prompt_name == pr.name) {
                let user_hdr = headers.get("x-galileo-user-id").and_then(|v| v.to_str().ok());
                let (v, arm) = experiment_arm(exp, user_hdr);
                pr.version = Some(v);
                experiment = Some((exp.name.clone(), arm));
            }
        }
        let v = apply_prompt(&gw, project.0, &pr, client_format, &mut body).await?;
        prompt_name = Some(pr.name);
        prompt_version = Some(v);
    }

    // --- guardrails ---------------------------------------------------------------------------
    let mut guardrail_hit: Option<(String, String)> = None;
    if let Some(gcfg) = route.budget.guardrails.as_ref() {
        let pre = formats::parse_request(client_format, &body);
        let user_text: String = pre.messages.iter().filter(|m| m.role == "user").map(|m| m.content()).collect::<Vec<_>>().join("\n");
        let hits = crate::guardrails::check(gcfg, &user_text);
        if let Some(block) = hits.iter().find(|h| h.action == crate::guardrails::Action::Block) {
            let e = gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", format!("request blocked by guardrail '{}' ({})", block.kind, block.detail));
            let kind = format!("guardrail_{}", block.kind);
            record_rejection(&gw, &ctx, &headers, client_format, &alias, &e, &kind, Some(&body));
            return Err(e);
        }
        if hits.iter().any(|h| h.kind == "pii" && h.action == crate::guardrails::Action::Redact) {
            crate::guardrails::redact_body(&mut body);
        }
        if let Some(first) = hits.first() {
            guardrail_hit = Some((hits.iter().map(|h| h.kind).collect::<Vec<_>>().join(","), format!("{:?}", first.action).to_lowercase()));
        }
        // per-user daily caps
        if let Some(user) = headers.get("x-galileo-user-id").and_then(|v| v.to_str().ok()).filter(|u| !u.is_empty()) {
            if gcfg.max_tokens_per_user_day.is_some() || gcfg.max_cost_per_user_day.is_some() {
                let (usd, tokens) = user_spend_today(&gw, project, &route.alias, user).await;
                if gcfg.max_cost_per_user_day.map(|cap| usd >= cap).unwrap_or(false) || gcfg.max_tokens_per_user_day.map(|cap| tokens >= cap).unwrap_or(false) {
                    let e = gw_err(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", format!("daily limit for user '{user}' on route '{alias}' reached (${usd:.4}, {tokens} tokens)"));
                    record_rejection(&gw, &ctx, &headers, client_format, &alias, &e, "guardrail_user_cap", Some(&body));
                    return Err(e);
                }
            }
        }
    }
    // --- smart routing: reorder targets by live health ---------------------------------------
    let mut resolved = resolved;
    let mut routing_attr: Option<&'static str> = None;
    if route.budget.smart_routing.unwrap_or(false) && resolved.targets.len() > 1 {
        let models: Vec<String> = resolved.targets.iter().map(|(_, t)| t.model.clone()).collect();
        let order = gw.health.order(project.0, route.id, &models);
        if order.iter().enumerate().any(|(i, &j)| i != j) { routing_attr = Some("health"); }
        resolved.targets = order.into_iter().map(|i| resolved.targets[i].clone()).collect();
    }
    let route = &resolved.route;

    let norm = formats::parse_request(client_format, &body);
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let trace_ctx = TraceContext::from_header(h("traceparent").as_deref());
    let span_id = SpanId::random();

    let mut record = CallRecord { operation: "chat", caller: None,
        project_id: project,
        ctx: trace_ctx.clone(),
        span_id,
        started,
        started_at,
        client_format,
        route_alias: route.alias.clone(),
        route_id: (route.id != Uuid::nil()).then_some(route.id),
        provider_name: String::new(),
        provider_system: String::new(),
        request_model: String::new(),
        fallback_index: 0,
        streaming,
        max_tokens: norm.max_tokens,
        temperature: norm.temperature,
        prompt: formats::prompt_text(&norm),
        prompt_name,
        prompt_version,
        experiment: experiment.clone(),
        cache_hit: false, cache_kind: None, guardrail: guardrail_hit.clone(), routing: routing_attr,
        user_id: h("x-galileo-user-id"),
        tenant_id: h("x-galileo-tenant-id"),
        conversation_id: h("x-galileo-session-id").or_else(|| h("x-galileo-conversation-id")),
        record_content: route.record_content,
        attempts: vec![],
        observed: Observed::default(),
        cost_usd: 0.0,
        cost_known: false,
        http_status: 0,
        error: None,
        ttft_ms: None,
        done: false,
    };

    // --- upstream with fallbacks ----------------------------------------------------------
    let mut last_err: Option<GwError> = None;
    for (i, (provider, target)) in resolved.targets.iter().enumerate() {
        let pformat = provider.kind.format();
        record.fallback_index = i as u32;
        record.provider_name = provider.name.clone();
        record.provider_system = provider.kind.gen_ai_system().into();
        record.request_model = target.model.clone();

        let upstream_body = if pformat == client_format {
            let mut b = body.clone();
            b["model"] = json!(target.model);
            if streaming && pformat == Format::Openai {
                b["stream_options"] = json!({ "include_usage": true });
            }
            b
        } else {
            formats::build_request(pformat, &norm, &target.model, providers::openai_strict(provider.kind))
        };
        // --- response cache (non-streaming only) ----------------------------------------
        let cache_ttl = route.budget.cache_ttl_secs.unwrap_or(0);
        let cache_key = (!streaming && cache_ttl > 0).then(|| crate::cache::key(&upstream_body));
        if let Some(k) = cache_key {
            if let Some((cached_upstream, observed)) = gw.cache.get(project.0, k) {
                let out_body = if pformat == client_format { cached_upstream } else { formats::build_response(client_format, &observed, pformat) };
                record.observed = observed;
                record.http_status = 200;
                record.cache_hit = true;
                record.cache_kind = Some("exact");
                record.cost_usd = 0.0;
                record.cost_known = true;
                record.done = true;
                let tp = trace_ctx.traceparent(span_id);
                let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
                guard.emit();
                let mut resp = (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], out_body.to_string()).into_response();
                for (k2, v) in [("x-galileo-cache", "hit".to_string()), ("x-galileo-trace-id", trace_ctx.trace_id.to_hex()), ("x-galileo-route", route.alias.clone()), ("traceparent", tp)] {
                    if let Ok(hv) = HeaderValue::from_str(&v) { resp.headers_mut().insert(k2, hv); }
                }
                return Ok(resp);
            }
        }
        // --- semantic cache (non-streaming only) -------------------------------------------
        let sem = route.budget.semantic_cache.as_ref().filter(|_| !streaming).map(|_| { let (b, t) = crate::semcache::bucket_and_text(&norm); (b, crate::semcache::embed(&t)) });
        if let (Some(sc), Some((bucket, vec))) = (route.budget.semantic_cache.as_ref(), sem.as_ref()) {
            if let Some((cached_upstream, observed, sim)) = gw.semcache.get(project.0, route.id, *bucket, vec, sc.threshold) {
                let out_body = if pformat == client_format { cached_upstream } else { formats::build_response(client_format, &observed, pformat) };
                record.observed = observed;
                record.http_status = 200;
                record.cache_hit = true;
                record.cache_kind = Some("semantic");
                record.cost_usd = 0.0;
                record.cost_known = true;
                record.done = true;
                let tp = trace_ctx.traceparent(span_id);
                let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
                guard.emit();
                let mut resp = (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], out_body.to_string()).into_response();
                for (k2, v) in [("x-galileo-cache", "semantic".to_string()), ("x-galileo-cache-similarity", format!("{sim:.3}")), ("x-galileo-trace-id", trace_ctx.trace_id.to_hex()), ("x-galileo-route", route.alias.clone()), ("traceparent", tp)] {
                    if let Ok(hv) = HeaderValue::from_str(&v) { resp.headers_mut().insert(k2, hv); }
                }
                return Ok(resp);
            }
        }
        let up = providers::build(provider, upstream_body.clone(), &headers);
        debug!(url = %up.url, alias, model = %target.model, attempt = i, "gateway upstream");

        let resp = match gw.http.post(&up.url).headers(up.headers).json(&up.body).send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("{}: connection error: {e}", provider.name);
                warn!(%msg, "upstream failed");
                gw.health.record(project.0, route.id, &target.model, false, started_at.elapsed().as_secs_f64() * 1000.0);
                record.attempts.push(msg.clone());
                last_err = Some(gw_err(StatusCode::BAD_GATEWAY, "api_error", msg));
                continue;
            }
        };
        let status = resp.status().as_u16();
        if status >= 400 {
            gw.health.record(project.0, route.id, &target.model, false, started_at.elapsed().as_secs_f64() * 1000.0);
            let text = resp.text().await.unwrap_or_default();
            let msg = format!("{} returned {status}: {}", provider.name, formats::cap(&text));
            if providers::is_retryable_status(status) && i + 1 < resolved.targets.len() {
                warn!(%msg, "upstream failed, trying next target");
                record.attempts.push(msg);
                continue;
            }
            // Final failure: record and pass the upstream error through in the client's format.
            record.http_status = status;
            record.error = Some(format!("upstream_{status}"));
            record.done = true;
            let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
            guard.emit();
            let body = if pformat == client_format {
                serde_json::from_str::<Value>(&text).unwrap_or_else(|_| formats::error_body(client_format, status, "api_error", &text))
            } else {
                formats::error_body(client_format, status, "api_error", &msg)
            };
            return Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), [(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response());
        }

        record.http_status = status;
        let price = match (target.price_input, target.price_output) {
            (Some(i), Some(o)) => (crate::pricing::Price::simple(i, o), true),
            _ => {
                let l = crate::pricing::lookup(&target.model);
                (l.price, l.known)
            }
        };
        let tp = trace_ctx.traceparent(span_id);
        let common_headers = [
            ("x-galileo-trace-id", trace_ctx.trace_id.to_hex()),
            ("x-galileo-span-id", span_id.to_hex()),
            ("x-galileo-route", route.alias.clone()),
            ("x-galileo-provider", provider.name.clone()),
            ("x-galileo-model", target.model.clone()),
            ("traceparent", tp),
        ];

        if streaming {
            return Ok(stream_response(gw.clone(), ctx.redactor.clone(), record, resp, pformat, client_format, price, &common_headers, route.id, project));
        }

        // non-streaming
        let text = resp.text().await.map_err(|e| gw_err(StatusCode::BAD_GATEWAY, "api_error", format!("reading upstream body: {e}")))?;
        let upstream_json: Value = serde_json::from_str(&text).map_err(|e| gw_err(StatusCode::BAD_GATEWAY, "api_error", format!("upstream sent invalid JSON: {e}")))?;
        let observed = formats::observe_response(pformat, &upstream_json);
        gw.health.record(project.0, route.id, &target.model, true, started_at.elapsed().as_secs_f64() * 1000.0);
        if let Some(k) = cache_key {
            gw.cache.put(project.0, k, upstream_json.clone(), observed.clone(), std::time::Duration::from_secs(cache_ttl));
        }
        if let (Some(sc), Some((bucket, vec))) = (route.budget.semantic_cache.as_ref(), sem) {
            gw.semcache.put(project.0, route.id, bucket, vec, upstream_json.clone(), observed.clone(), std::time::Duration::from_secs(sc.ttl_secs.max(1)));
        }
        let out_body = if pformat == client_format { upstream_json } else { formats::build_response(client_format, &observed, pformat) };
        finalize(&gw, &mut record, observed, price, route.id, project);
        budget_watch(&gw, project.0, route).await;
        let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
        guard.emit();
        let mut resp = (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], out_body.to_string()).into_response();
        for (k, v) in &common_headers {
            if let Ok(hv) = HeaderValue::from_str(v) {
                resp.headers_mut().insert(*k, hv);
            }
        }
        if cache_key.is_some() || route.budget.semantic_cache.is_some() { resp.headers_mut().insert("x-galileo-cache", HeaderValue::from_static("miss")); }
        return Ok(resp);
    }
    // every target failed
    let e = last_err.unwrap_or_else(|| gw_err(StatusCode::BAD_GATEWAY, "api_error", "all targets failed"));
    record.http_status = e.status.as_u16();
    record.error = Some("all_targets_failed".into());
    record.done = true;
    let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
    guard.emit();
    Err(e)
}

/// Today's spend for one user on a route, from recorded spans.
async fn user_spend_today(gw: &Gateway, project: ProjectId, alias: &str, user: &str) -> (f64, u64) {
    let q = galileo_storage::SqlQuery {
        sql: "SELECT sum(gen_ai_cost_usd), sum(gen_ai_input_tokens + gen_ai_output_tokens) FROM spans WHERE project_id = ? AND timestamp >= toStartOfDay(now()) AND gen_ai_system != '' AND attrs['gen_ai.galileo.route'] = ? AND user_id = ?".into(),
        params: vec![project.into(), alias.to_string().into(), user.to_string().into()],
    };
    match gw.storage.query(&q).await {
        Ok(r) => { let n = |i: usize| r.rows.first().and_then(|row| row.get(i)).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0); (n(0), n(1) as u64) }
        Err(_) => (0.0, 0),
    }
}

/// After a paid call: write a budget_events row once per period when 80% or 100% of a USD
/// budget is reached. The alert evaluator turns rows into notifications.
async fn budget_watch(gw: &Gateway, project: Uuid, route: &Route) {
    let b = &route.budget;
    if b.daily_usd.is_none() && b.monthly_usd.is_none() { return; }
    let spend = gw.limits.spend(&gw.storage, ProjectId(project), route.id, &route.alias).await;
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let month = Utc::now().format("%Y-%m").to_string();
    for (cap, spent, period) in [(b.daily_usd, spend.day_usd, format!("day:{today}")), (b.monthly_usd, spend.month_usd, format!("month:{month}"))] {
        let Some(cap) = cap else { continue };
        if cap <= 0.0 { continue; }
        let kind = if spent >= cap { "exhausted" } else if spent >= cap * 0.8 { "warn80" } else { continue };
        let _ = sqlx::query("INSERT INTO budget_events (id, project_id, route_alias, kind, period, spent_usd, cap_usd) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING")
            .bind(Uuid::now_v7()).bind(project).bind(&route.alias).bind(kind).bind(&period).bind(spent).bind(cap).execute(&gw.pg).await;
    }
}

#[derive(serde::Deserialize)]
struct FeedbackBody { span_id: String, #[serde(default)] trace_id: String, rating: i16, #[serde(default)] comment: String, #[serde(default)] user_id: String }

/// End-user feedback on a gateway response: `{span_id, rating (-1|1 or 1..5), comment?}`.
async fn feedback(State(gw): State<Arc<Gateway>>, headers: HeaderMap, Json(b): Json<FeedbackBody>) -> Response {
    let ctx = match authenticate(&gw, &headers).await { Ok(c) => c, Err(e) => return error_response(Format::Openai, e) };
    if !(-1..=5).contains(&b.rating) || b.rating == 0 { return error_response(Format::Openai, gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", "rating must be -1, 1 or 1..5")); }
    let user = if b.user_id.is_empty() { headers.get("x-galileo-user-id").and_then(|v| v.to_str().ok()).unwrap_or("").to_string() } else { b.user_id.clone() };
    let res = sqlx::query("INSERT INTO gateway_feedback (id, project_id, span_id, trace_id, rating, comment, user_id) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(Uuid::now_v7()).bind(ctx.project_id.0).bind(&b.span_id).bind(&b.trace_id).bind(b.rating).bind(b.comment.chars().take(2000).collect::<String>()).bind(user).execute(&gw.pg).await;
    match res { Ok(_) => Json(json!({ "ok": true })).into_response(), Err(e) => error_response(Format::Openai, gw_err(StatusCode::INTERNAL_SERVER_ERROR, "api_error", e.to_string())) }
}

/// Simple passthrough with recording: resolve `model` like chat, forward JSON, record usage.
async fn passthrough_json(gw: Arc<Gateway>, headers: HeaderMap, raw: Bytes, path: &str, operation: &'static str) -> Result<Response, GwError> {
    let started = Utc::now(); let started_at = Instant::now();
    let ctx = authenticate(&gw, &headers).await?;
    let project = ctx.project_id;
    let mut body: Value = serde_json::from_slice(&raw).map_err(|e| gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", format!("invalid JSON body: {e}")))?;
    let alias = body.get("model").and_then(|m| m.as_str()).unwrap_or("").trim().to_string();
    let cfg = gw.routes.get(project.0, &gw.secret).await;
    let resolved = resolve_route(&cfg, &alias)?;
    let (provider, target) = resolved.targets.first().cloned().ok_or_else(|| gw_err(StatusCode::BAD_GATEWAY, "api_error", "route has no targets"))?;
    body["model"] = json!(target.model);
    let base = provider.base_url.trim_end_matches('/'); let base = base.strip_suffix("/v1").unwrap_or(base);
    let mut req = gw.http.post(format!("{base}{path}")).json(&body);
    if let Some(k) = &provider.api_key { req = req.bearer_auth(k); }
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let trace_ctx = TraceContext::from_header(h("traceparent").as_deref());
    let mut record = CallRecord { operation, caller: None, project_id: project, ctx: trace_ctx.clone(), span_id: SpanId::random(), started, started_at, client_format: Format::Openai, route_alias: resolved.route.alias.clone(), route_id: (resolved.route.id != Uuid::nil()).then_some(resolved.route.id), provider_name: provider.name.clone(), provider_system: provider.kind.gen_ai_system().into(), request_model: target.model.clone(), fallback_index: 0, streaming: false, max_tokens: None, temperature: None, prompt: String::new(), prompt_name: None, prompt_version: None, experiment: None, cache_hit: false, cache_kind: None, guardrail: None, routing: None, user_id: h("x-galileo-user-id"), tenant_id: h("x-galileo-tenant-id"), conversation_id: None, record_content: false, attempts: vec![], observed: Observed::default(), cost_usd: 0.0, cost_known: false, http_status: 0, error: None, ttft_ms: None, done: false };
    record.observed.stop_reason = operation.to_string();
    let resp = req.send().await.map_err(|e| gw_err(StatusCode::BAD_GATEWAY, "api_error", format!("{}: {e}", provider.name)))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    record.http_status = status;
    if status >= 400 { record.error = Some(format!("upstream_{status}")); }
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        let u = v.get("usage").cloned().unwrap_or(json!({}));
        record.observed.input_tokens = u.get("prompt_tokens").and_then(|x| x.as_u64()).or_else(|| u.get("total_tokens").and_then(|x| x.as_u64())).unwrap_or(0);
        record.observed.model = v.get("model").and_then(|m| m.as_str()).unwrap_or(&target.model).to_string();
        let l = crate::pricing::lookup(&target.model);
        record.cost_usd = l.price.cost(record.observed.input_tokens, 0, 0, 0);
        record.cost_known = l.known;
        if operation == "embeddings" { record.observed.text = format!("{} vectors", v.get("data").and_then(|d| d.as_array()).map(|a| a.len()).unwrap_or(0)); }
    }
    record.done = true;
    let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
    guard.emit();
    Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), [(header::CONTENT_TYPE, "application/json")], text).into_response())
}

async fn embeddings(State(gw): State<Arc<Gateway>>, headers: HeaderMap, raw: Bytes) -> Response {
    match passthrough_json(gw, headers, raw, "/v1/embeddings", "embeddings").await { Ok(r) => r, Err(e) => error_response(Format::Openai, e) }
}

/// Multipart passthrough for Whisper-style transcription. `model` field = route alias.
async fn transcriptions(State(gw): State<Arc<Gateway>>, headers: HeaderMap, raw: Bytes) -> Response {
    match transcriptions_inner(gw, headers, raw).await { Ok(r) => r, Err(e) => error_response(Format::Openai, e) }
}

async fn transcriptions_inner(gw: Arc<Gateway>, headers: HeaderMap, raw: Bytes) -> Result<Response, GwError> {
    let started = Utc::now(); let started_at = Instant::now();
    let ctx = authenticate(&gw, &headers).await?;
    let project = ctx.project_id;
    let ct = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let boundary = ct.split("boundary=").nth(1).map(|b| b.trim_matches('"').to_string()).ok_or_else(|| gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", "multipart/form-data with a boundary is required"))?;
    // Find the `model` field without re-encoding the body: scan the parts textually.
    let alias = multipart_field(&raw, &boundary, "model").unwrap_or_default();
    if alias.is_empty() { return Err(gw_err(StatusCode::BAD_REQUEST, "invalid_request_error", "'model' form field is required (route alias)")); }
    let cfg = gw.routes.get(project.0, &gw.secret).await;
    let resolved = resolve_route(&cfg, &alias)?;
    let (provider, target) = resolved.targets.first().cloned().ok_or_else(|| gw_err(StatusCode::BAD_GATEWAY, "api_error", "route has no targets"))?;
    // Replace the model value in the raw multipart body.
    let body = replace_multipart_field(&raw, &boundary, "model", &target.model);
    let base = provider.base_url.trim_end_matches('/'); let base = base.strip_suffix("/v1").unwrap_or(base);
    let mut req = gw.http.post(format!("{base}/v1/audio/transcriptions")).header(header::CONTENT_TYPE, ct.clone()).body(body);
    if let Some(k) = &provider.api_key { req = req.bearer_auth(k); }
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let trace_ctx = TraceContext::from_header(h("traceparent").as_deref());
    let mut record = CallRecord { operation: "transcription", caller: None, project_id: project, ctx: trace_ctx, span_id: SpanId::random(), started, started_at, client_format: Format::Openai, route_alias: resolved.route.alias.clone(), route_id: (resolved.route.id != Uuid::nil()).then_some(resolved.route.id), provider_name: provider.name.clone(), provider_system: provider.kind.gen_ai_system().into(), request_model: target.model.clone(), fallback_index: 0, streaming: false, max_tokens: None, temperature: None, prompt: format!("[audio {} bytes]", raw.len()), prompt_name: None, prompt_version: None, experiment: None, cache_hit: false, cache_kind: None, guardrail: None, routing: None, user_id: h("x-galileo-user-id"), tenant_id: h("x-galileo-tenant-id"), conversation_id: None, record_content: resolved.route.record_content, attempts: vec![], observed: Observed::default(), cost_usd: 0.0, cost_known: false, http_status: 0, error: None, ttft_ms: None, done: false };
    record.observed.stop_reason = "transcribe".into();
    let resp = req.send().await.map_err(|e| gw_err(StatusCode::BAD_GATEWAY, "api_error", format!("{}: {e}", provider.name)))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    record.http_status = status;
    if status >= 400 { record.error = Some(format!("upstream_{status}")); }
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        let transcript = v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let duration_s = v.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0);
        record.observed.text = transcript;
        record.observed.model = target.model.clone();
        if let Some(ppm) = crate::pricing::audio_price_per_minute(&target.model) { record.cost_usd = ppm * duration_s / 60.0; record.cost_known = duration_s > 0.0; }
        record.attempts.push(format!("audio duration {duration_s:.1}s"));
    }
    record.done = true;
    let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor: ctx.redactor.clone() };
    guard.emit();
    Ok((StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), [(header::CONTENT_TYPE, "application/json")], text).into_response())
}

fn multipart_field(raw: &[u8], boundary: &str, name: &str) -> Option<String> {
    let marker = format!("name=\"{name}\"");
    let text = String::from_utf8_lossy(raw);
    for part in text.split(&format!("--{boundary}")) {
        if part.contains(&marker) && !part.contains("filename=") {
            let (_, body) = part.split_once("\r\n\r\n").or_else(|| part.split_once("\n\n"))?;
            return Some(body.trim_end_matches(['\r', '\n', '-']).trim().to_string());
        }
    }
    None
}

fn replace_multipart_field(raw: &Bytes, boundary: &str, name: &str, value: &str) -> Vec<u8> {
    let marker = format!("name=\"{name}\"").into_bytes();
    let Some(pos) = raw.windows(marker.len()).position(|w| w == marker.as_slice()) else { return raw.to_vec() };
    let Some(start_rel) = raw[pos..].windows(4).position(|w| w == b"\r\n\r\n") else { return raw.to_vec() };
    let start = pos + start_rel + 4;
    let delim = format!("\r\n--{boundary}").into_bytes();
    let Some(end_rel) = raw[start..].windows(delim.len()).position(|w| w == delim.as_slice()) else { return raw.to_vec() };
    let mut out = Vec::with_capacity(raw.len());
    out.extend_from_slice(&raw[..start]);
    out.extend_from_slice(value.as_bytes());
    out.extend_from_slice(&raw[start + end_rel..]);
    out
}

fn finalize(gw: &Gateway, record: &mut CallRecord, observed: Observed, price: (crate::pricing::Price, bool), route_id: Uuid, project: ProjectId) {
    let cost = price.0.cost(observed.input_tokens, observed.output_tokens, observed.cache_read_tokens, observed.cache_write_tokens);
    let tokens = observed.input_tokens + observed.output_tokens;
    record.observed = observed;
    record.cost_usd = cost;
    record.cost_known = price.1;
    record.done = true;
    gw.limits.record(project.0, route_id, cost, tokens);
}

#[allow(clippy::too_many_arguments)]
fn stream_response(
    gw: Arc<Gateway>,
    redactor: Arc<galileo_core::Redactor>,
    record: CallRecord,
    resp: reqwest::Response,
    pformat: Format,
    client_format: Format,
    price: (crate::pricing::Price, bool),
    common_headers: &[(&'static str, String)],
    route_id: Uuid,
    project: ProjectId,
) -> Response {
    let model_hint = record.request_model.clone();
    let mut guard = RecordGuard { record: Some(record), writer: gw.writer.clone(), redactor };
    let mut upstream = resp.bytes_stream();
    let started_at = guard.record.as_ref().map(|r| r.started_at).unwrap_or_else(Instant::now);

    let body_stream = async_stream::stream! {
        let mut parser = SseParser::default();
        let mut observer = StreamObserver::new(pformat);
        let mut translator = (pformat != client_format).then(|| StreamTranslator::new(pformat, client_format, &model_hint));
        while let Some(chunk) = upstream.next().await {
            match chunk {
                Ok(bytes) => {
                    let events = parser.feed(&bytes);
                    if let Some(t) = translator.as_mut() {
                        for (_, data) in &events {
                            for out in t.feed(data) {
                                yield Ok::<Bytes, std::io::Error>(out);
                            }
                        }
                    } else {
                        for (_, data) in &events {
                            observer.feed(data);
                        }
                        yield Ok(bytes);
                    }
                }
                Err(e) => {
                    if let Some(r) = guard.record.as_mut() {
                        r.error = Some(format!("upstream_stream_error: {e}"));
                    }
                    break;
                }
            }
        }
        if let Some(t) = translator.as_mut() {
            for out in t.finish() {
                yield Ok(out);
            }
        }
        let (observed, ttft) = match translator {
            Some(t) => (t.obs.observed, t.obs.first_token_at),
            None => (observer.observed, observer.first_token_at),
        };
        if let Some(r) = guard.record.as_mut() {
            r.ttft_ms = ttft.map(|t| t.duration_since(started_at).as_secs_f64() * 1000.0);
            finalize(&gw, r, observed, price, route_id, project);
        }
        guard.emit();
    };

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(body_stream))
        .unwrap();
    for (k, v) in common_headers {
        if let Ok(hv) = HeaderValue::from_str(v) {
            response.headers_mut().insert(*k, hv);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_helpers() {
        let b = "XYZ";
        let raw = format!("--{b}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nmelea-stt\r\n--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.webm\"\r\nContent-Type: audio/webm\r\n\r\nBYTES\r\n--{b}--\r\n");
        assert_eq!(multipart_field(raw.as_bytes(), b, "model").as_deref(), Some("melea-stt"));
        let out = replace_multipart_field(&Bytes::from(raw.clone()), b, "model", "whisper-large-v3");
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\r\n\r\nwhisper-large-v3\r\n--XYZ"));
        assert!(s.contains("BYTES"));
        let exp = crate::routing::Experiment { name: "e".into(), prompt_name: "p".into(), version_a: 1, version_b: 2, percent_b: 100, sticky: true };
        assert_eq!(experiment_arm(&exp, Some("u1")).0, 2);
        let exp0 = crate::routing::Experiment { percent_b: 0, ..exp };
        assert_eq!(experiment_arm(&exp0, Some("u1")).0, 1);
    }

    #[test]
    fn template_and_ext() {
        let mut vars = serde_json::Map::new();
        vars.insert("name".into(), json!("Sam"));
        vars.insert("n".into(), json!(3));
        assert_eq!(render_template("hi {{name}}, {{n}} items {{missing}}", &vars), "hi Sam, 3 items {{missing}}");
        let mut body = json!({ "model": "a", "galileo": { "prompt": { "name": "greet", "version": 2, "variables": { "x": "y" } } }, "messages": [] });
        let pr = take_galileo_ext(&mut body).unwrap();
        assert_eq!(pr.name, "greet");
        assert_eq!(pr.version, Some(2));
        assert!(body.get("galileo").is_none());
    }
}
