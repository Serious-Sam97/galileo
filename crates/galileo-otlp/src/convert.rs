//! OTLP protobuf → Galileo domain types. Pure functions; redaction is applied here too so
//! nothing un-redacted ever leaves this module.

use chrono::{DateTime, TimeZone, Utc};
use galileo_core::semconv as sc;
use galileo_core::{
    AggregationTemporality, AttributeValue, Attributes, LogRecord, MetricKind, MetricPoint, ProjectId,
    Redactor, Severity, Span, SpanEvent, SpanId, SpanKind, SpanLink, SpanStatus, StatusCode, TraceId,
};
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, InstrumentationScope, KeyValue};
use opentelemetry_proto::tonic::logs::v1::ResourceLogs;
use opentelemetry_proto::tonic::metrics::v1::{metric::Data, number_data_point, ResourceMetrics};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::ResourceSpans;

pub fn any_value(v: AnyValue) -> Option<AttributeValue> {
    Some(match v.value? {
        any_value::Value::StringValue(s) => AttributeValue::Str(s),
        any_value::Value::BoolValue(b) => AttributeValue::Bool(b),
        any_value::Value::IntValue(i) => AttributeValue::Int(i),
        any_value::Value::DoubleValue(d) => AttributeValue::Float(d),
        any_value::Value::BytesValue(b) => AttributeValue::Bytes(b),
        any_value::Value::ArrayValue(a) => {
            AttributeValue::Array(a.values.into_iter().filter_map(any_value).collect())
        }
        any_value::Value::KvlistValue(kv) => AttributeValue::Map(
            kv.values
                .into_iter()
                .filter_map(|kv| Some((kv.key, any_value(kv.value?)?)))
                .collect(),
        ),
        // Dictionary-encoded strings (OTLP 1.9 string tables) are not emitted by any SDK yet;
        // drop rather than store a meaningless index.
        #[allow(unreachable_patterns)]
        _ => return None,
    })
}

pub fn attributes(kvs: Vec<KeyValue>) -> Attributes {
    kvs.into_iter()
        .filter_map(|kv| Some((kv.key, any_value(kv.value?)?)))
        .collect()
}

fn resource_attrs(r: Option<Resource>) -> Attributes {
    r.map(|r| attributes(r.attributes)).unwrap_or_default()
}

fn service_name(resource: &Attributes) -> String {
    resource
        .get(sc::SERVICE_NAME)
        .and_then(|v| v.as_str())
        .unwrap_or("unknown_service")
        .to_owned()
}

fn scope_parts(scope: Option<InstrumentationScope>) -> (String, String, Attributes) {
    match scope {
        Some(s) => (s.name, s.version, attributes(s.attributes)),
        None => Default::default(),
    }
}

pub fn nanos_to_dt(n: u64) -> DateTime<Utc> {
    if n == 0 {
        return Utc::now();
    }
    Utc.timestamp_nanos(n.min(i64::MAX as u64) as i64)
}

fn trace_id(b: &[u8]) -> TraceId {
    TraceId::from_bytes(b).unwrap_or(TraceId::ZERO)
}
fn span_id(b: &[u8]) -> SpanId {
    SpanId::from_bytes(b).unwrap_or(SpanId::ZERO)
}

/// Convert a whole export request worth of spans. Invalid spans (zero ids) are skipped and
/// counted in the returned `rejected`.
pub fn spans(project_id: ProjectId, redactor: &Redactor, rs: Vec<ResourceSpans>) -> (Vec<Span>, usize) {
    let mut out = Vec::new();
    let mut rejected = 0usize;
    for r in rs {
        let mut resource = resource_attrs(r.resource);
        redactor.redact_attrs(&mut resource);
        let service = service_name(&resource);
        for ss in r.scope_spans {
            let (scope_name, scope_version, _scope_attrs) = scope_parts(ss.scope);
            for s in ss.spans {
                let tid = trace_id(&s.trace_id);
                let sid = span_id(&s.span_id);
                if tid.is_zero() || sid.is_zero() {
                    rejected += 1;
                    continue;
                }
                let mut attrs = attributes(s.attributes);
                redactor.redact_attrs(&mut attrs);
                let events = s
                    .events
                    .into_iter()
                    .map(|e| {
                        let mut a = attributes(e.attributes);
                        redactor.redact_attrs(&mut a);
                        SpanEvent { name: e.name, timestamp: nanos_to_dt(e.time_unix_nano), attributes: a }
                    })
                    .collect();
                let links = s
                    .links
                    .into_iter()
                    .filter_map(|l| {
                        let t = trace_id(&l.trace_id);
                        let sp = span_id(&l.span_id);
                        if t.is_zero() || sp.is_zero() {
                            return None;
                        }
                        let mut a = attributes(l.attributes);
                        redactor.redact_attrs(&mut a);
                        Some(SpanLink { trace_id: t, span_id: sp, attributes: a })
                    })
                    .collect();
                let start = nanos_to_dt(s.start_time_unix_nano);
                let end = if s.end_time_unix_nano == 0 { start } else { nanos_to_dt(s.end_time_unix_nano) };
                let status = s
                    .status
                    .map(|st| SpanStatus {
                        code: StatusCode::from_u8(st.code as u8),
                        message: redactor.redact_text(&st.message),
                    })
                    .unwrap_or_default();
                out.push(Span {
                    project_id,
                    trace_id: tid,
                    span_id: sid,
                    parent_span_id: SpanId::from_bytes(&s.parent_span_id).filter(|p| !p.is_zero()),
                    name: s.name,
                    kind: SpanKind::from_u8(s.kind as u8),
                    start_time: start,
                    end_time: end.max(start),
                    status,
                    service_name: service.clone(),
                    scope_name: scope_name.clone(),
                    scope_version: scope_version.clone(),
                    resource: resource.clone(),
                    attributes: attrs,
                    events,
                    links,
                });
            }
        }
    }
    (out, rejected)
}

pub fn logs(project_id: ProjectId, redactor: &Redactor, rl: Vec<ResourceLogs>) -> (Vec<LogRecord>, usize) {
    let mut out = Vec::new();
    for r in rl {
        let mut resource = resource_attrs(r.resource);
        redactor.redact_attrs(&mut resource);
        let service = service_name(&resource);
        for sl in r.scope_logs {
            let (scope_name, _v, _a) = scope_parts(sl.scope);
            for l in sl.log_records {
                let mut attrs = attributes(l.attributes);
                redactor.redact_attrs(&mut attrs);
                let body_value = l.body.and_then(any_value).map(|mut v| {
                    if let AttributeValue::Str(s) = &mut v {
                        *s = redactor.redact_text(s);
                    } else if let AttributeValue::Map(_) | AttributeValue::Array(_) = &v {
                        // structured bodies: redact string leaves through the attrs path
                        let mut tmp = Attributes::new();
                        tmp.insert("body".into(), v.clone());
                        redactor.redact_attrs(&mut tmp);
                        if let Some(b) = tmp.shift_remove("body") {
                            v = b;
                        }
                    }
                    v
                });
                let body = body_value.as_ref().map(|v| v.to_string_repr()).unwrap_or_default();
                let severity = if l.severity_number != 0 {
                    Severity::from_otlp_number(l.severity_number)
                } else {
                    Severity::from_text(&l.severity_text)
                };
                let ts = if l.time_unix_nano != 0 { l.time_unix_nano } else { l.observed_time_unix_nano };
                out.push(LogRecord {
                    project_id,
                    timestamp: nanos_to_dt(ts),
                    observed_timestamp: nanos_to_dt(l.observed_time_unix_nano),
                    severity,
                    severity_text: l.severity_text,
                    body,
                    body_value,
                    trace_id: TraceId::from_bytes(&l.trace_id).filter(|t| !t.is_zero()),
                    span_id: SpanId::from_bytes(&l.span_id).filter(|t| !t.is_zero()),
                    service_name: service.clone(),
                    scope_name: scope_name.clone(),
                    resource: resource.clone(),
                    attributes: attrs,
                });
            }
        }
    }
    (out, 0)
}

fn num_value(v: Option<number_data_point::Value>) -> f64 {
    match v {
        Some(number_data_point::Value::AsDouble(d)) => d,
        Some(number_data_point::Value::AsInt(i)) => i as f64,
        None => 0.0,
    }
}

pub fn metrics(project_id: ProjectId, redactor: &Redactor, rm: Vec<ResourceMetrics>) -> (Vec<MetricPoint>, usize) {
    let mut out = Vec::new();
    let mut rejected = 0usize;
    for r in rm {
        let mut resource = resource_attrs(r.resource);
        redactor.redact_attrs(&mut resource);
        let service = service_name(&resource);
        for sm in r.scope_metrics {
            let (scope_name, _v, _a) = scope_parts(sm.scope);
            for m in sm.metrics {
                let base = |kind: MetricKind, temporality: i32, monotonic: bool, ts: u64, start: u64, attrs: Attributes| {
                    MetricPoint {
                        project_id,
                        name: m.name.clone(),
                        description: m.description.clone(),
                        unit: m.unit.clone(),
                        kind,
                        temporality: AggregationTemporality::from_u8(temporality as u8),
                        is_monotonic: monotonic,
                        timestamp: nanos_to_dt(ts),
                        start_timestamp: (start != 0).then(|| nanos_to_dt(start)),
                        service_name: service.clone(),
                        scope_name: scope_name.clone(),
                        resource: resource.clone(),
                        attributes: attrs,
                        value: 0.0,
                        count: 0,
                        sum: 0.0,
                        min: None,
                        max: None,
                        bucket_counts: vec![],
                        explicit_bounds: vec![],
                        exp_scale: 0, exp_zero_count: 0, exp_pos_offset: 0, exp_pos_counts: vec![], exp_neg_offset: 0, exp_neg_counts: vec![],
                    }
                };
                match m.data.clone() {
                    Some(Data::Gauge(g)) => {
                        for p in g.data_points {
                            let mut a = attributes(p.attributes);
                            redactor.redact_attrs(&mut a);
                            let mut mp = base(MetricKind::Gauge, 0, false, p.time_unix_nano, p.start_time_unix_nano, a);
                            mp.value = num_value(p.value);
                            out.push(mp);
                        }
                    }
                    Some(Data::Sum(s)) => {
                        for p in s.data_points {
                            let mut a = attributes(p.attributes);
                            redactor.redact_attrs(&mut a);
                            let mut mp = base(
                                MetricKind::Sum,
                                s.aggregation_temporality,
                                s.is_monotonic,
                                p.time_unix_nano,
                                p.start_time_unix_nano,
                                a,
                            );
                            mp.value = num_value(p.value);
                            out.push(mp);
                        }
                    }
                    Some(Data::Histogram(h)) => {
                        for p in h.data_points {
                            let mut a = attributes(p.attributes);
                            redactor.redact_attrs(&mut a);
                            let mut mp = base(
                                MetricKind::Histogram,
                                h.aggregation_temporality,
                                false,
                                p.time_unix_nano,
                                p.start_time_unix_nano,
                                a,
                            );
                            mp.count = p.count;
                            mp.sum = p.sum.unwrap_or(0.0);
                            mp.min = p.min;
                            mp.max = p.max;
                            mp.bucket_counts = p.bucket_counts;
                            mp.explicit_bounds = p.explicit_bounds;
                            out.push(mp);
                        }
                    }
                    Some(Data::ExponentialHistogram(h)) => {
                        // Raw buckets are kept (scale/offset/counts) and also expanded into explicit
                        // bounds so heatmaps and per-point quantiles work like explicit histograms.
                        for p in h.data_points {
                            let mut a = attributes(p.attributes);
                            redactor.redact_attrs(&mut a);
                            let mut mp = base(
                                MetricKind::ExponentialHistogram,
                                h.aggregation_temporality,
                                false,
                                p.time_unix_nano,
                                p.start_time_unix_nano,
                                a,
                            );
                            mp.count = p.count;
                            mp.sum = p.sum.unwrap_or(0.0);
                            mp.min = p.min;
                            mp.max = p.max;
                            mp.exp_scale = p.scale;
                            mp.exp_zero_count = p.zero_count;
                            let (pos_off, pos_counts) = p.positive.map(|b| (b.offset, b.bucket_counts)).unwrap_or((0, vec![]));
                            let (neg_off, neg_counts) = p.negative.map(|b| (b.offset, b.bucket_counts)).unwrap_or((0, vec![]));
                            let neg_total: u64 = neg_counts.iter().sum();
                            let (bounds, counts) = galileo_core::metric::expand_exponential(p.scale, p.zero_count, pos_off, &pos_counts, neg_total);
                            mp.explicit_bounds = bounds;
                            mp.bucket_counts = counts;
                            mp.exp_pos_offset = pos_off;
                            mp.exp_pos_counts = pos_counts;
                            mp.exp_neg_offset = neg_off;
                            mp.exp_neg_counts = neg_counts;
                            out.push(mp);
                        }
                    }
                    Some(Data::Summary(s)) => {
                        for p in s.data_points {
                            let mut a = attributes(p.attributes);
                            redactor.redact_attrs(&mut a);
                            let mut mp = base(MetricKind::Summary, 0, false, p.time_unix_nano, p.start_time_unix_nano, a);
                            mp.count = p.count;
                            mp.sum = p.sum;
                            // quantiles as bounds/counts pairs: bounds = quantile, counts = value*1e6 is lossy;
                            // keep them as attributes instead so they stay exact.
                            for q in p.quantile_values {
                                mp.attributes.insert(format!("quantile.{}", q.quantile), AttributeValue::Float(q.value));
                            }
                            out.push(mp);
                        }
                    }
                    None => rejected += 1,
                }
            }
        }
    }
    (out, rejected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_proto::tonic::logs::v1 as logsv1;
    use opentelemetry_proto::tonic::metrics::v1 as metricsv1;
    use opentelemetry_proto::tonic::trace::v1 as tracev1;

    fn kv(k: &str, v: any_value::Value) -> KeyValue {
        KeyValue { key: k.into(), value: Some(AnyValue { value: Some(v) }), ..Default::default() }
    }
    fn s(v: &str) -> any_value::Value {
        any_value::Value::StringValue(v.into())
    }

    fn resource() -> Resource {
        Resource {
            attributes: vec![kv("service.name", s("api")), kv("db.password", s("x"))],
            dropped_attributes_count: 0,
            entity_refs: vec![],
        }
    }

    #[test]
    fn converts_and_redacts_spans() {
        let red = Redactor::with_defaults(&[]).unwrap();
        let pid = ProjectId::new();
        let rs = vec![ResourceSpans {
            resource: Some(resource()),
            scope_spans: vec![tracev1::ScopeSpans {
                scope: Some(InstrumentationScope { name: "lib".into(), version: "1".into(), attributes: vec![], dropped_attributes_count: 0 }),
                spans: vec![
                    tracev1::Span {
                        trace_id: vec![1; 16],
                        span_id: vec![2; 8],
                        parent_span_id: vec![],
                        name: "GET /".into(),
                        kind: 2,
                        start_time_unix_nano: 1_700_000_000_000_000_000,
                        end_time_unix_nano: 1_700_000_000_500_000_000,
                        attributes: vec![
                            kv("http.status_code", any_value::Value::IntValue(200)),
                            kv("note", s("Bearer sekret")),
                        ],
                        status: Some(tracev1::Status { code: 2, message: "boom".into() }),
                        events: vec![tracev1::span::Event {
                            time_unix_nano: 1_700_000_000_100_000_000,
                            name: "exception".into(),
                            attributes: vec![kv("exception.type", s("Boom"))],
                            dropped_attributes_count: 0,
                        }],
                        links: vec![],
                        ..Default::default()
                    },
                    tracev1::Span { trace_id: vec![], span_id: vec![3; 8], ..Default::default() },
                ],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }];
        let (spans, rejected) = spans(pid, &red, rs);
        assert_eq!(rejected, 1);
        assert_eq!(spans.len(), 1);
        let sp = &spans[0];
        assert_eq!(sp.service_name, "api");
        assert!(sp.resource.get("db.password").is_none(), "resource attrs are redacted");
        assert_eq!(sp.kind, SpanKind::Server);
        assert_eq!(sp.status.code, StatusCode::Error);
        assert_eq!(sp.duration_ns(), 500_000_000);
        assert_eq!(sp.attributes["http.status_code"], AttributeValue::Int(200));
        assert_eq!(sp.attributes["note"].as_str(), Some("Bearer [REDACTED]"));
        assert_eq!(sp.events[0].attributes["exception.type"].as_str(), Some("Boom"));
        assert!(sp.parent_span_id.is_none());
    }

    #[test]
    fn converts_logs() {
        let red = Redactor::empty();
        let rl = vec![ResourceLogs {
            resource: Some(resource()),
            scope_logs: vec![logsv1::ScopeLogs {
                scope: None,
                log_records: vec![logsv1::LogRecord {
                    time_unix_nano: 0,
                    observed_time_unix_nano: 1_700_000_000_000_000_000,
                    severity_number: 13,
                    severity_text: "WARN".into(),
                    body: Some(AnyValue { value: Some(s("careful")) }),
                    attributes: vec![kv("k", s("v"))],
                    trace_id: vec![1; 16],
                    span_id: vec![2; 8],
                    ..Default::default()
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }];
        let (logs, _) = logs(ProjectId::new(), &red, rl);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].severity, Severity::Warn);
        assert_eq!(logs[0].body, "careful");
        assert_eq!(logs[0].timestamp, logs[0].observed_timestamp);
        assert!(logs[0].trace_id.is_some());
    }

    #[test]
    fn converts_metrics() {
        let red = Redactor::empty();
        let rm = vec![ResourceMetrics {
            resource: Some(resource()),
            scope_metrics: vec![metricsv1::ScopeMetrics {
                scope: None,
                metrics: vec![
                    metricsv1::Metric {
                        name: "cpu".into(),
                        description: String::new(),
                        unit: "1".into(),
                        metadata: vec![],
                        data: Some(Data::Gauge(metricsv1::Gauge {
                            data_points: vec![metricsv1::NumberDataPoint {
                                attributes: vec![kv("core", s("0"))],
                                time_unix_nano: 1,
                                value: Some(number_data_point::Value::AsDouble(0.5)),
                                ..Default::default()
                            }],
                        })),
                    },
                    metricsv1::Metric {
                        name: "latency".into(),
                        description: String::new(),
                        unit: "ms".into(),
                        metadata: vec![],
                        data: Some(Data::Histogram(metricsv1::Histogram {
                            data_points: vec![metricsv1::HistogramDataPoint {
                                count: 3,
                                sum: Some(30.0),
                                bucket_counts: vec![1, 2],
                                explicit_bounds: vec![10.0],
                                time_unix_nano: 1,
                                ..Default::default()
                            }],
                            aggregation_temporality: 2,
                        })),
                    },
                ],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }];
        let (pts, rejected) = metrics(ProjectId::new(), &red, rm);
        assert_eq!(rejected, 0);
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].kind, MetricKind::Gauge);
        assert_eq!(pts[0].value, 0.5);
        assert_eq!(pts[1].kind, MetricKind::Histogram);
        assert_eq!(pts[1].count, 3);
        assert_eq!(pts[1].temporality, AggregationTemporality::Cumulative);
    }
}
