//! Long-retention rollups: when a spans query only touches the RED dimensions and the range is
//! wider than the raw retention (or the query asks for it), answer from `spans_red_1m`.

use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use galileo_core::ProjectId;
use galileo_storage::{SqlQuery, SqlValue};

use crate::{CalcOp, Dataset, FilterOp, Query, QueryError, Result};

static RAW_RETENTION_SECS: OnceLock<i64> = OnceLock::new();

/// Called once by the server with `[retention].spans` so the engine knows when raw data ends.
pub fn set_raw_retention(secs: i64) { let _ = RAW_RETENTION_SECS.set(secs); }
pub fn raw_retention() -> i64 { *RAW_RETENTION_SECS.get().unwrap_or(&(30 * 86_400)) }

fn dim(name: &str) -> Option<&'static str> {
    match name {
        "service_name" | "service.name" => Some("service_name"),
        "http_route" | "http.route" => Some("http_route"),
        "tenant_id" | "tenant.id" => Some("tenant_id"),
        "status_code" => Some("status_code"),
        _ => None,
    }
}

/// True when every filter, breakdown and calculation can be answered from the rollup.
pub fn eligible(q: &Query) -> bool {
    if q.dataset != Dataset::Spans || q.calculations.is_empty() || q.search.is_some() || !q.having.is_empty() { return false; }
    if !q.breakdowns.iter().all(|b| dim(b).is_some()) { return false; }
    // root-only filter (parent_span_id = "") is implied by the rollup; other filters must be dimensions
    for f in &q.filters {
        if f.field == "parent_span_id" && f.op == FilterOp::Eq && f.value.as_ref().and_then(|v| v.as_str()) == Some("") { continue; }
        if f.field == "is_root" { continue; }
        if dim(&f.field).is_none() || !matches!(f.op, FilterOp::Eq | FilterOp::Ne | FilterOp::In | FilterOp::NotIn) { return false; }
    }
    q.calculations.iter().all(|c| matches!((c.op, c.field.as_deref()),
        (CalcOp::Count, _) | (CalcOp::RatePerSec, _) | (CalcOp::Avg, Some("is_error")) | (CalcOp::P50 | CalcOp::P75 | CalcOp::P90 | CalcOp::P95 | CalcOp::P99, Some("duration_ms")) | (CalcOp::Sum, Some("duration_ms"))))
}

/// Should this query use the rollup? Wider than raw retention, or explicitly asked.
pub fn should_use(q: &Query, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    if !eligible(q) { return false; }
    q.prefer_rollup.unwrap_or(false) || (end - start).num_seconds() > raw_retention()
}

fn calc_sql(c: &crate::Calculation, range_secs: f64) -> String {
    match (c.op, c.field.as_deref()) {
        (CalcOp::Count, _) => "toFloat64(countMerge(requests))".into(),
        (CalcOp::RatePerSec, _) => format!("toFloat64(countMerge(requests)) / {}", range_secs.max(1.0)),
        (CalcOp::Avg, _) => "toFloat64(countIfMerge(errors)) / greatest(toFloat64(countMerge(requests)), 1)".into(),
        (CalcOp::Sum, _) => "sumMerge(duration_sum)".into(),
        (op, _) => { let i = match op { CalcOp::P50 => 1, CalcOp::P75 => 2, CalcOp::P90 => 3, CalcOp::P95 => 4, _ => 5 }; format!("quantilesTDigestMerge(0.5, 0.75, 0.9, 0.95, 0.99)(duration)[{i}]") }
    }
}

fn where_sql(q: &Query, project: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<(String, Vec<SqlValue>)> {
    let mut parts = vec!["project_id = ?".to_string(), "minute >= toDateTime(?)".to_string(), "minute < toDateTime(?)".to_string()];
    let mut params: Vec<SqlValue> = vec![project.into(), start.timestamp().into(), end.timestamp().into()];
    for f in &q.filters {
        let Some(col) = dim(&f.field) else { continue };
        match f.op {
            FilterOp::Eq | FilterOp::Ne => { parts.push(format!("{col} {} ?", if f.op == FilterOp::Eq { "=" } else { "!=" })); params.push(f.value.as_ref().and_then(|v| v.as_str()).unwrap_or("").to_string().into()); }
            FilterOp::In | FilterOp::NotIn => {
                let vals: Vec<String> = f.value.as_ref().and_then(|v| v.as_array()).map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                if vals.is_empty() { return Err(QueryError::Invalid("empty IN list".into())); }
                parts.push(format!("{col} {} ({})", if f.op == FilterOp::In { "IN" } else { "NOT IN" }, vec!["?"; vals.len()].join(", ")));
                for v in vals { params.push(v.into()); }
            }
            _ => {}
        }
    }
    Ok((parts.join(" AND "), params))
}

pub fn totals_query(q: &Query, project: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<SqlQuery> {
    let (w, params) = where_sql(q, project, start, end)?;
    let mut select: Vec<String> = q.breakdowns.iter().enumerate().map(|(i, b)| format!("{} AS b{i}", dim(b).unwrap_or("service_name"))).collect();
    let range = (end - start).num_seconds() as f64;
    for (i, c) in q.calculations.iter().enumerate() { select.push(format!("{} AS c{i}", calc_sql(c, range))); }
    let group = if q.breakdowns.is_empty() { String::new() } else { format!(" GROUP BY {}", (0..q.breakdowns.len()).map(|i| format!("b{i}")).collect::<Vec<_>>().join(", ")) };
    let limit = q.limit.unwrap_or(crate::DEFAULT_GROUPS).clamp(1, crate::MAX_GROUPS);
    Ok(SqlQuery { sql: format!("SELECT {} FROM spans_red_1m WHERE {w}{group} ORDER BY c0 DESC LIMIT {limit}", select.join(", ")), params })
}

pub fn series_query(q: &Query, project: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>, granularity: u32, groups: Option<&[Vec<String>]>) -> Result<SqlQuery> {
    let (w, mut params) = where_sql(q, project, start, end)?;
    let g = granularity.max(60);
    let mut select = vec![format!("toUnixTimestamp(toStartOfInterval(minute, INTERVAL {g} SECOND)) AS bucket")];
    let bexprs: Vec<&str> = q.breakdowns.iter().map(|b| dim(b).unwrap_or("service_name")).collect();
    for (i, b) in bexprs.iter().enumerate() { select.push(format!("{b} AS b{i}")); }
    for (i, c) in q.calculations.iter().enumerate() { select.push(format!("{} AS c{i}", calc_sql(c, g as f64))); }
    let mut extra = String::new();
    if let Some(groups) = groups {
        if !bexprs.is_empty() && !groups.is_empty() {
            let mut items = vec![];
            for grp in groups { let ph = vec!["?"; grp.len()].join(", "); items.push(if grp.len() == 1 { ph } else { format!("({ph})") }); for v in grp { params.push(v.clone().into()); } }
            let lhs = if bexprs.len() == 1 { bexprs[0].to_string() } else { format!("({})", bexprs.join(", ")) };
            extra = format!(" AND {lhs} IN ({})", items.join(", "));
        }
    }
    let group_cols: Vec<String> = std::iter::once("bucket".to_string()).chain((0..bexprs.len()).map(|i| format!("b{i}"))).collect();
    Ok(SqlQuery { sql: format!("SELECT {} FROM spans_red_1m WHERE {w}{extra} GROUP BY {} ORDER BY bucket ASC", select.join(", "), group_cols.join(", ")), params })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Calculation, Filter};
    #[test]
    fn eligibility() {
        let mut q = Query { calculations: vec![Calculation { op: CalcOp::P95, field: Some("duration_ms".into()) }, Calculation { op: CalcOp::Count, field: None }], breakdowns: vec!["http_route".into()], filters: vec![Filter::new("service_name", FilterOp::Eq, "melea-api"), Filter::new("parent_span_id", FilterOp::Eq, "")], ..Default::default() };
        assert!(eligible(&q));
        q.breakdowns.push("user_id".into());
        assert!(!eligible(&q));
        q.breakdowns.pop();
        q.calculations.push(Calculation { op: CalcOp::P95, field: Some("gen_ai.usage.cost_usd".into()) });
        assert!(!eligible(&q));
    }
    #[test]
    fn sql_shape() {
        let q = Query { calculations: vec![Calculation { op: CalcOp::Count, field: None }, Calculation { op: CalcOp::Avg, field: Some("is_error".into()) }], breakdowns: vec!["service_name".into()], ..Default::default() };
        let t = totals_query(&q, ProjectId(Default::default()), Utc::now() - chrono::Duration::days(90), Utc::now()).unwrap();
        assert!(t.sql.contains("countMerge(requests)") && t.sql.contains("FROM spans_red_1m") && t.sql.contains("GROUP BY b0"));
    }
}
