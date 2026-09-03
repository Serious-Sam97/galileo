use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use clickhouse::{Client, Row};
use galileo_core::config::{ClickHouseConfig, RetentionConfig};
use galileo_core::{LogRecord, MetricPoint, ProjectId, Span, TraceId};
use serde::Deserialize;
use tracing::{debug, info};

use crate::rows::{LogRow, MetricRow, SpanRow};
use crate::schema::{MIGRATIONS, MIGRATIONS_TABLE, RETENTION_TABLES};
use crate::{Column, QueryResult, QueryStats, Result, SqlQuery, SqlValue, Storage, StorageError};

#[derive(Clone)]
pub struct ClickHouseStorage {
    client: Client,
}

impl ClickHouseStorage {
    pub fn new(cfg: &ClickHouseConfig) -> Self {
        let client = Client::default()
            .with_url(&cfg.url)
            .with_database(&cfg.database)
            .with_user(&cfg.user)
            .with_password(&cfg.password)
            // 64-bit ints come back as JSON numbers, not strings.
            .with_setting("output_format_json_quote_64bit_integers", "0")
            .with_setting("date_time_output_format", "iso")
            .with_setting("async_insert", "0");
        Self { client }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Bind positional parameters onto a query in order.
    fn bind_all(mut q: clickhouse::query::Query, params: &[SqlValue]) -> clickhouse::query::Query {
        for p in params {
            q = match p {
                SqlValue::Str(s) => q.bind(s.as_str()),
                SqlValue::Int(i) => q.bind(*i),
                SqlValue::UInt(u) => q.bind(*u),
                SqlValue::Float(f) => q.bind(*f),
                SqlValue::Bool(b) => q.bind(*b),
                SqlValue::Uuid(u) => q.bind(u.to_string()),
                SqlValue::Timestamp(t) => q.bind(t.timestamp_nanos_opt().unwrap_or(0)),
                SqlValue::StrList(v) => q.bind(v.as_slice()),
            };
        }
        q
    }

    async fn insert_all<R>(&self, table: &str, rows: Vec<R>) -> Result<()>
    where
        R: Row + serde::Serialize,
        for<'a> R: clickhouse::RowWrite<Value<'a> = R>,
    {
        if rows.is_empty() {
            return Ok(());
        }
        let n = rows.len();
        let mut insert = self.client.insert::<R>(table).await?;
        for r in &rows {
            insert.write(r).await?;
        }
        insert.end().await?;
        debug!(table, rows = n, "inserted");
        Ok(())
    }
}

#[derive(Row, Deserialize)]
struct VersionRow {
    version: u32,
}

#[derive(Deserialize)]
struct CompactMeta {
    name: String,
    #[serde(rename = "type")]
    ty: String,
}

#[derive(Deserialize)]
struct CompactStats {
    #[serde(default)]
    elapsed: f64,
    #[serde(default)]
    rows_read: u64,
    #[serde(default)]
    bytes_read: u64,
}

#[derive(Deserialize)]
struct CompactResponse {
    meta: Vec<CompactMeta>,
    data: Vec<Vec<serde_json::Value>>,
    #[serde(default)]
    statistics: Option<CompactStats>,
}

#[async_trait]
impl Storage for ClickHouseStorage {
    async fn migrate(&self) -> Result<()> {
        self.client
            .query(&format!(
                "CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (version UInt32, applied_at DateTime DEFAULT now()) \
                 ENGINE = MergeTree ORDER BY version"
            ))
            .execute()
            .await?;
        let applied: Vec<VersionRow> = self
            .client
            .query(&format!("SELECT version FROM {MIGRATIONS_TABLE} ORDER BY version"))
            .fetch_all()
            .await?;
        let applied: Vec<u32> = applied.into_iter().map(|v| v.version).collect();
        for m in MIGRATIONS {
            if applied.contains(&m.version) {
                continue;
            }
            info!(version = m.version, "applying clickhouse migration");
            for stmt in m.statements {
                self.client.query(stmt).execute().await?;
            }
            self.client
                .query(&format!("INSERT INTO {MIGRATIONS_TABLE} (version) VALUES (?)"))
                .bind(m.version)
                .execute()
                .await?;
        }
        Ok(())
    }

    async fn apply_retention(&self, retention: &RetentionConfig) -> Result<()> {
        for table in RETENTION_TABLES {
            let d: Duration = match *table {
                "spans" => retention.spans,
                "logs" => retention.logs,
                _ => retention.metrics,
            };
            let secs = d.as_secs().max(3600);
            let ttl = match retention.storage_policy.as_deref().filter(|p| !p.is_empty()) {
                Some(policy) => {
                    // move to the cold volume after hot_days, delete after the retention window
                    self.client.query(&format!("ALTER TABLE {table} MODIFY SETTING storage_policy = '{}'", policy.replace('\'', ""))).execute().await?;
                    let hot = (u64::from(retention.hot_days.max(1)) * 86_400).min(secs);
                    format!("toDateTime(timestamp) + INTERVAL {hot} SECOND TO VOLUME 'cold', toDateTime(timestamp) + INTERVAL {secs} SECOND")
                }
                None => format!("toDateTime(timestamp) + INTERVAL {secs} SECOND"),
            };
            self.client.query(&format!("ALTER TABLE {table} MODIFY TTL {ttl}")).execute().await?;
        }
        Ok(())
    }

    async fn write_spans(&self, spans: &[Span]) -> Result<()> {
        self.insert_all("spans", spans.iter().map(SpanRow::from).collect()).await
    }

    async fn write_logs(&self, logs: &[LogRecord]) -> Result<()> {
        self.insert_all("logs", logs.iter().map(LogRow::from).collect()).await
    }

    async fn write_metrics(&self, points: &[MetricPoint]) -> Result<()> {
        self.insert_all("metrics", points.iter().map(MetricRow::from).collect()).await
    }

    async fn query(&self, q: &SqlQuery) -> Result<QueryResult> {
        let sql = q.sql.trim().trim_end_matches(';');
        let lowered = sql.to_ascii_lowercase();
        if !(lowered.starts_with("select") || lowered.starts_with("with")) {
            return Err(StorageError::InvalidQuery("only SELECT queries are allowed".into()));
        }
        let query = Self::bind_all(self.client.query(sql), &q.params)
            .with_setting("max_execution_time", "60")
            .with_setting("max_result_rows", "100000")
            .with_setting("result_overflow_mode", "break");
        let mut cursor = query.fetch_bytes("JSONCompact")?;
        let bytes = cursor.collect().await?;
        let parsed: CompactResponse = serde_json::from_slice(&bytes)?;
        Ok(QueryResult {
            columns: parsed.meta.into_iter().map(|m| Column { name: m.name, ty: m.ty }).collect(),
            rows: parsed.data,
            stats: parsed
                .statistics
                .map(|s| QueryStats { elapsed: s.elapsed, rows_read: s.rows_read, bytes_read: s.bytes_read })
                .unwrap_or_default(),
        })
    }

    async fn execute(&self, q: &SqlQuery) -> Result<()> {
        let sql = q.sql.trim().trim_end_matches(';');
        if !sql.to_ascii_lowercase().starts_with("delete") {
            return Err(StorageError::InvalidQuery("execute only accepts DELETE statements".into()));
        }
        Self::bind_all(self.client.query(sql), &q.params).with_setting("mutations_sync", "0").execute().await?;
        Ok(())
    }

    async fn fetch_trace(&self, project_id: ProjectId, trace_id: TraceId) -> Result<Vec<Span>> {
        let rows: Vec<SpanRow> = self
            .client
            .query("SELECT ?fields FROM spans WHERE project_id = ? AND trace_id = ? ORDER BY timestamp ASC")
            .bind(project_id.0.to_string())
            .bind(trace_id.to_hex())
            .fetch_all()
            .await?;
        Ok(rows.into_iter().map(Span::from).collect())
    }

    async fn ping(&self) -> Result<()> {
        #[derive(Row, Deserialize)]
        struct One {
            _one: u8,
        }
        let _: One = self.client.query("SELECT 1 AS _one").fetch_one().await?;
        Ok(())
    }
}

/// Helper for tests and tools: parse a ClickHouse ISO timestamp string.
pub fn parse_ch_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc)).or_else(|| {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
            .ok()
            .map(|n| n.and_utc())
    })
}
