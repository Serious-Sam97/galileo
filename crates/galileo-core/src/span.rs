use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Attributes, ProjectId, SpanId, TraceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// OTLP numbering: 0 = unspecified (treated as internal), 1 = internal, 2 = server ...
    #[default]
    Internal = 1,
    Server = 2,
    Client = 3,
    Producer = 4,
    Consumer = 5,
}

impl SpanKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanKind::Internal => "internal",
            SpanKind::Server => "server",
            SpanKind::Client => "client",
            SpanKind::Producer => "producer",
            SpanKind::Consumer => "consumer",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        match v {
            2 => SpanKind::Server,
            3 => SpanKind::Client,
            4 => SpanKind::Producer,
            5 => SpanKind::Consumer,
            _ => SpanKind::Internal,
        }
    }
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StatusCode {
    #[default]
    Unset,
    Ok,
    Error,
}

impl StatusCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusCode::Unset => "unset",
            StatusCode::Ok => "ok",
            StatusCode::Error => "error",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => StatusCode::Ok,
            2 => StatusCode::Error,
            _ => StatusCode::Unset,
        }
    }
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SpanStatus {
    pub code: StatusCode,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub attributes: Attributes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanLink {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    #[serde(default)]
    pub attributes: Attributes,
}

/// One wide event. Resource attributes (service.name, host, deployment) and scope
/// (instrumentation library) are kept apart from span attributes because they are queried
/// differently and because keeping them apart avoids key collisions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub project_id: ProjectId,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<SpanId>,
    pub name: String,
    pub kind: SpanKind,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub status: SpanStatus,
    pub service_name: String,
    #[serde(default)]
    pub scope_name: String,
    #[serde(default)]
    pub scope_version: String,
    #[serde(default)]
    pub resource: Attributes,
    #[serde(default)]
    pub attributes: Attributes,
    #[serde(default)]
    pub events: Vec<SpanEvent>,
    #[serde(default)]
    pub links: Vec<SpanLink>,
}

impl Span {
    pub fn duration_ns(&self) -> u64 {
        (self.end_time - self.start_time)
            .num_nanoseconds()
            .unwrap_or(0)
            .max(0) as u64
    }

    pub fn is_root(&self) -> bool {
        self.parent_span_id.is_none()
    }

    /// Attribute lookup that checks span attributes first, then resource attributes. This is
    /// what the hot-column extraction and the redactor use.
    pub fn attr(&self, key: &str) -> Option<&crate::AttributeValue> {
        self.attributes.get(key).or_else(|| self.resource.get(key))
    }
}
