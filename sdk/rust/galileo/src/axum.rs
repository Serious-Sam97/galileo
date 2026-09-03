//! axum middleware: one SERVER span per request named by the route template, W3C traceparent in,
//! `x-galileo-trace-id` out, identity from an `Identity` request extension (set by your auth layer).

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::trace::TraceContextExt;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::identity::{self, Identity};

/// `Router::new().layer(axum::middleware::from_fn(galileo::axum::middleware))`
pub async fn middleware(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let route = req.extensions().get::<MatchedPath>().map(|m| m.as_str().to_string()).unwrap_or_else(|| path.clone());
    let id = req.extensions().get::<Identity>().cloned().unwrap_or_default();
    let parent = opentelemetry::global::get_text_map_propagator(|p| p.extract(&HeaderCarrier(req.headers())));
    let span = tracing::info_span!("http.request", otel.name = %format!("{method} {route}"), otel.kind = "server",
        http.request.method = %method, http.route = %route, url.path = %path, http.response.status_code = tracing::field::Empty, otel.status_code = tracing::field::Empty,
        user_agent.original = %req.headers().get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or(""));
    let _ = span.set_parent(parent);
    let trace_id = span.context().span().span_context().trace_id().to_string();
    let fut = async move {
        let resp = next.run(req).await;
        let status = resp.status().as_u16();
        let s = tracing::Span::current();
        s.record("http.response.status_code", status);
        if status >= 500 { s.record("otel.status_code", "ERROR"); }
        resp
    }.instrument(span);
    let mut resp = identity::with(id, fut).await;
    if let Ok(v) = http::HeaderValue::from_str(&trace_id) { resp.headers_mut().insert("x-galileo-trace-id", v); }
    resp
}

struct HeaderCarrier<'a>(&'a http::HeaderMap);
impl opentelemetry::propagation::Extractor for HeaderCarrier<'_> {
    fn get(&self, key: &str) -> Option<&str> { self.0.get(key).and_then(|v| v.to_str().ok()) }
    fn keys(&self) -> Vec<&str> { self.0.keys().map(|k| k.as_str()).collect() }
}
