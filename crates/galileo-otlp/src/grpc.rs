//! OTLP/gRPC receiver: the three collector services on one tonic server.

use std::sync::atomic::Ordering;

use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::{LogsService, LogsServiceServer};
use opentelemetry_proto::tonic::collector::logs::v1::{ExportLogsPartialSuccess, ExportLogsServiceRequest, ExportLogsServiceResponse};
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::{MetricsService, MetricsServiceServer};
use opentelemetry_proto::tonic::collector::metrics::v1::{ExportMetricsPartialSuccess, ExportMetricsServiceRequest, ExportMetricsServiceResponse};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{TraceService, TraceServiceServer};
use opentelemetry_proto::tonic::collector::trace::v1::{ExportTracePartialSuccess, ExportTraceServiceRequest, ExportTraceServiceResponse};
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::auth::{DynResolver, ProjectContext};
use crate::writer::{Batch, WriteError, WriterHandle};
use crate::{convert, extract_key, HEADER_API_KEY, HEADER_AUTHORIZATION};

#[derive(Clone)]
pub struct OtlpGrpc {
    resolver: DynResolver,
    writer: WriterHandle,
}

impl OtlpGrpc {
    pub fn new(resolver: DynResolver, writer: WriterHandle) -> Self {
        Self { resolver, writer }
    }

    /// Build the three services as a tonic router ready to `serve`.
    pub fn router(self) -> tonic::service::Routes {
        tonic::service::Routes::new(TraceServiceServer::new(self.clone()).accept_compressed(tonic::codec::CompressionEncoding::Gzip))
            .add_service(LogsServiceServer::new(self.clone()).accept_compressed(tonic::codec::CompressionEncoding::Gzip))
            .add_service(MetricsServiceServer::new(self).accept_compressed(tonic::codec::CompressionEncoding::Gzip))
    }

    async fn authenticate<T>(&self, req: &Request<T>) -> Result<ProjectContext, Status> {
        let md = req.metadata();
        let auth = md.get(HEADER_AUTHORIZATION).and_then(|v| v.to_str().ok());
        let key = md.get(HEADER_API_KEY).and_then(|v| v.to_str().ok());
        let Some(raw) = extract_key(auth, key) else {
            self.writer.stats.auth_failures.fetch_add(1, Ordering::Relaxed);
            return Err(Status::unauthenticated("missing API key: use 'Authorization: Bearer <key>' or 'x-galileo-key'"));
        };
        match self.resolver.resolve(&raw).await {
            Some(ctx) if ctx.has_scope("ingest") => Ok(ctx),
            Some(_) => Err(Status::permission_denied("API key lacks the 'ingest' scope")),
            None => {
                self.writer.stats.auth_failures.fetch_add(1, Ordering::Relaxed);
                Err(Status::unauthenticated("invalid API key"))
            }
        }
    }

    fn push(&self, batch: Batch) -> Result<(), Status> {
        match self.writer.push(batch) {
            Ok(()) => Ok(()),
            Err(WriteError::Full) => Err(Status::resource_exhausted("ingest queue full, retry with backoff")),
            Err(WriteError::Closed) => Err(Status::unavailable("ingest shutting down")),
        }
    }
}

#[tonic::async_trait]
impl TraceService for OtlpGrpc {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        let ctx = self.authenticate(&request).await?;
        let (spans, rejected) = convert::spans(ctx.project_id, &ctx.redactor, request.into_inner().resource_spans);
        debug!(project = %ctx.project_id, spans = spans.len(), rejected, "grpc traces");
        if let Some(limit) = crate::quota_exceeded(&ctx, &self.writer.stats, "spans", spans.len()) { return Err(tonic::Status::resource_exhausted(format!("daily span quota of {limit} exceeded"))); }
        self.push(if ctx.sampling.is_passthrough() { Batch::Spans(spans) } else { Batch::SampledSpans(ctx.project_id, ctx.sampling.clone(), spans) })?;
        Ok(Response::new(ExportTraceServiceResponse {
            partial_success: (rejected > 0).then(|| ExportTracePartialSuccess {
                rejected_spans: rejected as i64,
                error_message: "spans with empty trace/span id were rejected".into(),
            }),
        }))
    }
}

#[tonic::async_trait]
impl LogsService for OtlpGrpc {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        let ctx = self.authenticate(&request).await?;
        let (logs, rejected) = convert::logs(ctx.project_id, &ctx.redactor, request.into_inner().resource_logs);
        debug!(project = %ctx.project_id, logs = logs.len(), "grpc logs");
        if let Some(limit) = crate::quota_exceeded(&ctx, &self.writer.stats, "logs", logs.len()) { return Err(tonic::Status::resource_exhausted(format!("daily log quota of {limit} exceeded"))); }
        let (logs, points) = crate::apply_pipeline(&ctx, logs);
        if !points.is_empty() { self.push(Batch::Metrics(points))?; }
        self.push(Batch::Logs(logs))?;
        Ok(Response::new(ExportLogsServiceResponse {
            partial_success: (rejected > 0).then(|| ExportLogsPartialSuccess {
                rejected_log_records: rejected as i64,
                error_message: String::new(),
            }),
        }))
    }
}

#[tonic::async_trait]
impl MetricsService for OtlpGrpc {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let ctx = self.authenticate(&request).await?;
        let (points, rejected) = convert::metrics(ctx.project_id, &ctx.redactor, request.into_inner().resource_metrics);
        debug!(project = %ctx.project_id, points = points.len(), "grpc metrics");
        self.push(Batch::Metrics(points))?;
        Ok(Response::new(ExportMetricsServiceResponse {
            partial_success: (rejected > 0).then(|| ExportMetricsPartialSuccess {
                rejected_data_points: rejected as i64,
                error_message: "metrics without data were rejected".into(),
            }),
        }))
    }
}
