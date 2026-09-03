//! Turn a finished (or failed) gateway call into a span and hand it to the ingest writer.

use std::time::Instant;

use chrono::{DateTime, Utc};
use galileo_core::semconv as sc;
use galileo_core::{AttributeValue, Attributes, ProjectId, Redactor, Span, SpanEvent, SpanId, SpanKind, SpanStatus, StatusCode, TraceId};
use galileo_otlp::writer::Batch;
use galileo_otlp::WriterHandle;

use crate::formats::{cap, Format, Observed};

#[derive(Debug, Clone)]
pub struct TraceContext {
    pub trace_id: TraceId,
    pub parent_span_id: Option<SpanId>,
    pub sampled: bool,
}

impl TraceContext {
    /// Parse a W3C `traceparent` header; a fresh trace when absent or malformed.
    pub fn from_header(h: Option<&str>) -> Self {
        if let Some(h) = h {
            let parts: Vec<&str> = h.trim().split('-').collect();
            if parts.len() >= 4 {
                if let (Some(t), Some(p)) = (TraceId::from_hex(parts[1]), SpanId::from_hex(parts[2])) {
                    if !t.is_zero() {
                        let sampled = u8::from_str_radix(parts[3], 16).map(|f| f & 1 == 1).unwrap_or(true);
                        return Self { trace_id: t, parent_span_id: Some(p).filter(|p| !p.is_zero()), sampled };
                    }
                }
            }
        }
        Self { trace_id: TraceId::random(), parent_span_id: None, sampled: true }
    }

    pub fn traceparent(&self, span_id: SpanId) -> String {
        format!("00-{}-{}-01", self.trace_id.to_hex(), span_id.to_hex())
    }
}

pub struct CallRecord {
    /// "chat" | "embeddings" | "transcription" — span name prefix and gen_ai.operation.name.
    pub operation: &'static str,
    /// Internal caller (e.g. "assistant") when the call did not come from an app; recorded as
    /// `gen_ai.galileo.caller`.
    pub caller: Option<String>,
    pub project_id: ProjectId,
    pub ctx: TraceContext,
    pub span_id: SpanId,
    pub started: DateTime<Utc>,
    pub started_at: Instant,
    pub client_format: Format,
    pub route_alias: String,
    pub route_id: Option<uuid::Uuid>,
    pub provider_name: String,
    pub provider_system: String,
    pub request_model: String,
    pub fallback_index: u32,
    pub streaming: bool,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub prompt: String,
    pub prompt_name: Option<String>,
    pub prompt_version: Option<i64>,
    pub experiment: Option<(String, &'static str)>,
    pub cache_hit: bool,
    /// "exact" | "semantic" when served from a cache.
    pub cache_kind: Option<&'static str>,
    /// Guardrail hit: (kind, action) e.g. ("pii", "redact").
    pub guardrail: Option<(String, String)>,
    /// "health" when smart routing reordered the targets.
    pub routing: Option<&'static str>,
    pub user_id: Option<String>,
    pub tenant_id: Option<String>,
    pub conversation_id: Option<String>,
    pub record_content: bool,
    pub attempts: Vec<String>,
    // filled at the end
    pub observed: Observed,
    pub cost_usd: f64,
    pub cost_known: bool,
    pub http_status: u16,
    pub error: Option<String>,
    pub ttft_ms: Option<f64>,
    pub done: bool,
}

impl CallRecord {
    pub fn finish_span(&self, redactor: &Redactor) -> Span {
        let end = Utc::now();
        let mut a = Attributes::new();
        let o = &self.observed;
        a.insert(sc::GEN_AI_SYSTEM.into(), self.provider_system.clone().into());
        a.insert(sc::GEN_AI_OPERATION.into(), self.operation.into());
        if let Some(c) = &self.caller {
            a.insert("gen_ai.galileo.caller".into(), c.clone().into());
        }
        a.insert(sc::GEN_AI_REQUEST_MODEL.into(), self.request_model.clone().into());
        if !o.model.is_empty() {
            a.insert(sc::GEN_AI_RESPONSE_MODEL.into(), o.model.clone().into());
        } else {
            a.insert(sc::GEN_AI_RESPONSE_MODEL.into(), self.request_model.clone().into());
        }
        a.insert(sc::GEN_AI_INPUT_TOKENS.into(), (o.input_tokens as i64).into());
        a.insert(sc::GEN_AI_OUTPUT_TOKENS.into(), (o.output_tokens as i64).into());
        if o.cache_read_tokens > 0 {
            a.insert("gen_ai.usage.cache_read_input_tokens".into(), (o.cache_read_tokens as i64).into());
        }
        if o.cache_write_tokens > 0 {
            a.insert("gen_ai.usage.cache_creation_input_tokens".into(), (o.cache_write_tokens as i64).into());
        }
        if !o.stop_reason.is_empty() {
            a.insert(sc::GEN_AI_FINISH_REASONS.into(), o.stop_reason.clone().into());
        }
        if !o.id.is_empty() {
            a.insert("gen_ai.response.id".into(), o.id.clone().into());
        }
        if let Some(t) = self.temperature {
            a.insert(sc::GEN_AI_REQUEST_TEMPERATURE.into(), t.into());
        }
        if let Some(m) = self.max_tokens {
            a.insert(sc::GEN_AI_REQUEST_MAX_TOKENS.into(), (m as i64).into());
        }
        a.insert(sc::GEN_AI_COST_USD.into(), self.cost_usd.into());
        a.insert("gen_ai.galileo.cost_known".into(), self.cost_known.into());
        a.insert(sc::GEN_AI_ROUTE.into(), self.route_alias.clone().into());
        a.insert(sc::GEN_AI_PROVIDER.into(), self.provider_name.clone().into());
        a.insert(sc::GEN_AI_FALLBACK_INDEX.into(), (self.fallback_index as i64).into());
        a.insert(sc::GEN_AI_STREAMING.into(), self.streaming.into());
        a.insert("gen_ai.galileo.client_format".into(), self.client_format.as_str().into());
        a.insert("http.response.status_code".into(), (self.http_status as i64).into());
        if let Some(t) = self.ttft_ms {
            a.insert(sc::GEN_AI_TTFT_MS.into(), t.into());
        }
        if let Some(n) = &self.prompt_name {
            a.insert(sc::GEN_AI_PROMPT_NAME.into(), n.clone().into());
        }
        if let Some(v) = self.prompt_version {
            a.insert(sc::GEN_AI_PROMPT_VERSION.into(), v.into());
        }
        if let Some((name, arm)) = &self.experiment {
            a.insert("gen_ai.galileo.experiment".into(), name.clone().into());
            a.insert("gen_ai.galileo.experiment.arm".into(), (*arm).into());
        }
        a.insert("gen_ai.galileo.cache_hit".into(), self.cache_hit.into());
        if let Some(k) = self.cache_kind { a.insert("gen_ai.galileo.cache_kind".into(), k.into()); }
        if let Some((k, act)) = &self.guardrail { a.insert("gen_ai.galileo.guardrail".into(), k.clone().into()); a.insert("gen_ai.galileo.guardrail_action".into(), act.clone().into()); }
        if let Some(r) = self.routing { a.insert("gen_ai.galileo.routing".into(), r.into()); }
        if !o.tool_calls.is_empty() {
            a.insert("gen_ai.tool_calls".into(), o.tool_calls.iter().map(|(_, n, _)| n.as_str()).collect::<Vec<_>>().join(",").into());
        }
        if let Some(u) = &self.user_id {
            a.insert(sc::USER_ID.into(), u.clone().into());
        }
        if let Some(t) = &self.tenant_id {
            a.insert(sc::TENANT_ID.into(), t.clone().into());
        }
        if let Some(c) = &self.conversation_id {
            a.insert("gen_ai.conversation.id".into(), c.clone().into());
        }
        if self.record_content {
            a.insert(sc::GEN_AI_PROMPT.into(), redactor.redact_text(&cap(&self.prompt)).into());
            a.insert(sc::GEN_AI_COMPLETION.into(), redactor.redact_text(&cap(&o.text)).into());
        }
        if let Some(e) = &self.error {
            a.insert("error.type".into(), e.clone().into());
        }
        let mut resource = Attributes::new();
        resource.insert(sc::SERVICE_NAME.into(), "galileo-gateway".into());
        let events = self
            .attempts
            .iter()
            .map(|msg| SpanEvent {
                name: "gateway.fallback".into(),
                timestamp: end,
                attributes: [("message".to_string(), AttributeValue::from(msg.as_str()))].into_iter().collect(),
            })
            .collect();
        let status = match &self.error {
            Some(e) => SpanStatus { code: StatusCode::Error, message: e.clone() },
            None if self.http_status >= 400 => SpanStatus { code: StatusCode::Error, message: format!("upstream status {}", self.http_status) },
            None => SpanStatus { code: StatusCode::Ok, message: String::new() },
        };
        Span {
            project_id: self.project_id,
            trace_id: self.ctx.trace_id,
            span_id: self.span_id,
            parent_span_id: self.ctx.parent_span_id,
            name: format!("{} {}", self.operation, if o.model.is_empty() { &self.request_model } else { &o.model }),
            kind: SpanKind::Client,
            start_time: self.started,
            end_time: end,
            status,
            service_name: "galileo-gateway".into(),
            scope_name: "galileo.gateway".into(),
            scope_version: env!("CARGO_PKG_VERSION").into(),
            resource,
            attributes: a,
            events,
            links: vec![],
        }
    }
}

/// Emits the span exactly once, even if the response stream is dropped mid-way.
pub struct RecordGuard {
    pub record: Option<CallRecord>,
    pub writer: WriterHandle,
    pub redactor: std::sync::Arc<Redactor>,
}

impl RecordGuard {
    pub fn emit(&mut self) {
        if let Some(r) = self.record.take() {
            let span = r.finish_span(&self.redactor);
            if let Err(e) = self.writer.push(Batch::Spans(vec![span])) {
                tracing::warn!(error = %e, "could not record gateway span");
            }
        }
    }
}

impl Drop for RecordGuard {
    fn drop(&mut self) {
        if let Some(r) = self.record.as_mut() {
            if !r.done {
                r.error.get_or_insert_with(|| "client_disconnected".into());
            }
        }
        self.emit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traceparent_roundtrip() {
        let c = TraceContext::from_header(Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"));
        assert_eq!(c.trace_id.to_hex(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(c.parent_span_id.unwrap().to_hex(), "00f067aa0ba902b7");
        let s = SpanId::random();
        assert!(c.traceparent(s).starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"));
        let fresh = TraceContext::from_header(Some("garbage"));
        assert!(fresh.parent_span_id.is_none());
        assert!(!fresh.trace_id.is_zero());
    }
}
