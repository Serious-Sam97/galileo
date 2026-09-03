//! Field catalog: which names map to real columns per dataset, which are aliases for
//! semantic-convention attribute names, and how everything else falls through to the
//! attribute maps.

use serde::Serialize;

use crate::{Dataset, QueryError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Number,
    Timestamp,
    Bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldInfo {
    pub name: String,
    pub ty: FieldType,
    /// True for real columns, false for attribute-map keys discovered from data.
    pub column: bool,
}

/// A resolved field: SQL for reading it as a string and, when meaningful, as a number.
#[derive(Debug, Clone)]
pub struct FieldExpr {
    pub name: String,
    pub ty: FieldType,
    /// Expression usable in string comparisons and GROUP BY.
    pub str_sql: String,
    /// Expression usable in numeric aggregates; NULL when absent so aggregates skip it.
    pub num_sql: String,
    /// Expression that is true when the field is present on the row.
    pub exists_sql: String,
    pub is_column: bool,
}

struct Col {
    name: &'static str,
    ty: FieldType,
    /// Semantic-convention aliases users may type.
    aliases: &'static [&'static str],
}

const COMMON_TAIL: &[Col] = &[
    Col { name: "service_name", ty: FieldType::String, aliases: &["service.name", "service"] },
    Col { name: "service_version", ty: FieldType::String, aliases: &["service.version"] },
    Col { name: "deployment_env", ty: FieldType::String, aliases: &["deployment.environment.name", "deployment.environment", "env"] },
    Col { name: "host_name", ty: FieldType::String, aliases: &["host.name", "host"] },
    Col { name: "scope_name", ty: FieldType::String, aliases: &["otel.scope.name", "scope"] },
];

const SPAN_COLS: &[Col] = &[
    Col { name: "timestamp", ty: FieldType::Timestamp, aliases: &["start_time"] },
    Col { name: "end_time", ty: FieldType::Timestamp, aliases: &[] },
    Col { name: "duration_ns", ty: FieldType::Number, aliases: &[] },
    Col { name: "trace_id", ty: FieldType::String, aliases: &["trace.trace_id"] },
    Col { name: "span_id", ty: FieldType::String, aliases: &["trace.span_id"] },
    Col { name: "parent_span_id", ty: FieldType::String, aliases: &["trace.parent_id"] },
    Col { name: "name", ty: FieldType::String, aliases: &["span.name"] },
    Col { name: "kind", ty: FieldType::String, aliases: &["span.kind"] },
    Col { name: "status_code", ty: FieldType::String, aliases: &["status", "otel.status_code"] },
    Col { name: "status_message", ty: FieldType::String, aliases: &["otel.status_description"] },
    Col { name: "scope_version", ty: FieldType::String, aliases: &[] },
    Col { name: "http_method", ty: FieldType::String, aliases: &["http.request.method", "http.method"] },
    Col { name: "http_route", ty: FieldType::String, aliases: &["http.route"] },
    Col { name: "http_status_code", ty: FieldType::Number, aliases: &["http.response.status_code", "http.status_code"] },
    Col { name: "url_path", ty: FieldType::String, aliases: &["url.path", "http.target"] },
    Col { name: "db_system", ty: FieldType::String, aliases: &["db.system"] },
    Col { name: "db_operation", ty: FieldType::String, aliases: &["db.operation", "db.operation.name"] },
    Col { name: "db_table", ty: FieldType::String, aliases: &["db.table", "db.collection.name"] },
    Col { name: "user_id", ty: FieldType::String, aliases: &["user.id", "enduser.id"] },
    Col { name: "tenant_id", ty: FieldType::String, aliases: &["tenant.id"] },
    Col { name: "request_id", ty: FieldType::String, aliases: &["request.id"] },
    Col { name: "exception_type", ty: FieldType::String, aliases: &["exception.type"] },
    Col { name: "exception_message", ty: FieldType::String, aliases: &["exception.message"] },
    Col { name: "exception_culprit", ty: FieldType::String, aliases: &["exception.culprit"] },
    Col { name: "exception_fingerprint", ty: FieldType::String, aliases: &["exception.fingerprint", "issue.fingerprint"] },
    Col { name: "code_function", ty: FieldType::String, aliases: &["code.function.name", "code.function"] },
    Col { name: "code_namespace", ty: FieldType::String, aliases: &["code.namespace"] },
    Col { name: "code_file", ty: FieldType::String, aliases: &["code.file.path", "code.filepath"] },
    Col { name: "code_line", ty: FieldType::Number, aliases: &["code.line.number", "code.lineno"] },
    Col { name: "gen_ai_system", ty: FieldType::String, aliases: &["gen_ai.system"] },
    Col { name: "gen_ai_model", ty: FieldType::String, aliases: &["gen_ai.response.model", "gen_ai.request.model", "model"] },
    Col { name: "gen_ai_input_tokens", ty: FieldType::Number, aliases: &["gen_ai.usage.input_tokens"] },
    Col { name: "gen_ai_output_tokens", ty: FieldType::Number, aliases: &["gen_ai.usage.output_tokens"] },
    Col { name: "gen_ai_cost_usd", ty: FieldType::Number, aliases: &["gen_ai.usage.cost_usd", "cost_usd"] },
];

const LOG_COLS: &[Col] = &[
    Col { name: "timestamp", ty: FieldType::Timestamp, aliases: &[] },
    Col { name: "observed_timestamp", ty: FieldType::Timestamp, aliases: &[] },
    Col { name: "severity_number", ty: FieldType::Number, aliases: &[] },
    Col { name: "severity", ty: FieldType::String, aliases: &["level", "severity_text_normalized"] },
    Col { name: "severity_text", ty: FieldType::String, aliases: &[] },
    Col { name: "body", ty: FieldType::String, aliases: &["message", "msg"] },
    Col { name: "trace_id", ty: FieldType::String, aliases: &["trace.trace_id"] },
    Col { name: "span_id", ty: FieldType::String, aliases: &["trace.span_id"] },
    Col { name: "user_id", ty: FieldType::String, aliases: &["user.id", "enduser.id"] },
    Col { name: "tenant_id", ty: FieldType::String, aliases: &["tenant.id"] },
    Col { name: "request_id", ty: FieldType::String, aliases: &["request.id"] },
    Col { name: "code_function", ty: FieldType::String, aliases: &["code.function.name", "code.function"] },
];

const METRIC_COLS: &[Col] = &[
    Col { name: "timestamp", ty: FieldType::Timestamp, aliases: &[] },
    Col { name: "name", ty: FieldType::String, aliases: &["metric", "metric.name"] },
    Col { name: "unit", ty: FieldType::String, aliases: &[] },
    Col { name: "kind", ty: FieldType::String, aliases: &["metric.kind"] },
    Col { name: "value", ty: FieldType::Number, aliases: &[] },
    Col { name: "count", ty: FieldType::Number, aliases: &[] },
    Col { name: "sum", ty: FieldType::Number, aliases: &[] },
    Col { name: "min", ty: FieldType::Number, aliases: &[] },
    Col { name: "max", ty: FieldType::Number, aliases: &[] },
];

/// Virtual fields computed from columns.
struct Virtual {
    name: &'static str,
    datasets: &'static [Dataset],
    sql: &'static str,
    ty: FieldType,
}

const VIRTUALS: &[Virtual] = &[
    Virtual { name: "duration_ms", datasets: &[Dataset::Spans], sql: "duration_ns / 1000000", ty: FieldType::Number },
    Virtual { name: "duration_s", datasets: &[Dataset::Spans], sql: "duration_ns / 1000000000", ty: FieldType::Number },
    Virtual { name: "is_root", datasets: &[Dataset::Spans], sql: "parent_span_id = ''", ty: FieldType::Bool },
    Virtual { name: "is_error", datasets: &[Dataset::Spans], sql: "status_code = 'error'", ty: FieldType::Bool },
    Virtual { name: "gen_ai_total_tokens", datasets: &[Dataset::Spans], sql: "gen_ai_input_tokens + gen_ai_output_tokens", ty: FieldType::Number },
    Virtual { name: "mean", datasets: &[Dataset::Metrics], sql: "if(count > 0, sum / count, value)", ty: FieldType::Number },
    // per-point quantile estimated from histogram buckets (explicit or expanded exponential),
    // linear interpolation inside the bucket; AVG/MAX over points gives the series
    Virtual { name: "hist_p50", datasets: &[Dataset::Metrics], sql: "if(length(bucket_counts) = 0 OR count = 0, NULL, (arrayFirstIndex(x -> x >= 0.5 * count, arrayCumSum(bucket_counts)) AS hi_0_5) * 0 + if(hi_0_5 <= 1, coalesce(min, 0.0), if(hi_0_5 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_5 - 1])) + (if(hi_0_5 <= 1, explicit_bounds[1], if(hi_0_5 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_5])) - if(hi_0_5 <= 1, coalesce(min, 0.0), if(hi_0_5 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_5 - 1]))) * if(bucket_counts[hi_0_5] = 0, 0.0, (0.5 * count - if(hi_0_5 <= 1, 0, arrayCumSum(bucket_counts)[hi_0_5 - 1])) / bucket_counts[hi_0_5]))", ty: FieldType::Number },
    Virtual { name: "hist_p90", datasets: &[Dataset::Metrics], sql: "if(length(bucket_counts) = 0 OR count = 0, NULL, (arrayFirstIndex(x -> x >= 0.9 * count, arrayCumSum(bucket_counts)) AS hi_0_9) * 0 + if(hi_0_9 <= 1, coalesce(min, 0.0), if(hi_0_9 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_9 - 1])) + (if(hi_0_9 <= 1, explicit_bounds[1], if(hi_0_9 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_9])) - if(hi_0_9 <= 1, coalesce(min, 0.0), if(hi_0_9 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_9 - 1]))) * if(bucket_counts[hi_0_9] = 0, 0.0, (0.9 * count - if(hi_0_9 <= 1, 0, arrayCumSum(bucket_counts)[hi_0_9 - 1])) / bucket_counts[hi_0_9]))", ty: FieldType::Number },
    Virtual { name: "hist_p95", datasets: &[Dataset::Metrics], sql: "if(length(bucket_counts) = 0 OR count = 0, NULL, (arrayFirstIndex(x -> x >= 0.95 * count, arrayCumSum(bucket_counts)) AS hi_0_95) * 0 + if(hi_0_95 <= 1, coalesce(min, 0.0), if(hi_0_95 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_95 - 1])) + (if(hi_0_95 <= 1, explicit_bounds[1], if(hi_0_95 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_95])) - if(hi_0_95 <= 1, coalesce(min, 0.0), if(hi_0_95 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_95 - 1]))) * if(bucket_counts[hi_0_95] = 0, 0.0, (0.95 * count - if(hi_0_95 <= 1, 0, arrayCumSum(bucket_counts)[hi_0_95 - 1])) / bucket_counts[hi_0_95]))", ty: FieldType::Number },
    Virtual { name: "hist_p99", datasets: &[Dataset::Metrics], sql: "if(length(bucket_counts) = 0 OR count = 0, NULL, (arrayFirstIndex(x -> x >= 0.99 * count, arrayCumSum(bucket_counts)) AS hi_0_99) * 0 + if(hi_0_99 <= 1, coalesce(min, 0.0), if(hi_0_99 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_99 - 1])) + (if(hi_0_99 <= 1, explicit_bounds[1], if(hi_0_99 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_99])) - if(hi_0_99 <= 1, coalesce(min, 0.0), if(hi_0_99 > length(explicit_bounds), coalesce(max, explicit_bounds[length(explicit_bounds)]), explicit_bounds[hi_0_99 - 1]))) * if(bucket_counts[hi_0_99] = 0, 0.0, (0.99 * count - if(hi_0_99 <= 1, 0, arrayCumSum(bucket_counts)[hi_0_99 - 1])) / bucket_counts[hi_0_99]))", ty: FieldType::Number },

];

fn columns(ds: Dataset) -> impl Iterator<Item = &'static Col> {
    let base: &'static [Col] = match ds {
        Dataset::Spans => SPAN_COLS,
        Dataset::Logs => LOG_COLS,
        Dataset::Metrics => METRIC_COLS,
    };
    let tail: &'static [Col] = match ds {
        Dataset::Metrics => &COMMON_TAIL[..1],
        _ => COMMON_TAIL,
    };
    base.iter().chain(tail.iter())
}

/// Field names are user input that ends up inside SQL string literals (map keys) or as
/// identifiers; keep them to a conservative charset so quoting is never a question.
pub fn validate_name(name: &str) -> Result<()> {
    let n = name.trim();
    if n.is_empty() || n.len() > 200 {
        return Err(QueryError::Invalid("field name must be 1..200 chars".into()));
    }
    if !n.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/' | '@' | '(' | ')' | ' ')) {
        return Err(QueryError::Invalid(format!("invalid characters in field name '{n}'")));
    }
    Ok(())
}

fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Resolve a user-facing field name into SQL for the dataset.
pub fn resolve(ds: Dataset, name: &str) -> Result<FieldExpr> {
    validate_name(name)?;
    let name = name.trim();

    for v in VIRTUALS.iter().filter(|v| v.datasets.contains(&ds)) {
        if v.name == name {
            let num = match v.ty {
                FieldType::Bool => format!("toUInt8({})", v.sql),
                _ => format!("({})", v.sql),
            };
            return Ok(FieldExpr {
                name: name.into(),
                ty: v.ty,
                str_sql: format!("toString({})", v.sql),
                num_sql: num,
                exists_sql: "1".into(),
                is_column: true,
            });
        }
    }

    for c in columns(ds) {
        if c.name == name || c.aliases.contains(&name) {
            let (str_sql, num_sql) = match c.ty {
                FieldType::String => (c.name.to_string(), format!("toFloat64OrNull({})", c.name)),
                FieldType::Number => (format!("toString({})", c.name), format!("toFloat64({})", c.name)),
                FieldType::Timestamp => (format!("toString({})", c.name), format!("toUnixTimestamp64Nano({})", c.name)),
                FieldType::Bool => (format!("toString({})", c.name), format!("toUInt8({})", c.name)),
            };
            let exists_sql = match c.ty {
                FieldType::String => format!("{} != ''", c.name),
                FieldType::Number if c.name == "min" || c.name == "max" => format!("{} IS NOT NULL", c.name),
                _ => "1".into(),
            };
            return Ok(FieldExpr { name: name.into(), ty: c.ty, str_sql, num_sql, exists_sql, is_column: true });
        }
    }

    // resource.* → resource map; everything else → attrs map.
    if let Some(k) = name.strip_prefix("resource.") {
        let key = lit(k);
        return Ok(FieldExpr {
            name: name.into(),
            ty: FieldType::String,
            str_sql: format!("resource[{key}]"),
            num_sql: format!("toFloat64OrNull(resource[{key}])"),
            exists_sql: format!("mapContains(resource, {key})"),
            is_column: false,
        });
    }
    let key = lit(name);
    let has_num_map = !matches!(ds, Dataset::Metrics);
    Ok(FieldExpr {
        name: name.into(),
        ty: FieldType::String,
        str_sql: format!("attrs[{key}]"),
        num_sql: if has_num_map {
            format!("if(mapContains(attrs_num, {key}), attrs_num[{key}], toFloat64OrNull(attrs[{key}]))")
        } else {
            format!("toFloat64OrNull(attrs[{key}])")
        },
        exists_sql: format!("mapContains(attrs, {key})"),
        is_column: false,
    })
}

/// Static columns for the field picker (attribute keys are added from data at runtime).
pub fn static_fields(ds: Dataset) -> Vec<FieldInfo> {
    let mut out: Vec<FieldInfo> = columns(ds)
        .map(|c| FieldInfo { name: c.aliases.first().copied().unwrap_or(c.name).to_string(), ty: c.ty, column: true })
        .collect();
    out.extend(
        VIRTUALS
            .iter()
            .filter(|v| v.datasets.contains(&ds))
            .map(|v| FieldInfo { name: v.name.into(), ty: v.ty, column: true }),
    );
    out
}

/// Columns returned in raw mode by default, per dataset.
pub fn default_raw_columns(ds: Dataset) -> &'static [&'static str] {
    match ds {
        Dataset::Spans => &["timestamp", "trace_id", "span_id", "parent_span_id", "service_name", "name", "kind", "duration_ms", "status_code", "http_route", "http_status_code", "user_id", "tenant_id", "code_function", "db_table", "gen_ai_model"],
        Dataset::Logs => &["timestamp", "severity", "service_name", "body", "trace_id", "span_id", "user_id", "tenant_id", "request_id", "code_function"],
        Dataset::Metrics => &["timestamp", "name", "kind", "unit", "service_name", "value", "count", "sum"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_columns_aliases_and_attrs() {
        let f = resolve(Dataset::Spans, "http.route").unwrap();
        assert_eq!(f.str_sql, "http_route");
        assert!(f.is_column);
        let f = resolve(Dataset::Spans, "duration_ms").unwrap();
        assert_eq!(f.num_sql, "(duration_ns / 1000000)");
        let f = resolve(Dataset::Spans, "app.cart_size").unwrap();
        assert_eq!(f.str_sql, "attrs['app.cart_size']");
        assert!(f.num_sql.contains("attrs_num['app.cart_size']"));
        assert!(!f.is_column);
        let f = resolve(Dataset::Spans, "resource.k8s.pod").unwrap();
        assert_eq!(f.str_sql, "resource['k8s.pod']");
        let f = resolve(Dataset::Logs, "message").unwrap();
        assert_eq!(f.str_sql, "body");
        let f = resolve(Dataset::Metrics, "custom").unwrap();
        assert!(!f.num_sql.contains("attrs_num"));
    }

    #[test]
    fn rejects_injection_in_attr_keys() {
        assert!(resolve(Dataset::Spans, "a' OR 1=1 --").is_err());
        assert!(resolve(Dataset::Spans, "it's").is_err());
    }
}
