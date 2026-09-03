//! A small pet-shop API instrumented with the OpenTelemetry SDK that:
//! * exports traces to Galileo over OTLP/gRPC with a project API key,
//! * tags every request with user.id / tenant.id,
//! * calls an LLM through the Galileo gateway with the trace context propagated, so the
//!   LLM span shows up inside the request trace.
//!
//! Run: GALILEO_KEY=glk_... cargo run -p galileo-demo-app   (then hit http://127.0.0.1:9090)

use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use opentelemetry::trace::{SpanKind, Status, TraceContextExt, Tracer, TracerProvider};
use opentelemetry::{global, KeyValue};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use rand::Rng;
use tracing::{info, instrument, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::prelude::*;

#[derive(Clone)]
struct App {
    http: reqwest::Client,
    gateway: String,
    key: String,
}

fn init_tracing() -> SdkTracerProvider {
    let key = std::env::var("GALILEO_KEY").unwrap_or_else(|_| "dev".into());
    let endpoint = std::env::var("GALILEO_OTLP").unwrap_or_else(|_| "http://127.0.0.1:4317".into());
    let mut md = tonic_metadata();
    md.insert("authorization", format!("Bearer {key}").parse().expect("metadata"));
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_metadata(md)
        .with_timeout(Duration::from_secs(5))
        .build()
        .expect("otlp exporter");
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_service_name("petshop-demo")
                .with_attributes([KeyValue::new("deployment.environment.name", "demo"), KeyValue::new("service.version", "0.1.0")])
                .build(),
        )
        .build();
    global::set_tracer_provider(provider.clone());
    global::set_text_map_propagator(opentelemetry_sdk::propagation::TraceContextPropagator::new());
    let tracer = provider.tracer("petshop-demo");
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .init();
    provider
}

fn tonic_metadata() -> opentelemetry_otlp::tonic_types::metadata::MetadataMap {
    opentelemetry_otlp::tonic_types::metadata::MetadataMap::new()
}

#[tokio::main]
async fn main() {
    let provider = init_tracing();
    let app = App {
        http: reqwest::Client::new(),
        gateway: std::env::var("GALILEO_GATEWAY").unwrap_or_else(|_| "http://127.0.0.1:8080/gw".into()),
        key: std::env::var("GALILEO_KEY").unwrap_or_else(|_| "dev".into()),
    };
    let router = Router::new()
        .route("/pets/{id}", get(get_pet))
        .route("/pets/{id}/describe", post(describe_pet))
        .route("/health", get(|| async { "ok" }))
        .with_state(app);
    let addr = "127.0.0.1:9090";
    info!(%addr, "petshop demo listening");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, router).with_graceful_shutdown(async { tokio::signal::ctrl_c().await.ok(); }).await.expect("serve");
    provider.shutdown().ok();
}

/// A fake auth layer: every request belongs to a tenant and a user.
fn identity() -> (String, String) {
    let mut r = rand::rng();
    let tenants = ["acme", "globex", "umbrella"];
    (tenants[r.random_range(0..tenants.len())].to_string(), format!("user-{}", r.random_range(1..30)))
}

#[instrument(name = "GET /pets/{id}", skip_all, fields(otel.kind = "server", http.request.method = "GET", http.route = "/pets/{id}", url.path, user.id, tenant.id, http.response.status_code))]
async fn get_pet(Path(id): Path<u32>) -> (StatusCode, Json<serde_json::Value>) {
    let (tenant, user) = identity();
    Span::current().record("url.path", format!("/pets/{id}").as_str());
    Span::current().record("user.id", user.as_str());
    Span::current().record("tenant.id", tenant.as_str());
    let pet = load_pet(id, &tenant).await;
    let status = if pet.is_some() { StatusCode::OK } else { StatusCode::NOT_FOUND };
    Span::current().record("http.response.status_code", status.as_u16());
    match pet {
        Some(p) => (status, Json(p)),
        None => (status, Json(serde_json::json!({ "error": "no such pet" }))),
    }
}

#[instrument(name = "SELECT pets", skip_all, fields(otel.kind = "client", db.system = "postgresql", db.query.text = "SELECT * FROM pets WHERE id = $1", tenant.id = %tenant))]
async fn load_pet(id: u32, tenant: &str) -> Option<serde_json::Value> {
    // umbrella's shard is slow, so BubbleUp has something to find
    let ms = if tenant == "umbrella" { rand::rng().random_range(200..600) } else { rand::rng().random_range(5..40) };
    tokio::time::sleep(Duration::from_millis(ms)).await;
    let name = ["Rex", "Mia", "Tom"][id as usize % 3];
    (!id.is_multiple_of(7)).then(|| serde_json::json!({ "id": id, "name": name, "species": "dog" }))
}

#[instrument(name = "POST /pets/{id}/describe", skip_all, fields(otel.kind = "server", http.request.method = "POST", http.route = "/pets/{id}/describe", user.id, tenant.id, http.response.status_code))]
async fn describe_pet(State(app): State<App>, Path(id): Path<u32>) -> (StatusCode, Json<serde_json::Value>) {
    let (tenant, user) = identity();
    Span::current().record("user.id", user.as_str());
    Span::current().record("tenant.id", tenant.as_str());
    let Some(pet) = load_pet(id, &tenant).await else {
        Span::current().record("http.response.status_code", 404);
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "no such pet" })));
    };
    match ask_llm(&app, &user, &tenant, &pet).await {
        Ok(text) => {
            Span::current().record("http.response.status_code", 200);
            (StatusCode::OK, Json(serde_json::json!({ "pet": pet, "description": text })))
        }
        Err(e) => {
            Span::current().record("http.response.status_code", 502);
            Span::current().set_status(Status::error(e.clone()));
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "error": e })))
        }
    }
}

/// Call the gateway with the current trace context in `traceparent`, so Galileo joins the LLM
/// span to this request's trace. Any Anthropic/OpenAI SDK can do the same by adding headers.
async fn ask_llm(app: &App, user: &str, tenant: &str, pet: &serde_json::Value) -> Result<String, String> {
    let tracer = global::tracer("petshop-demo");
    let parent = Span::current().context();
    let span = tracer.span_builder("gateway.describe").with_kind(SpanKind::Client).start_with_context(&tracer, &parent);
    let cx = parent.with_span(span);
    let sc = cx.span().span_context().clone();
    let traceparent = format!("00-{}-{}-01", sc.trace_id(), sc.span_id());
    let body = serde_json::json!({
        "model": "assistant",
        "max_tokens": 60,
        "messages": [{ "role": "user", "content": format!("Describe this pet in one short sentence: {pet}") }],
        "galileo": { "prompt": { "name": "greet", "variables": { "tone": "a friendly vet" } } }
    });
    let res = app
        .http
        .post(format!("{}/v1/messages", app.gateway))
        .header("x-api-key", &app.key)
        .header("anthropic-version", "2023-06-01")
        .header("traceparent", traceparent)
        .header("x-galileo-user-id", user)
        .header("x-galileo-tenant-id", tenant)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let v: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    cx.span().end();
    if !status.is_success() {
        return Err(v["error"]["message"].as_str().unwrap_or("gateway error").to_string());
    }
    Ok(v["content"][0]["text"].as_str().unwrap_or("").to_string())
}
