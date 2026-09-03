//! ClickHouse schema. Each migration is a list of idempotent statements. Add a new entry to
//! evolve the schema; never edit an applied one.
//!
//! Design notes:
//! * one row per span/log/point, partitioned by day, ordered by (project, service, time) so
//!   project-scoped time-range scans touch only their own granules;
//! * "hot" columns for the attributes the UI defaults to, filled at write time from the
//!   attribute maps; the maps keep everything, so nothing is lost;
//! * `attrs` holds every attribute as a string, `attrs_num` additionally holds numeric ones as
//!   Float64 so percentiles/sums work without casts;
//! * bloom filters on map keys/values and trace ids make "any attribute = X" filters skip
//!   granules instead of scanning them (the Honeycomb property).

pub const MIGRATIONS_TABLE: &str = "galileo_schema_migrations";

pub struct Migration {
    pub version: u32,
    pub statements: &'static [&'static str],
}

pub const MIGRATIONS: &[Migration] = &[Migration { version: 1, statements: V1 }, Migration { version: 2, statements: V2 }, Migration { version: 3, statements: V3 }, Migration { version: 4, statements: V4 }, Migration { version: 5, statements: V5 }];

const V1: &[&str] = &[
    r#"
CREATE TABLE IF NOT EXISTS spans (
    project_id          UUID,
    timestamp           DateTime64(9, 'UTC'),
    end_time            DateTime64(9, 'UTC'),
    duration_ns         UInt64,
    trace_id            String,
    span_id             String,
    parent_span_id      String,
    name                LowCardinality(String),
    kind                LowCardinality(String),
    status_code         LowCardinality(String),
    status_message      String,
    service_name        LowCardinality(String),
    service_version     LowCardinality(String),
    deployment_env      LowCardinality(String),
    host_name           LowCardinality(String),
    scope_name          LowCardinality(String),
    scope_version       LowCardinality(String),
    http_method         LowCardinality(String),
    http_route          LowCardinality(String),
    http_status_code    UInt16,
    url_path            String,
    db_system           LowCardinality(String),
    user_id             String,
    tenant_id           String,
    exception_type      LowCardinality(String),
    gen_ai_system       LowCardinality(String),
    gen_ai_model        LowCardinality(String),
    gen_ai_input_tokens UInt32,
    gen_ai_output_tokens UInt32,
    gen_ai_cost_usd     Float64,
    resource            Map(LowCardinality(String), String),
    attrs               Map(LowCardinality(String), String),
    attrs_num           Map(LowCardinality(String), Float64),
    events_name         Array(String),
    events_timestamp    Array(DateTime64(9, 'UTC')),
    events_attrs        Array(Map(String, String)),
    links_trace_id      Array(String),
    links_span_id       Array(String),
    links_attrs         Array(Map(String, String)),

    INDEX idx_trace_id    trace_id          TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_span_id     span_id           TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_user_id     user_id           TYPE bloom_filter(0.01)  GRANULARITY 1,
    INDEX idx_tenant_id   tenant_id         TYPE bloom_filter(0.01)  GRANULARITY 1,
    INDEX idx_attr_keys   mapKeys(attrs)    TYPE bloom_filter(0.01)  GRANULARITY 1,
    INDEX idx_attr_vals   mapValues(attrs)  TYPE bloom_filter(0.01)  GRANULARITY 1,
    INDEX idx_res_vals    mapValues(resource) TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_url_path    url_path          TYPE tokenbf_v1(8192, 3, 0) GRANULARITY 1,
    INDEX idx_duration    duration_ns       TYPE minmax GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (project_id, service_name, timestamp, trace_id)
TTL toDateTime(timestamp) + INTERVAL 30 DAY
SETTINGS index_granularity = 8192, ttl_only_drop_parts = 1
"#,
    r#"
CREATE TABLE IF NOT EXISTS logs (
    project_id          UUID,
    timestamp           DateTime64(9, 'UTC'),
    observed_timestamp  DateTime64(9, 'UTC'),
    severity_number     UInt8,
    severity            LowCardinality(String),
    severity_text       LowCardinality(String),
    body                String,
    trace_id            String,
    span_id             String,
    service_name        LowCardinality(String),
    service_version     LowCardinality(String),
    deployment_env      LowCardinality(String),
    host_name           LowCardinality(String),
    scope_name          LowCardinality(String),
    user_id             String,
    tenant_id           String,
    resource            Map(LowCardinality(String), String),
    attrs               Map(LowCardinality(String), String),
    attrs_num           Map(LowCardinality(String), Float64),

    INDEX idx_body        body              TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 1,
    INDEX idx_trace_id    trace_id          TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_attr_keys   mapKeys(attrs)    TYPE bloom_filter(0.01)  GRANULARITY 1,
    INDEX idx_attr_vals   mapValues(attrs)  TYPE bloom_filter(0.01)  GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (project_id, service_name, timestamp)
TTL toDateTime(timestamp) + INTERVAL 14 DAY
SETTINGS index_granularity = 8192, ttl_only_drop_parts = 1
"#,
    r#"
CREATE TABLE IF NOT EXISTS metrics (
    project_id          UUID,
    timestamp           DateTime64(9, 'UTC'),
    start_timestamp     DateTime64(9, 'UTC'),
    name                LowCardinality(String),
    description         String,
    unit                LowCardinality(String),
    kind                LowCardinality(String),
    temporality         UInt8,
    is_monotonic        Bool,
    service_name        LowCardinality(String),
    scope_name          LowCardinality(String),
    resource            Map(LowCardinality(String), String),
    attrs               Map(LowCardinality(String), String),
    value               Float64,
    count               UInt64,
    sum                 Float64,
    min                 Nullable(Float64),
    max                 Nullable(Float64),
    bucket_counts       Array(UInt64),
    explicit_bounds     Array(Float64),

    INDEX idx_attr_vals   mapValues(attrs)  TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (project_id, name, service_name, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY
SETTINGS index_granularity = 8192, ttl_only_drop_parts = 1
"#,
];

/// V2: code location and query detail as hot columns, request id for log ↔ trace joins.
const V2: &[&str] = &[
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS code_function LowCardinality(String) AFTER exception_type",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS code_namespace LowCardinality(String) AFTER code_function",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS code_file LowCardinality(String) AFTER code_namespace",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS code_line UInt32 AFTER code_file",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS db_operation LowCardinality(String) AFTER db_system",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS db_table LowCardinality(String) AFTER db_operation",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS request_id String AFTER tenant_id",
    "ALTER TABLE spans ADD INDEX IF NOT EXISTS idx_request_id request_id TYPE bloom_filter(0.01) GRANULARITY 1",
    "ALTER TABLE logs ADD COLUMN IF NOT EXISTS request_id String AFTER tenant_id",
    "ALTER TABLE logs ADD COLUMN IF NOT EXISTS code_function LowCardinality(String) AFTER request_id",
    "ALTER TABLE logs ADD INDEX IF NOT EXISTS idx_log_user user_id TYPE bloom_filter(0.01) GRANULARITY 1",
    "ALTER TABLE logs ADD INDEX IF NOT EXISTS idx_log_request request_id TYPE bloom_filter(0.01) GRANULARITY 1",
];

/// V5: pre-aggregated RED and DB rollups per minute for long retention (fed by materialized views).
const V5: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS spans_red_1m (
        project_id UUID, minute DateTime, service_name LowCardinality(String), http_route LowCardinality(String), tenant_id String, status_code LowCardinality(String),
        requests AggregateFunction(count), errors AggregateFunction(countIf, UInt8), duration AggregateFunction(quantilesTDigest(0.5, 0.75, 0.9, 0.95, 0.99), Float64), duration_sum AggregateFunction(sum, Float64)
    ) ENGINE = AggregatingMergeTree PARTITION BY toYYYYMM(minute) ORDER BY (project_id, minute, service_name, http_route, tenant_id, status_code) TTL minute + INTERVAL 400 DAY",
    "CREATE MATERIALIZED VIEW IF NOT EXISTS spans_red_1m_mv TO spans_red_1m AS
        SELECT project_id, toStartOfMinute(timestamp) AS minute, service_name, http_route, tenant_id, status_code,
               countState() AS requests, countIfState(status_code = 'error') AS errors,
               quantilesTDigestState(0.5, 0.75, 0.9, 0.95, 0.99)(duration_ns / 1e6) AS duration, sumState(duration_ns / 1e6) AS duration_sum
        FROM spans WHERE parent_span_id = '' GROUP BY project_id, minute, service_name, http_route, tenant_id, status_code",
    "CREATE TABLE IF NOT EXISTS db_calls_1m (
        project_id UUID, minute DateTime, service_name LowCardinality(String), db_table LowCardinality(String), db_operation LowCardinality(String),
        calls AggregateFunction(count), total_ms AggregateFunction(sum, Float64), duration AggregateFunction(quantilesTDigest(0.5, 0.95, 0.99), Float64)
    ) ENGINE = AggregatingMergeTree PARTITION BY toYYYYMM(minute) ORDER BY (project_id, minute, service_name, db_table, db_operation) TTL minute + INTERVAL 400 DAY",
    "CREATE MATERIALIZED VIEW IF NOT EXISTS db_calls_1m_mv TO db_calls_1m AS
        SELECT project_id, toStartOfMinute(timestamp) AS minute, service_name, db_table, db_operation,
               countState() AS calls, sumState(duration_ns / 1e6) AS total_ms, quantilesTDigestState(0.5, 0.95, 0.99)(duration_ns / 1e6) AS duration
        FROM spans WHERE db_system != '' GROUP BY project_id, minute, service_name, db_table, db_operation",
    // backfill: materialized views only see new inserts, so seed the rollups from existing spans once
    "INSERT INTO spans_red_1m SELECT project_id, toStartOfMinute(timestamp) AS minute, service_name, http_route, tenant_id, status_code,
        countState() AS requests, countIfState(status_code = 'error') AS errors, quantilesTDigestState(0.5, 0.75, 0.9, 0.95, 0.99)(duration_ns / 1e6) AS duration, sumState(duration_ns / 1e6) AS duration_sum
        FROM spans WHERE parent_span_id = '' GROUP BY project_id, minute, service_name, http_route, tenant_id, status_code",
    "INSERT INTO db_calls_1m SELECT project_id, toStartOfMinute(timestamp) AS minute, service_name, db_table, db_operation,
        countState() AS calls, sumState(duration_ns / 1e6) AS total_ms, quantilesTDigestState(0.5, 0.95, 0.99)(duration_ns / 1e6) AS duration
        FROM spans WHERE db_system != '' GROUP BY project_id, minute, service_name, db_table, db_operation",
];

/// V4: exponential-histogram raw buckets on metrics.
const V4: &[&str] = &[
    "ALTER TABLE metrics ADD COLUMN IF NOT EXISTS exp_scale Int32 AFTER explicit_bounds",
    "ALTER TABLE metrics ADD COLUMN IF NOT EXISTS exp_zero_count UInt64 AFTER exp_scale",
    "ALTER TABLE metrics ADD COLUMN IF NOT EXISTS exp_pos_offset Int32 AFTER exp_zero_count",
    "ALTER TABLE metrics ADD COLUMN IF NOT EXISTS exp_pos_counts Array(UInt64) AFTER exp_pos_offset",
    "ALTER TABLE metrics ADD COLUMN IF NOT EXISTS exp_neg_offset Int32 AFTER exp_pos_counts",
    "ALTER TABLE metrics ADD COLUMN IF NOT EXISTS exp_neg_counts Array(UInt64) AFTER exp_neg_offset",
];

/// V3: exception grouping.
const V3: &[&str] = &[
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS exception_message String AFTER exception_type",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS exception_culprit LowCardinality(String) AFTER exception_message",
    "ALTER TABLE spans ADD COLUMN IF NOT EXISTS exception_fingerprint String AFTER exception_culprit",
    "ALTER TABLE spans ADD INDEX IF NOT EXISTS idx_fingerprint exception_fingerprint TYPE bloom_filter(0.01) GRANULARITY 1",
];

/// Tables whose TTL follows the retention config, with the config field they map to.
pub const RETENTION_TABLES: &[&str] = &["spans", "logs", "metrics"];
