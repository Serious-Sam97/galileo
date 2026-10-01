//! OTLP/HTTP receiver: `POST /v1/{traces,logs,metrics}` with protobuf or JSON bodies,
//! optionally gzip-encoded. Responses follow the OTLP/HTTP spec (same encoding as the request).

use std::sync::atomic::Ordering;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use opentelemetry_proto::tonic::collector::logs::v1::{ExportLogsPartialSuccess, ExportLogsServiceRequest, ExportLogsServiceResponse};
use opentelemetry_proto::tonic::collector::metrics::v1::{ExportMetricsPartialSuccess, ExportMetricsServiceRequest, ExportMetricsServiceResponse};
use opentelemetry_proto::tonic::collector::trace::v1::{ExportTracePartialSuccess, ExportTraceServiceRequest, ExportTraceServiceResponse};
use prost::Message;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::debug;

use crate::auth::{DynResolver, ProjectContext};
use crate::writer::{Batch, WriteError, WriterHandle};
use crate::{convert, extract_key, HEADER_API_KEY, HEADER_AUTHORIZATION};

const MAX_BODY: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct OtlpHttp {
    resolver: DynResolver,
    writer: WriterHandle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Protobuf,
    Json,
}

impl OtlpHttp {
    pub fn new(resolver: DynResolver, writer: WriterHandle) -> Self {
        Self { resolver, writer }
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/v1/traces", post(traces))
            .route("/v1/logs", post(logs))
            .route("/v1/metrics", post(metrics))
            .route("/healthz", get(|| async { "ok" }))
            // axum's extractor applies its own 2 MB default *after* decompression; without raising it
            // every batch over 2 MB (gzip or not) gets a 413 that SDKs treat as permanent and drop.
            .layer(DefaultBodyLimit::max(MAX_BODY))
            .layer(RequestDecompressionLayer::new().gzip(true))
            .layer(RequestBodyLimitLayer::new(MAX_BODY))
            // Browsers (galileo-rum) post straight here from any page origin.
            .layer(
                tower_http::cors::CorsLayer::new()
                    .allow_origin(tower_http::cors::Any)
                    .allow_methods([axum::http::Method::POST, axum::http::Method::GET, axum::http::Method::OPTIONS])
                    .allow_headers(tower_http::cors::Any)
                    .max_age(std::time::Duration::from_secs(3600)),
            )
            .with_state(self)
    }
}

struct OtlpError {
    status: StatusCode,
    message: String,
}

fn err(status: StatusCode, message: impl Into<String>) -> OtlpError {
    OtlpError { status, message: message.into() }
}

impl IntoResponse for OtlpError {
    fn into_response(self) -> Response {
        // OTLP/HTTP says errors are google.rpc.Status; JSON with a message is what every SDK
        // actually logs, so use that.
        let body = serde_json::json!({ "code": self.status.as_u16(), "message": self.message });
        (self.status, [(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
    }
}

fn encoding(headers: &HeaderMap) -> Result<Encoding, OtlpError> {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/x-protobuf");
    if ct.starts_with("application/x-protobuf") || ct.starts_with("application/protobuf") {
        Ok(Encoding::Protobuf)
    } else if ct.starts_with("application/json") {
        Ok(Encoding::Json)
    } else {
        Err(err(StatusCode::UNSUPPORTED_MEDIA_TYPE, format!("unsupported content-type {ct}")))
    }
}

async fn authenticate(state: &OtlpHttp, headers: &HeaderMap) -> Result<ProjectContext, OtlpError> {
    let auth = headers.get(HEADER_AUTHORIZATION).and_then(|v| v.to_str().ok());
    let key = headers.get(HEADER_API_KEY).and_then(|v| v.to_str().ok());
    let Some(raw) = extract_key(auth, key) else {
        state.writer.stats.auth_failures.fetch_add(1, Ordering::Relaxed);
        return Err(err(StatusCode::UNAUTHORIZED, "missing API key: use 'Authorization: Bearer <key>' or 'x-galileo-key'"));
    };
    match state.resolver.resolve(&raw).await {
        // `rum` keys are meant to be public in page source: ingest only, and everything they send is
        // stamped `galileo.rum = true` so it cannot impersonate a backend service.
        Some(ctx) if ctx.has_scope("ingest") || ctx.has_scope("rum") => Ok(ctx),
        Some(_) => Err(err(StatusCode::FORBIDDEN, "API key lacks the 'ingest' scope")),
        None => {
            state.writer.stats.auth_failures.fetch_add(1, Ordering::Relaxed);
            Err(err(StatusCode::UNAUTHORIZED, "invalid API key"))
        }
    }
}

fn is_rum_only(ctx: &ProjectContext) -> bool {
    ctx.has_scope("rum") && !ctx.has_scope("ingest")
}

fn stamp_rum(resource: &mut opentelemetry_proto::tonic::resource::v1::Resource) {
    use opentelemetry_proto::tonic::common::v1::{any_value::Value, AnyValue, KeyValue};
    resource.attributes.retain(|kv| kv.key != "galileo.rum");
    resource.attributes.push(KeyValue { key: "galileo.rum".into(), value: Some(AnyValue { value: Some(Value::BoolValue(true)) }), ..Default::default() });
}

fn decode<T: Message + Default + serde::de::DeserializeOwned>(enc: Encoding, body: &Bytes) -> Result<T, OtlpError> {
    match enc {
        Encoding::Protobuf => T::decode(body.as_ref()).map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid protobuf: {e}"))),
        Encoding::Json => {
            let mut v: serde_json::Value = serde_json::from_slice(body).map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid json: {e}")))?;
            fill_json_defaults(&mut v);
            serde_json::from_value(v).map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid json: {e}")))
        }
    }
}

/// The generated OTLP serde for exponential-histogram points has no field defaults, and the
/// `data` oneof is flattened, so a point missing `zeroThreshold`/`flags`/`exemplars`/`startTimeUnixNano`
/// silently decodes as "no data". Real exporters omit defaults in JSON; fill them in first.
fn fill_json_defaults(v: &mut serde_json::Value) {
    let Some(rms) = v.get_mut("resourceMetrics").and_then(|x| x.as_array_mut()) else { return };
    for rm in rms {
        let Some(sms) = rm.get_mut("scopeMetrics").and_then(|x| x.as_array_mut()) else { continue };
        for sm in sms {
            let Some(ms) = sm.get_mut("metrics").and_then(|x| x.as_array_mut()) else { continue };
            for m in ms {
                let Some(h) = m.get_mut("exponentialHistogram") else { continue };
                if let Some(pts) = h.get_mut("dataPoints").and_then(|x| x.as_array_mut()) {
                    for p in pts {
                        if let Some(o) = p.as_object_mut() {
                            o.entry("zeroThreshold").or_insert(serde_json::json!(0.0));
                            o.entry("flags").or_insert(serde_json::json!(0));
                            o.entry("exemplars").or_insert(serde_json::json!([]));
                            o.entry("attributes").or_insert(serde_json::json!([]));
                            o.entry("zeroCount").or_insert(serde_json::json!("0"));
                            o.entry("scale").or_insert(serde_json::json!(0));
                            if !o.contains_key("startTimeUnixNano") { let t = o.get("timeUnixNano").cloned().unwrap_or(serde_json::json!("0")); o.insert("startTimeUnixNano".into(), t); }
                            for side in ["positive", "negative"] {
                                if let Some(b) = o.get_mut(side).and_then(|x| x.as_object_mut()) { b.entry("offset").or_insert(serde_json::json!(0)); b.entry("bucketCounts").or_insert(serde_json::json!([])); }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn encode<T: Message + serde::Serialize>(enc: Encoding, resp: &T) -> Response {
    match enc {
        Encoding::Protobuf => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/x-protobuf")],
            resp.encode_to_vec(),
        )
            .into_response(),
        Encoding::Json => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_vec(resp).unwrap_or_default(),
        )
            .into_response(),
    }
}

fn push(state: &OtlpHttp, batch: Batch) -> Result<(), OtlpError> {
    match state.writer.push(batch) {
        Ok(()) => Ok(()),
        Err(WriteError::Full) => Err(OtlpError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "ingest queue full, retry with backoff".into(),
        }),
        Err(WriteError::Closed) => Err(err(StatusCode::SERVICE_UNAVAILABLE, "ingest shutting down")),
    }
}

async fn traces(State(state): State<OtlpHttp>, headers: HeaderMap, body: Bytes) -> Result<Response, OtlpError> {
    let enc = encoding(&headers)?;
    let ctx = authenticate(&state, &headers).await?;
    let mut req: ExportTraceServiceRequest = decode(enc, &body)?;
    if is_rum_only(&ctx) { for rs in &mut req.resource_spans { stamp_rum(rs.resource.get_or_insert_with(Default::default)); } }
    let (spans, rejected) = convert::spans(ctx.project_id, &ctx.redactor, req.resource_spans);
    debug!(project = %ctx.project_id, spans = spans.len(), rejected, ?enc, "http traces");
    if let Some(limit) = crate::quota_exceeded(&ctx, &state.writer.stats, "spans", spans.len()) { state.writer.stats.quota_rejected.fetch_add(spans.len() as u64, Ordering::Relaxed); return Err(err(StatusCode::TOO_MANY_REQUESTS, format!("daily span quota of {limit} exceeded"))); }
    push(&state, if ctx.sampling.is_passthrough() { Batch::Spans(spans) } else { Batch::SampledSpans(ctx.project_id, ctx.sampling.clone(), spans) })?;
    Ok(encode(
        enc,
        &ExportTraceServiceResponse {
            partial_success: (rejected > 0).then(|| ExportTracePartialSuccess {
                rejected_spans: rejected as i64,
                error_message: "spans with empty trace/span id were rejected".into(),
            }),
        },
    ))
}

async fn logs(State(state): State<OtlpHttp>, headers: HeaderMap, body: Bytes) -> Result<Response, OtlpError> {
    let enc = encoding(&headers)?;
    let ctx = authenticate(&state, &headers).await?;
    let mut req: ExportLogsServiceRequest = decode(enc, &body)?;
    if is_rum_only(&ctx) { for rl in &mut req.resource_logs { stamp_rum(rl.resource.get_or_insert_with(Default::default)); } }
    let (logs, rejected) = convert::logs(ctx.project_id, &ctx.redactor, req.resource_logs);
    debug!(project = %ctx.project_id, logs = logs.len(), ?enc, "http logs");
    if let Some(limit) = crate::quota_exceeded(&ctx, &state.writer.stats, "logs", logs.len()) { state.writer.stats.quota_rejected.fetch_add(logs.len() as u64, Ordering::Relaxed); return Err(err(StatusCode::TOO_MANY_REQUESTS, format!("daily log quota of {limit} exceeded"))); }
    let (logs, points) = crate::apply_pipeline(&ctx, logs);
    if !points.is_empty() { push(&state, Batch::Metrics(points))?; }
    push(&state, Batch::Logs(logs))?;
    Ok(encode(
        enc,
        &ExportLogsServiceResponse {
            partial_success: (rejected > 0).then(|| ExportLogsPartialSuccess {
                rejected_log_records: rejected as i64,
                error_message: String::new(),
            }),
        },
    ))
}

async fn metrics(State(state): State<OtlpHttp>, headers: HeaderMap, body: Bytes) -> Result<Response, OtlpError> {
    let enc = encoding(&headers)?;
    let ctx = authenticate(&state, &headers).await?;
    let mut req: ExportMetricsServiceRequest = decode(enc, &body)?;
    if is_rum_only(&ctx) { for rm in &mut req.resource_metrics { stamp_rum(rm.resource.get_or_insert_with(Default::default)); } }
    let (points, rejected) = convert::metrics(ctx.project_id, &ctx.redactor, req.resource_metrics);
    debug!(project = %ctx.project_id, points = points.len(), ?enc, "http metrics");
    if let Some(limit) = crate::quota_exceeded(&ctx, &state.writer.stats, "metrics", points.len()) { state.writer.stats.quota_rejected.fetch_add(points.len() as u64, Ordering::Relaxed); return Err(err(StatusCode::TOO_MANY_REQUESTS, format!("daily metric quota of {limit} exceeded"))); }
    push(&state, Batch::Metrics(points))?;
    Ok(encode(
        enc,
        &ExportMetricsServiceResponse {
            partial_success: (rejected > 0).then(|| ExportMetricsPartialSuccess {
                rejected_data_points: rejected as i64,
                error_message: "metrics without data were rejected".into(),
            }),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StaticResolver;
    use crate::writer::BatchWriter;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use galileo_core::config::IngestConfig;
    use galileo_core::{AttributeValue, LogRecord, MetricPoint, ProjectId, Redactor, Span, TraceId};
    use galileo_storage::{QueryResult, SqlQuery, Storage};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[derive(Default)]
    struct MemStorage {
        spans: Mutex<Vec<Span>>,
        logs: Mutex<Vec<LogRecord>>,
    }

    #[async_trait]
    impl Storage for MemStorage {
        async fn migrate(&self) -> galileo_storage::Result<()> {
            Ok(())
        }
        async fn apply_retention(&self, _: &galileo_core::config::RetentionConfig) -> galileo_storage::Result<()> {
            Ok(())
        }
        async fn write_spans(&self, s: &[Span]) -> galileo_storage::Result<()> {
            self.spans.lock().unwrap().extend_from_slice(s);
            Ok(())
        }
        async fn write_logs(&self, l: &[LogRecord]) -> galileo_storage::Result<()> {
            self.logs.lock().unwrap().extend_from_slice(l);
            Ok(())
        }
        async fn write_metrics(&self, _: &[MetricPoint]) -> galileo_storage::Result<()> {
            Ok(())
        }
        async fn query(&self, _: &SqlQuery) -> galileo_storage::Result<QueryResult> {
            Ok(QueryResult::default())
        }
        async fn execute(&self, _: &SqlQuery) -> galileo_storage::Result<()> {
            Ok(())
        }
        async fn fetch_trace(&self, _: ProjectId, _: TraceId) -> galileo_storage::Result<Vec<Span>> {
            Ok(vec![])
        }
        async fn ping(&self) -> galileo_storage::Result<()> {
            Ok(())
        }
    }

    fn setup() -> (Router, Arc<MemStorage>, ProjectId) {
        let storage = Arc::new(MemStorage::default());
        let cfg = IngestConfig {
            batch_max_rows: 1,
            batch_max_wait: std::time::Duration::from_millis(50),
            max_queued_rows: 10_000,
        };
        let writer = BatchWriter::start(storage.clone(), &cfg);
        let pid = ProjectId::new();
        let resolver = Arc::new(StaticResolver::new([(
            "k1".to_string(),
            ProjectContext { project_id: pid, redactor: Arc::new(Redactor::with_defaults(&[]).unwrap()), scopes: vec!["ingest".into()], sampling: Default::default(), pipeline: Default::default(), log_metrics: Default::default(), quotas: Default::default() },
        )]));
        let app = OtlpHttp::new(resolver, writer.handle()).router();
        std::mem::forget(writer); // keep the task alive for the test's lifetime
        (app, storage, pid)
    }

    #[tokio::test]
    async fn json_traces_end_to_end() {
        let (app, storage, pid) = setup();
        let body = serde_json::json!({
            "resourceSpans": [{
                "resource": { "attributes": [{ "key": "service.name", "value": { "stringValue": "web" } }] },
                "scopeSpans": [{
                    "scope": { "name": "test" },
                    "spans": [{
                        "traceId": "5b8efff798038103d269b633813fc60c",
                        "spanId": "eee19b7ec3c1b174",
                        "name": "GET /hello",
                        "kind": 2,
                        "startTimeUnixNano": "1700000000000000000",
                        "endTimeUnixNano": "1700000000010000000",
                        "attributes": [
                            { "key": "http.route", "value": { "stringValue": "/hello" } },
                            { "key": "http.response.status_code", "value": { "intValue": "200" } },
                            { "key": "password", "value": { "stringValue": "nope" } }
                        ],
                        "status": { "code": 1 }
                    }]
                }]
            }]
        });
        let req = Request::post("/v1/traces")
            .header("content-type", "application/json")
            .header("authorization", "Bearer k1")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let spans = storage.spans.lock().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].project_id, pid);
        assert_eq!(spans[0].service_name, "web");
        assert_eq!(spans[0].attributes["http.response.status_code"], AttributeValue::Int(200));
        assert!(spans[0].attributes.get("password").is_none());
        assert_eq!(spans[0].trace_id.to_hex(), "5b8efff798038103d269b633813fc60c");
    }

    #[tokio::test]
    async fn protobuf_roundtrip_and_auth() {
        let (app, _storage, _pid) = setup();
        let req_pb = ExportTraceServiceRequest { resource_spans: vec![] };
        let unauthenticated = Request::post("/v1/traces")
            .header("content-type", "application/x-protobuf")
            .body(Body::from(req_pb.encode_to_vec()))
            .unwrap();
        assert_eq!(app.clone().oneshot(unauthenticated).await.unwrap().status(), StatusCode::UNAUTHORIZED);

        let bad_key = Request::post("/v1/traces")
            .header("content-type", "application/x-protobuf")
            .header("x-galileo-key", "wrong")
            .body(Body::from(req_pb.encode_to_vec()))
            .unwrap();
        assert_eq!(app.clone().oneshot(bad_key).await.unwrap().status(), StatusCode::UNAUTHORIZED);

        let ok = Request::post("/v1/traces")
            .header("content-type", "application/x-protobuf")
            .header("x-galileo-key", "k1")
            .body(Body::from(req_pb.encode_to_vec()))
            .unwrap();
        let resp = app.clone().oneshot(ok).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()[header::CONTENT_TYPE], "application/x-protobuf");

        let bad_ct = Request::post("/v1/logs")
            .header("content-type", "text/plain")
            .header("x-galileo-key", "k1")
            .body(Body::from("x"))
            .unwrap();
        assert_eq!(app.oneshot(bad_ct).await.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn accepts_batches_over_two_megabytes() {
        let (app, _storage, _pid) = setup();
        let pad = "x".repeat(3 * 1024 * 1024);
        let body = serde_json::json!({ "resourceSpans": [{ "resource": { "attributes": [{ "key": "pad", "value": { "stringValue": pad } }] }, "scopeSpans": [] }] });
        let req = Request::post("/v1/traces")
            .header("content-type", "application/json")
            .header("authorization", "Bearer k1")
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_eq!(app.oneshot(req).await.unwrap().status(), StatusCode::OK);
    }
}
