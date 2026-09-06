//! Row structs matching the ClickHouse tables exactly, plus conversions to/from the domain.

use chrono::{DateTime, Utc};
use clickhouse::Row;
use galileo_core::attrs::split_for_storage;
use galileo_core::semconv as sc;
use galileo_core::{
    AttributeValue, Attributes, LogRecord, MetricPoint, ProjectId, Severity, Span, SpanEvent,
    SpanId, SpanKind, SpanLink, SpanStatus, StatusCode, TraceId,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

type StrMap = IndexMap<String, String>;
type NumMap = IndexMap<String, f64>;

fn str_map(attrs: &Attributes) -> StrMap {
    attrs.iter().map(|(k, v)| (k.clone(), v.to_string_repr())).collect()
}

fn from_str_map(m: StrMap) -> Attributes {
    m.into_iter().map(|(k, v)| (k, AttributeValue::Str(v))).collect()
}

/// Rebuild typed attributes from the string map plus the numeric map: numbers win.
fn merge_attrs(strs: StrMap, nums: &NumMap) -> Attributes {
    let mut out = Attributes::with_capacity(strs.len());
    for (k, v) in strs {
        if let Some(n) = nums.get(&k) {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                out.insert(k, AttributeValue::Int(*n as i64));
            } else {
                out.insert(k, AttributeValue::Float(*n));
            }
        } else if v == "true" {
            out.insert(k, AttributeValue::Bool(true));
        } else if v == "false" {
            out.insert(k, AttributeValue::Bool(false));
        } else {
            out.insert(k, AttributeValue::Str(v));
        }
    }
    out
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct SpanRow {
    #[serde(with = "clickhouse::serde::uuid")]
    pub project_id: Uuid,
    #[serde(with = "clickhouse::serde::chrono::datetime64::nanos")]
    pub timestamp: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::nanos")]
    pub end_time: DateTime<Utc>,
    pub duration_ns: u64,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: String,
    pub name: String,
    pub kind: String,
    pub status_code: String,
    pub status_message: String,
    pub service_name: String,
    pub service_version: String,
    pub deployment_env: String,
    pub host_name: String,
    pub scope_name: String,
    pub scope_version: String,
    pub http_method: String,
    pub http_route: String,
    pub http_status_code: u16,
    pub url_path: String,
    pub db_system: String,
    pub db_operation: String,
    pub db_table: String,
    pub user_id: String,
    pub tenant_id: String,
    pub request_id: String,
    pub exception_type: String,
    pub exception_message: String,
    pub exception_culprit: String,
    pub exception_fingerprint: String,
    pub code_function: String,
    pub code_namespace: String,
    pub code_file: String,
    pub code_line: u32,
    pub gen_ai_system: String,
    pub gen_ai_model: String,
    pub gen_ai_input_tokens: u32,
    pub gen_ai_output_tokens: u32,
    pub gen_ai_cost_usd: f64,
    pub resource: StrMap,
    pub attrs: StrMap,
    pub attrs_num: NumMap,
    pub events_name: Vec<String>,
    /// DateTime64(9) as nanoseconds since epoch (no Vec helper in the client crate).
    pub events_timestamp: Vec<i64>,
    pub events_attrs: Vec<StrMap>,
    pub links_trace_id: Vec<String>,
    pub links_span_id: Vec<String>,
    pub links_attrs: Vec<StrMap>,
}

fn s(attrs: &Attributes, keys: &[&str]) -> String {
    sc::first_str(attrs, keys).map(str::to_owned).unwrap_or_default()
}

impl From<&Span> for SpanRow {
    fn from(sp: &Span) -> Self {
        let (attr_strs, attr_nums) = split_for_storage(&sp.attributes);
        let a = &sp.attributes;
        let r = &sp.resource;
        // exception.* usually lives on the "exception" event, not the span.
        let exc_event = sp.events.iter().find(|e| e.attributes.contains_key(sc::EXCEPTION_TYPE));
        let exc_get = |k: &str| -> String {
            sc::first_str(a, &[k])
                .map(str::to_owned)
                .or_else(|| exc_event.and_then(|e| e.attributes.get(k).and_then(|v| v.as_str()).map(str::to_owned)))
                .unwrap_or_default()
        };
        let exception_type = exc_get(sc::EXCEPTION_TYPE);
        let exception_message: String = exc_get(sc::EXCEPTION_MESSAGE).chars().take(500).collect();
        let http_route_s = s(a, &[sc::HTTP_ROUTE]);
        let (exception_culprit, exception_fingerprint) = if exception_type.is_empty() {
            (String::new(), String::new())
        } else {
            let stack = exc_get("exception.stacktrace");
            let culprit = crate::exceptions::culprit_from_stacktrace(&stack)
                .or_else(|| {
                    let f = s(a, &[sc::CODE_FUNCTION, sc::CODE_FUNCTION_LEGACY]);
                    (!f.is_empty()).then(|| {
                        let ns = s(a, &[sc::CODE_NAMESPACE]);
                        if ns.is_empty() { f } else { format!("{ns}.{f}") }
                    })
                })
                .unwrap_or_default();
            (culprit.clone(), crate::exceptions::fingerprint(&exception_type, &culprit, &http_route_s))
        };
        Self {
            project_id: sp.project_id.0,
            timestamp: sp.start_time,
            end_time: sp.end_time,
            duration_ns: sp.duration_ns(),
            trace_id: sp.trace_id.to_hex(),
            span_id: sp.span_id.to_hex(),
            parent_span_id: sp.parent_span_id.map(|p| p.to_hex()).unwrap_or_default(),
            name: sp.name.clone(),
            kind: sp.kind.as_str().to_owned(),
            status_code: sp.status.code.as_str().to_owned(),
            status_message: sp.status.message.clone(),
            service_name: sp.service_name.clone(),
            service_version: s(r, &[sc::SERVICE_VERSION]),
            deployment_env: s(r, &[sc::DEPLOYMENT_ENV, sc::DEPLOYMENT_ENV_LEGACY]),
            host_name: s(r, &[sc::HOST_NAME]),
            scope_name: sp.scope_name.clone(),
            scope_version: sp.scope_version.clone(),
            http_method: s(a, &[sc::HTTP_METHOD, sc::HTTP_METHOD_LEGACY]),
            http_route: s(a, &[sc::HTTP_ROUTE]),
            http_status_code: sc::first_i64(a, &[sc::HTTP_STATUS, sc::HTTP_STATUS_LEGACY])
                .unwrap_or(0)
                .clamp(0, u16::MAX as i64) as u16,
            url_path: s(a, &[sc::URL_PATH, sc::HTTP_TARGET_LEGACY]),
            db_system: s(a, &[sc::DB_SYSTEM, sc::DB_SYSTEM_NAME]),
            db_operation: { let v = s(a, &[sc::DB_OPERATION, sc::DB_OPERATION_NEW]); if v.is_empty() { sql_shape(sc::first_str(a, &[sc::DB_STATEMENT, sc::DB_STATEMENT_LEGACY]).unwrap_or("")).0 } else { v } },
            db_table: { let v = s(a, &[sc::DB_TABLE, sc::DB_TABLE_NEW]); if v.is_empty() { sql_shape(sc::first_str(a, &[sc::DB_STATEMENT, sc::DB_STATEMENT_LEGACY]).unwrap_or("")).1 } else { v } },
            request_id: s(a, &[sc::REQUEST_ID]),
            code_function: s(a, &[sc::CODE_FUNCTION, sc::CODE_FUNCTION_LEGACY]),
            code_namespace: s(a, &[sc::CODE_NAMESPACE]),
            code_file: s(a, &[sc::CODE_FILE, sc::CODE_FILE_LEGACY]),
            code_line: sc::first_i64(a, &[sc::CODE_LINE, sc::CODE_LINE_LEGACY]).unwrap_or(0).clamp(0, u32::MAX as i64) as u32,
            user_id: sc::first_string(a, &[sc::USER_ID, sc::ENDUSER_ID_LEGACY])
                .or_else(|| sc::first_string(r, &[sc::USER_ID]))
                .unwrap_or_default(),
            tenant_id: sc::first_string(a, &[sc::TENANT_ID])
                .or_else(|| sc::first_string(r, &[sc::TENANT_ID]))
                .unwrap_or_default(),
            exception_type,
            exception_message,
            exception_culprit,
            exception_fingerprint,
            gen_ai_system: s(a, &[sc::GEN_AI_SYSTEM]),
            gen_ai_model: s(a, &[sc::GEN_AI_RESPONSE_MODEL, sc::GEN_AI_REQUEST_MODEL]),
            gen_ai_input_tokens: sc::first_i64(a, &[sc::GEN_AI_INPUT_TOKENS]).unwrap_or(0).max(0) as u32,
            gen_ai_output_tokens: sc::first_i64(a, &[sc::GEN_AI_OUTPUT_TOKENS]).unwrap_or(0).max(0) as u32,
            gen_ai_cost_usd: a.get(sc::GEN_AI_COST_USD).and_then(|v| v.as_f64()).unwrap_or(0.0),
            resource: str_map(&sp.resource),
            attrs: attr_strs.into_iter().collect(),
            attrs_num: attr_nums.into_iter().collect(),
            events_name: sp.events.iter().map(|e| e.name.clone()).collect(),
            events_timestamp: sp.events.iter().map(|e| e.timestamp.timestamp_nanos_opt().unwrap_or(0)).collect(),
            events_attrs: sp.events.iter().map(|e| str_map(&e.attributes)).collect(),
            links_trace_id: sp.links.iter().map(|l| l.trace_id.to_hex()).collect(),
            links_span_id: sp.links.iter().map(|l| l.span_id.to_hex()).collect(),
            links_attrs: sp.links.iter().map(|l| str_map(&l.attributes)).collect(),
        }
    }
}

impl From<SpanRow> for Span {
    fn from(r: SpanRow) -> Self {
        let events = r
            .events_name
            .into_iter()
            .zip(r.events_timestamp)
            .zip(r.events_attrs)
            .map(|((name, ts), attrs)| SpanEvent {
                name,
                timestamp: DateTime::<Utc>::from_timestamp_nanos(ts),
                attributes: from_str_map(attrs),
            })
            .collect();
        let links = r
            .links_trace_id
            .iter()
            .zip(r.links_span_id.iter())
            .zip(r.links_attrs)
            .filter_map(|((t, sid), attrs)| {
                Some(SpanLink {
                    trace_id: TraceId::from_hex(t)?,
                    span_id: SpanId::from_hex(sid)?,
                    attributes: from_str_map(attrs),
                })
            })
            .collect();
        Span {
            project_id: ProjectId(r.project_id),
            trace_id: TraceId::from_hex(&r.trace_id).unwrap_or(TraceId::ZERO),
            span_id: SpanId::from_hex(&r.span_id).unwrap_or(SpanId::ZERO),
            parent_span_id: SpanId::from_hex(&r.parent_span_id).filter(|p| !p.is_zero()),
            name: r.name,
            kind: kind_from_str(&r.kind),
            start_time: r.timestamp,
            end_time: r.end_time,
            status: SpanStatus { code: status_from_str(&r.status_code), message: r.status_message },
            service_name: r.service_name,
            scope_name: r.scope_name,
            scope_version: r.scope_version,
            resource: from_str_map(r.resource),
            attributes: merge_attrs(r.attrs, &r.attrs_num),
            events,
            links,
        }
    }
}

pub fn kind_from_str(s: &str) -> SpanKind {
    match s {
        "server" => SpanKind::Server,
        "client" => SpanKind::Client,
        "producer" => SpanKind::Producer,
        "consumer" => SpanKind::Consumer,
        _ => SpanKind::Internal,
    }
}

pub fn status_from_str(s: &str) -> StatusCode {
    match s {
        "ok" => StatusCode::Ok,
        "error" => StatusCode::Error,
        _ => StatusCode::Unset,
    }
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct LogRow {
    #[serde(with = "clickhouse::serde::uuid")]
    pub project_id: Uuid,
    #[serde(with = "clickhouse::serde::chrono::datetime64::nanos")]
    pub timestamp: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::nanos")]
    pub observed_timestamp: DateTime<Utc>,
    pub severity_number: u8,
    pub severity: String,
    pub severity_text: String,
    pub body: String,
    pub trace_id: String,
    pub span_id: String,
    pub service_name: String,
    pub service_version: String,
    pub deployment_env: String,
    pub host_name: String,
    pub scope_name: String,
    pub user_id: String,
    pub tenant_id: String,
    pub request_id: String,
    pub code_function: String,
    pub resource: StrMap,
    pub attrs: StrMap,
    pub attrs_num: NumMap,
}

impl From<&LogRecord> for LogRow {
    fn from(l: &LogRecord) -> Self {
        let (attr_strs, attr_nums) = split_for_storage(&l.attributes);
        let a = &l.attributes;
        let r = &l.resource;
        Self {
            project_id: l.project_id.0,
            timestamp: l.timestamp,
            observed_timestamp: l.observed_timestamp,
            severity_number: l.severity.as_u8(),
            severity: l.severity.as_str().to_owned(),
            severity_text: l.severity_text.clone(),
            body: l.body.clone(),
            trace_id: l.trace_id.map(|t| t.to_hex()).unwrap_or_default(),
            span_id: l.span_id.map(|t| t.to_hex()).unwrap_or_default(),
            service_name: l.service_name.clone(),
            service_version: s(r, &[sc::SERVICE_VERSION]),
            deployment_env: s(r, &[sc::DEPLOYMENT_ENV, sc::DEPLOYMENT_ENV_LEGACY]),
            host_name: s(r, &[sc::HOST_NAME]),
            scope_name: l.scope_name.clone(),
            user_id: sc::first_string(a, &[sc::USER_ID, sc::ENDUSER_ID_LEGACY]).unwrap_or_default(),
            tenant_id: sc::first_string(a, &[sc::TENANT_ID]).unwrap_or_default(),
            request_id: s(a, &[sc::REQUEST_ID]),
            code_function: s(a, &[sc::CODE_FUNCTION, sc::CODE_FUNCTION_LEGACY]),
            resource: str_map(&l.resource),
            attrs: attr_strs.into_iter().collect(),
            attrs_num: attr_nums.into_iter().collect(),
        }
    }
}

impl From<LogRow> for LogRecord {
    fn from(r: LogRow) -> Self {
        LogRecord {
            project_id: ProjectId(r.project_id),
            timestamp: r.timestamp,
            observed_timestamp: r.observed_timestamp,
            severity: Severity::from_u8(r.severity_number),
            severity_text: r.severity_text,
            body: r.body,
            body_value: None,
            trace_id: TraceId::from_hex(&r.trace_id).filter(|t| !t.is_zero()),
            span_id: SpanId::from_hex(&r.span_id).filter(|t| !t.is_zero()),
            service_name: r.service_name,
            scope_name: r.scope_name,
            resource: from_str_map(r.resource),
            attributes: merge_attrs(r.attrs, &r.attrs_num),
        }
    }
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct MetricRow {
    #[serde(with = "clickhouse::serde::uuid")]
    pub project_id: Uuid,
    #[serde(with = "clickhouse::serde::chrono::datetime64::nanos")]
    pub timestamp: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::nanos")]
    pub start_timestamp: DateTime<Utc>,
    pub name: String,
    pub description: String,
    pub unit: String,
    pub kind: String,
    pub temporality: u8,
    pub is_monotonic: bool,
    pub service_name: String,
    pub scope_name: String,
    pub resource: StrMap,
    pub attrs: StrMap,
    pub value: f64,
    pub count: u64,
    pub sum: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub bucket_counts: Vec<u64>,
    pub explicit_bounds: Vec<f64>,
    pub exp_scale: i32,
    pub exp_zero_count: u64,
    pub exp_pos_offset: i32,
    pub exp_pos_counts: Vec<u64>,
    pub exp_neg_offset: i32,
    pub exp_neg_counts: Vec<u64>,
}

impl From<&MetricPoint> for MetricRow {
    fn from(m: &MetricPoint) -> Self {
        Self {
            project_id: m.project_id.0,
            timestamp: m.timestamp,
            start_timestamp: m.start_timestamp.unwrap_or(m.timestamp),
            name: m.name.clone(),
            description: m.description.clone(),
            unit: m.unit.clone(),
            kind: m.kind.as_str().to_owned(),
            temporality: m.temporality.as_u8(),
            is_monotonic: m.is_monotonic,
            service_name: m.service_name.clone(),
            scope_name: m.scope_name.clone(),
            resource: str_map(&m.resource),
            attrs: str_map(&m.attributes),
            value: m.value,
            count: m.count,
            sum: m.sum,
            min: m.min,
            max: m.max,
            bucket_counts: m.bucket_counts.clone(),
            explicit_bounds: m.explicit_bounds.clone(),
            exp_scale: m.exp_scale,
            exp_zero_count: m.exp_zero_count,
            exp_pos_offset: m.exp_pos_offset,
            exp_pos_counts: m.exp_pos_counts.clone(),
            exp_neg_offset: m.exp_neg_offset,
            exp_neg_counts: m.exp_neg_counts.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_span() -> Span {
        let mut attrs = Attributes::new();
        attrs.insert("http.request.method".into(), "GET".into());
        attrs.insert("http.route".into(), "/users/{id}".into());
        attrs.insert("http.response.status_code".into(), 404i64.into());
        attrs.insert("user.id".into(), "u-1".into());
        attrs.insert("retry".into(), true.into());
        attrs.insert("latency_ms".into(), 12.5f64.into());
        let mut res = Attributes::new();
        res.insert("service.name".into(), "api".into());
        res.insert("deployment.environment.name".into(), "prod".into());
        let start = Utc::now();
        Span {
            project_id: ProjectId::new(),
            trace_id: TraceId::random(),
            span_id: SpanId::random(),
            parent_span_id: Some(SpanId::random()),
            name: "GET /users/{id}".into(),
            kind: SpanKind::Server,
            start_time: start,
            end_time: start + chrono::Duration::milliseconds(12),
            status: SpanStatus { code: StatusCode::Error, message: "not found".into() },
            service_name: "api".into(),
            scope_name: "axum".into(),
            scope_version: "0.8".into(),
            resource: res,
            attributes: attrs,
            events: vec![SpanEvent {
                name: "exception".into(),
                timestamp: start,
                attributes: [("exception.type".to_string(), AttributeValue::from("NotFound"))].into_iter().collect(),
            }],
            links: vec![SpanLink { trace_id: TraceId::random(), span_id: SpanId::random(), attributes: Attributes::new() }],
        }
    }

    #[test]
    fn hot_columns_are_extracted() {
        let sp = sample_span();
        let row = SpanRow::from(&sp);
        assert_eq!(row.http_method, "GET");
        assert_eq!(row.http_route, "/users/{id}");
        assert_eq!(row.http_status_code, 404);
        assert_eq!(row.user_id, "u-1");
        assert_eq!(row.deployment_env, "prod");
        assert_eq!(row.exception_type, "NotFound");
        assert_eq!(row.duration_ns, 12_000_000);
        assert_eq!(row.attrs_num.get("http.response.status_code"), Some(&404.0));
        assert_eq!(row.attrs_num.get("retry"), Some(&1.0));
        assert_eq!(row.attrs.get("retry").map(String::as_str), Some("true"));
    }

    #[test]
    fn span_roundtrip_preserves_types() {
        let sp = sample_span();
        let back: Span = SpanRow::from(&sp).into();
        assert_eq!(back.trace_id, sp.trace_id);
        assert_eq!(back.parent_span_id, sp.parent_span_id);
        assert_eq!(back.kind, SpanKind::Server);
        assert_eq!(back.status.code, StatusCode::Error);
        assert_eq!(back.attributes["http.response.status_code"], AttributeValue::Int(404));
        assert_eq!(back.attributes["latency_ms"], AttributeValue::Float(12.5));
        // bools are stored numerically too, so they come back as Int(1); the string map says "true".
        assert!(matches!(back.attributes["retry"], AttributeValue::Int(1)));
        assert_eq!(back.events.len(), 1);
        assert_eq!(back.links.len(), 1);
        assert_eq!(back.events[0].attributes["exception.type"], AttributeValue::from("NotFound"));
    }
}

/// (operation, table) guessed from a SQL statement when the SDK did not send `db.operation.name`
/// / `db.collection.name`. Plain OpenTelemetry clients (pgx, database/sql, JDBC) only send the text.
pub fn sql_shape(stmt: &str) -> (String, String) {
    let mut rest = stmt.trim_start();
    // leading comments: /* ... */
    while let Some(after) = rest.strip_prefix("/*") {
        match after.find("*/") { Some(i) => rest = after[i + 2..].trim_start(), None => return (String::new(), String::new()) }
    }
    let mut words = rest.split_whitespace();
    let op = words.next().map(|w| w.trim_matches(|c: char| !c.is_ascii_alphabetic()).to_ascii_uppercase()).unwrap_or_default();
    let keyword = match op.as_str() { "SELECT" | "DELETE" => "FROM", "INSERT" => "INTO", "UPDATE" => "UPDATE", _ => return (op, String::new()) };
    let table = if keyword == "UPDATE" {
        words.next()
    } else {
        let mut it = rest.split_whitespace();
        let mut found = None;
        while let Some(w) = it.next() { if w.eq_ignore_ascii_case(keyword) { found = it.next(); break; } }
        found
    };
    let table = table.map(|t| t.trim_matches(|c: char| c == '"' || c == '`' || c == '(' || c == ')' || c == ',' || c == ';').to_string()).unwrap_or_default();
    (op, table)
}

#[cfg(test)]
mod sql_shape_tests {
    use super::sql_shape;
    #[test]
    fn guesses_operation_and_table() {
        assert_eq!(sql_shape("SELECT * FROM \"agenda_consulta\" WHERE id = $1"), ("SELECT".into(), "agenda_consulta".into()));
        assert_eq!(sql_shape("insert into pets (a) values ($1)"), ("INSERT".into(), "pets".into()));
        assert_eq!(sql_shape("UPDATE tenants SET x = 1"), ("UPDATE".into(), "tenants".into()));
        assert_eq!(sql_shape("DELETE FROM sessions WHERE t < now()"), ("DELETE".into(), "sessions".into()));
        assert_eq!(sql_shape("/* hint */ SELECT 1"), ("SELECT".into(), String::new()));
        assert_eq!(sql_shape("BEGIN"), ("BEGIN".into(), String::new()));
    }
}
