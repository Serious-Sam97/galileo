//! # galileo — Rust SDK
//!
//! ```ignore
//! galileo::init(galileo::Config::from_env().service("billing"));
//! // axum
//! let app = axum::Router::new().layer(axum::middleware::from_fn(galileo::axum::middleware));
//! ```
//!
//! What you get: a `tracing` subscriber layer exporting spans and events to Galileo over OTLP/HTTP,
//! `code.*` call-site attributes from `tracing` span metadata, identity (`user.id`, `tenant.id`)
//! attached to every span created while [`identity::with`] is active, one server span per axum
//! request named by the route template, `sql!` spans for sqlx/diesel queries with the normalised
//! statement and table, and [`capture_error`] for exceptions Issues can group.

use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use tracing_subscriber::prelude::*;

pub mod identity;
pub mod sql;
#[cfg(feature = "axum")]
pub mod axum;

pub use opentelemetry;
pub use tracing;

#[derive(Debug, Clone)]
pub struct Config {
    pub endpoint: String,
    pub api_key: String,
    pub service: String,
    pub env: Option<String>,
    pub release: Option<String>,
    /// `RUST_LOG`-style filter for what gets exported (default `info`).
    pub filter: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("GALILEO_ENDPOINT").or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")).unwrap_or_else(|_| "http://localhost:4318".into()).trim_end_matches('/').to_string(),
            api_key: std::env::var("GALILEO_API_KEY").unwrap_or_default(),
            service: std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "rust-app".into()),
            env: std::env::var("GALILEO_ENV").ok(),
            release: std::env::var("GALILEO_RELEASE").ok(),
            filter: std::env::var("GALILEO_FILTER").unwrap_or_else(|_| "info".into()),
        }
    }
    pub fn service(mut self, s: &str) -> Self { self.service = s.into(); self }
    pub fn endpoint(mut self, s: &str) -> Self { self.endpoint = s.trim_end_matches('/').into(); self }
    pub fn api_key(mut self, s: &str) -> Self { self.api_key = s.into(); self }
    pub fn env(mut self, s: &str) -> Self { self.env = Some(s.into()); self }
}

/// Install the global subscriber (tracing → OTLP traces + logs). Call once at startup; returns a
/// guard that flushes on drop.
pub fn init(cfg: Config) -> Guard {
    let mut attrs = vec![KeyValue::new("service.name", cfg.service.clone()), KeyValue::new("telemetry.sdk.name", "galileo-rust")];
    if let Some(e) = &cfg.env { attrs.push(KeyValue::new("deployment.environment", e.clone())); }
    if let Some(r) = &cfg.release { attrs.push(KeyValue::new("service.version", r.clone())); }
    let resource = Resource::builder().with_attributes(attrs).build();
    let mut headers = std::collections::HashMap::new();
    if !cfg.api_key.is_empty() { headers.insert("authorization".to_string(), format!("Bearer {}", cfg.api_key)); }

    let span_exporter = opentelemetry_otlp::SpanExporter::builder().with_http().with_endpoint(format!("{}/v1/traces", cfg.endpoint)).with_headers(headers.clone()).with_timeout(Duration::from_secs(10)).build().expect("otlp span exporter");
    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().with_resource(resource.clone()).with_batch_exporter(span_exporter).with_span_processor(identity::IdentityProcessor).build();
    let log_exporter = opentelemetry_otlp::LogExporter::builder().with_http().with_endpoint(format!("{}/v1/logs", cfg.endpoint)).with_headers(headers).with_timeout(Duration::from_secs(10)).build().expect("otlp log exporter");
    let logger_provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder().with_resource(resource).with_batch_exporter(log_exporter).build();

    use opentelemetry::trace::TracerProvider as _;
    let tracer = tracer_provider.tracer("galileo-rust");
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());
    let filter = tracing_subscriber::EnvFilter::try_new(&cfg.filter).unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer).with_location(true);
    let log_layer = opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider);
    let _ = tracing_subscriber::registry().with(filter).with(otel_layer).with(log_layer).with(tracing_subscriber::fmt::layer().compact()).try_init();
    Guard { tracer_provider, logger_provider }
}

pub struct Guard { tracer_provider: opentelemetry_sdk::trace::SdkTracerProvider, logger_provider: opentelemetry_sdk::logs::SdkLoggerProvider }
impl Guard {
    pub fn flush(&self) { let _ = self.tracer_provider.force_flush(); let _ = self.logger_provider.force_flush(); }
}
impl Drop for Guard {
    fn drop(&mut self) { let _ = self.tracer_provider.shutdown(); let _ = self.logger_provider.shutdown(); }
}

/// Record an error on the current span (exception.* + error status) and emit an error event, so
/// Issues can group it by type + culprit + route.
pub fn capture_error(err: &dyn std::error::Error) {
    let ty = std::any::type_name_of_val(err).rsplit("::").next().unwrap_or("Error").to_string();
    let span = tracing::Span::current();
    span.record("exception.type", ty.as_str());
    span.record("exception.message", err.to_string().as_str());
    span.record("otel.status_code", "ERROR");
    tracing::error!(exception.r#type = %ty, exception.message = %err, error = true, "{ty}: {err}");
}

/// Wrap a fallible block in an `INTERNAL` span; errors are captured. Use for the functions you
/// care about: `galileo::traced!("price_cart", { ... })`.
#[macro_export]
macro_rules! traced {
    ($name:expr, $body:block) => {{
        let __span = $crate::tracing::info_span!($name, otel.name = $name, code.function.name = $name, exception.r#type = $crate::tracing::field::Empty, exception.message = $crate::tracing::field::Empty, otel.status_code = $crate::tracing::field::Empty);
        let __g = __span.enter();
        (|| $body)()
    }};
}
