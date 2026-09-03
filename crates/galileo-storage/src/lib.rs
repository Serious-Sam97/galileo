//! Storage abstraction for events. One trait, one production implementation (ClickHouse).
//!
//! The query crate never writes SQL for a specific engine directly; it emits a `SqlQuery`
//! (text + positional parameters) that the storage backend binds safely.

pub mod clickhouse;
pub mod exceptions;
pub mod rows;
pub mod schema;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use galileo_core::config::RetentionConfig;
use galileo_core::{LogRecord, MetricPoint, ProjectId, Span, TraceId};
use serde::{Deserialize, Serialize};

pub use crate::clickhouse::ClickHouseStorage;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("clickhouse: {0}")]
    ClickHouse(#[from] ::clickhouse::error::Error),
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// A positional query parameter. Backends are responsible for escaping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SqlValue {
    Str(String),
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    Uuid(uuid::Uuid),
    /// Nanosecond-precision timestamp, bound as DateTime64(9).
    Timestamp(DateTime<Utc>),
    StrList(Vec<String>),
}

impl From<&str> for SqlValue {
    fn from(v: &str) -> Self {
        SqlValue::Str(v.to_owned())
    }
}
impl From<String> for SqlValue {
    fn from(v: String) -> Self {
        SqlValue::Str(v)
    }
}
impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        SqlValue::Int(v)
    }
}
impl From<u64> for SqlValue {
    fn from(v: u64) -> Self {
        SqlValue::UInt(v)
    }
}
impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        SqlValue::Float(v)
    }
}
impl From<ProjectId> for SqlValue {
    fn from(v: ProjectId) -> Self {
        SqlValue::Uuid(v.0)
    }
}
impl From<DateTime<Utc>> for SqlValue {
    fn from(v: DateTime<Utc>) -> Self {
        SqlValue::Timestamp(v)
    }
}

/// SQL text with `?` placeholders plus the values for them, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SqlQuery {
    pub sql: String,
    pub params: Vec<SqlValue>,
}

impl SqlQuery {
    pub fn new(sql: impl Into<String>) -> Self {
        Self { sql: sql.into(), params: Vec::new() }
    }
    pub fn bind(mut self, v: impl Into<SqlValue>) -> Self {
        self.params.push(v.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryStats {
    #[serde(default)]
    pub elapsed: f64,
    #[serde(default)]
    pub rows_read: u64,
    #[serde(default)]
    pub bytes_read: u64,
}

/// Column-typed result of a dynamic query. Rows are JSON values in column order.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryResult {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<serde_json::Value>>,
    #[serde(default)]
    pub stats: QueryStats,
}

impl QueryResult {
    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
    /// Rows as objects, for callers that prefer names over positions.
    pub fn to_objects(&self) -> Vec<serde_json::Map<String, serde_json::Value>> {
        self.rows
            .iter()
            .map(|r| {
                self.columns
                    .iter()
                    .zip(r.iter())
                    .map(|(c, v)| (c.name.clone(), v.clone()))
                    .collect()
            })
            .collect()
    }
}

#[async_trait]
pub trait Storage: Send + Sync + 'static {
    /// Create or upgrade the schema. Idempotent.
    async fn migrate(&self) -> Result<()>;
    /// Apply TTLs from configuration. Idempotent.
    async fn apply_retention(&self, retention: &RetentionConfig) -> Result<()>;

    async fn write_spans(&self, spans: &[Span]) -> Result<()>;
    async fn write_logs(&self, logs: &[LogRecord]) -> Result<()>;
    async fn write_metrics(&self, points: &[MetricPoint]) -> Result<()>;

    /// Run a read-only query built by the query layer.
    async fn query(&self, q: &SqlQuery) -> Result<QueryResult>;

    /// Run a data-changing statement (retention deletes). Not exposed to users.
    async fn execute(&self, q: &SqlQuery) -> Result<()>;

    /// All spans of one trace, in start-time order.
    async fn fetch_trace(&self, project_id: ProjectId, trace_id: TraceId) -> Result<Vec<Span>>;

    /// Cheap liveness check.
    async fn ping(&self) -> Result<()>;
}

pub type DynStorage = Arc<dyn Storage>;
