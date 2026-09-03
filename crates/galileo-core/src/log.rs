use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AttributeValue, Attributes, ProjectId, SpanId, TraceId};

/// OTLP severity numbers, collapsed to the canonical text levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Unspecified,
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Severity {
    pub fn from_otlp_number(n: i32) -> Self {
        match n {
            1..=4 => Severity::Trace,
            5..=8 => Severity::Debug,
            9..=12 => Severity::Info,
            13..=16 => Severity::Warn,
            17..=20 => Severity::Error,
            21..=24 => Severity::Fatal,
            _ => Severity::Unspecified,
        }
    }

    pub fn from_text(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "trace" => Severity::Trace,
            "debug" => Severity::Debug,
            "info" | "information" | "notice" => Severity::Info,
            "warn" | "warning" => Severity::Warn,
            "error" | "err" => Severity::Error,
            "fatal" | "critical" | "crit" | "emergency" | "alert" => Severity::Fatal,
            _ => Severity::Unspecified,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Unspecified => "unspecified",
            Severity::Trace => "trace",
            Severity::Debug => "debug",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }

    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Severity::Trace,
            2 => Severity::Debug,
            3 => Severity::Info,
            4 => Severity::Warn,
            5 => Severity::Error,
            6 => Severity::Fatal,
            _ => Severity::Unspecified,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    pub project_id: ProjectId,
    pub timestamp: DateTime<Utc>,
    pub observed_timestamp: DateTime<Utc>,
    pub severity: Severity,
    #[serde(default)]
    pub severity_text: String,
    /// The body rendered as text. Structured bodies are kept as JSON in the same string and the
    /// original shape is preserved in `body_value`.
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_value: Option<AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<TraceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<SpanId>,
    pub service_name: String,
    #[serde(default)]
    pub scope_name: String,
    #[serde(default)]
    pub resource: Attributes,
    #[serde(default)]
    pub attributes: Attributes,
}
