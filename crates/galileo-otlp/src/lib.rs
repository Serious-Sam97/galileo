//! OTLP receivers. Apps send OpenTelemetry data here over gRPC (:4317) or HTTP (:4318); we
//! authenticate the API key, convert to domain types, redact, and hand rows to the batching
//! writer.

pub mod auth;
pub mod convert;
pub mod grpc;
pub mod http;
pub mod sampling;
pub mod writer;

pub use auth::{ApiKeyResolver, ProjectContext, StaticResolver};
pub use writer::{BatchWriter, IngestStats, WriterHandle};

/// Header names accepted for the project API key.
pub const HEADER_AUTHORIZATION: &str = "authorization";
pub const HEADER_API_KEY: &str = "x-galileo-key";

/// Extract a raw API key from either `Authorization: Bearer <key>` or `x-galileo-key: <key>`.
pub fn extract_key(auth_header: Option<&str>, key_header: Option<&str>) -> Option<String> {
    if let Some(k) = key_header {
        let k = k.trim();
        if !k.is_empty() {
            return Some(k.to_owned());
        }
    }
    let a = auth_header?.trim();
    let (scheme, rest) = a.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") {
        let rest = rest.trim();
        if !rest.is_empty() {
            return Some(rest.to_owned());
        }
    }
    None
}

/// Run the project's log pipeline over converted records and derive log-based metric points.
/// Returns the kept records and the metric points to write.
pub fn apply_pipeline(ctx: &ProjectContext, logs: Vec<galileo_core::LogRecord>) -> (Vec<galileo_core::LogRecord>, Vec<galileo_core::MetricPoint>) {
    use galileo_core::metric::{AggregationTemporality, MetricKind};
    let mut kept = Vec::with_capacity(logs.len());
    let mut points = vec![];
    for mut l in logs {
        if !ctx.pipeline.apply(&mut l) { continue; }
        for m in ctx.log_metrics.iter() {
            if let Some(v) = m.value(&l) {
                let mut attrs = galileo_core::Attributes::default();
                attrs.insert("service.name".into(), galileo_core::AttributeValue::Str(l.service_name.clone()));
                attrs.insert("severity".into(), galileo_core::AttributeValue::Str(format!("{:?}", l.severity).to_lowercase()));
                points.push(galileo_core::MetricPoint {
                    project_id: ctx.project_id, name: format!("log.{}", m.name), description: String::new(), unit: m.unit.clone(), kind: MetricKind::Gauge,
                    temporality: AggregationTemporality::Unspecified, is_monotonic: false, timestamp: l.timestamp, start_timestamp: None,
                    service_name: l.service_name.clone(), scope_name: "galileo-log-metrics".into(), resource: galileo_core::Attributes::default(), attributes: attrs,
                    value: v, count: 0, sum: 0.0, min: None, max: None, bucket_counts: vec![], explicit_bounds: vec![],
                    exp_scale: 0, exp_zero_count: 0, exp_pos_offset: 0, exp_pos_counts: vec![], exp_neg_offset: 0, exp_neg_counts: vec![],
                });
            }
        }
        kept.push(l);
    }
    (kept, points)
}

/// Quota check before writing: Some(limit) when the hard quota is exceeded.
pub fn quota_exceeded(ctx: &ProjectContext, stats: &IngestStats, signal: &'static str, n: usize) -> Option<u64> {
    let limit = ctx.quotas.limit(signal)?;
    let total = stats.quota_add(ctx.project_id.0, signal, n as u64);
    (ctx.quotas.mode == "hard" && total > limit).then_some(limit)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_extraction() {
        assert_eq!(extract_key(Some("Bearer abc"), None).as_deref(), Some("abc"));
        assert_eq!(extract_key(Some("bearer  abc "), None).as_deref(), Some("abc"));
        assert_eq!(extract_key(Some("Basic abc"), None), None);
        assert_eq!(extract_key(None, Some("k1")).as_deref(), Some("k1"));
        assert_eq!(extract_key(Some("Bearer a"), Some("k1")).as_deref(), Some("k1"));
        assert_eq!(extract_key(None, None), None);
    }
}
