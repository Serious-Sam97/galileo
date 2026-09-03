//! Galileo domain model: the types every other crate speaks.
//!
//! Nothing here knows about ClickHouse, OTLP or HTTP. Storage engines, receivers and the
//! query layer all convert to and from these types, which is what keeps the storage engine
//! swappable.

pub mod attrs;
pub mod config;
pub mod ids;
pub mod log;
pub mod metric;
pub mod redaction;
pub mod pipeline;
pub mod sampling;
pub mod semconv;
pub mod span;

pub use attrs::{AttributeValue, Attributes};
pub use config::Config;
pub use ids::{ProjectId, SpanId, TraceId};
pub use log::{LogRecord, Severity};
pub use metric::{AggregationTemporality, MetricKind, MetricPoint};
pub use pipeline::{LogMetric, LogPipeline, Processor, Quotas};
pub use sampling::Sampling;
pub use redaction::{RedactionAction, RedactionRule, Redactor};
pub use span::{Span, SpanEvent, SpanKind, SpanLink, SpanStatus, StatusCode};
