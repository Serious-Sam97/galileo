//! Query → SQL. Every user-controlled string is either validated to a safe charset (field
//! names, see `fields::validate_name`) or bound as a parameter; nothing is interpolated raw.

use chrono::{DateTime, Utc};
use galileo_core::ProjectId;
use galileo_storage::{SqlQuery, SqlValue};

use crate::fields::{self, FieldExpr, FieldType};
use crate::{CalcOp, Calculation, Combination, Dataset, Direction, Filter, FilterOp, Query, QueryError, Result};
use crate::{DEFAULT_GROUPS, DEFAULT_RAW_ROWS, MAX_GROUPS, MAX_RAW_ROWS};

pub struct Where {
    pub sql: String,
    pub params: Vec<SqlValue>,
}

fn json_to_param(v: &serde_json::Value) -> SqlValue {
    match v {
        serde_json::Value::String(s) => SqlValue::Str(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Int(i)
            } else {
                SqlValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::Bool(b) => SqlValue::Str(b.to_string()),
        serde_json::Value::Null => SqlValue::Str(String::new()),
        other => SqlValue::Str(other.to_string()),
    }
}

fn is_numeric_value(v: &serde_json::Value) -> bool {
    v.is_number()
}

/// One filter → SQL fragment with bound params.
pub fn filter_sql(ds: Dataset, f: &Filter, params: &mut Vec<SqlValue>) -> Result<String> {
    let fe: FieldExpr = fields::resolve(ds, &f.field)?;
    let v = f.value.as_ref();
    let numeric_cmp = |v: &serde_json::Value| is_numeric_value(v) || fe.ty == FieldType::Number;
    Ok(match f.op {
        FilterOp::Exists => format!("({})", fe.exists_sql),
        FilterOp::NotExists => format!("(NOT ({}))", fe.exists_sql),
        FilterOp::Eq | FilterOp::Ne => {
            let v = v.ok_or_else(|| QueryError::Invalid("missing value".into()))?;
            let op = if f.op == FilterOp::Eq { "=" } else { "!=" };
            if numeric_cmp(v) && fe.ty != FieldType::String || (is_numeric_value(v) && !fe.is_column) {
                params.push(SqlValue::Float(v.as_f64().unwrap_or(0.0)));
                format!("({} {op} ?)", fe.num_sql)
            } else {
                params.push(match v {
                    serde_json::Value::String(s) => SqlValue::Str(s.clone()),
                    other => SqlValue::Str(other.to_string().trim_matches('"').to_string()),
                });
                format!("({} {op} ?)", fe.str_sql)
            }
        }
        FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => {
            let v = v.ok_or_else(|| QueryError::Invalid("missing value".into()))?;
            let op = match f.op {
                FilterOp::Gt => ">",
                FilterOp::Gte => ">=",
                FilterOp::Lt => "<",
                _ => "<=",
            };
            if is_numeric_value(v) || fe.ty == FieldType::Number {
                let n = v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
                params.push(SqlValue::Float(n));
                format!("({} {op} ?)", fe.num_sql)
            } else if fe.ty == FieldType::Timestamp {
                params.push(json_to_param(v));
                format!("({} {op} parseDateTime64BestEffort(?, 9))", fe.name_col())
            } else {
                params.push(json_to_param(v));
                format!("({} {op} ?)", fe.str_sql)
            }
        }
        FilterOp::Contains | FilterOp::NotContains | FilterOp::StartsWith => {
            let v = v.ok_or_else(|| QueryError::Invalid("missing value".into()))?;
            params.push(SqlValue::Str(v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())));
            match f.op {
                FilterOp::Contains => format!("(positionCaseInsensitive({}, ?) > 0)", fe.str_sql),
                FilterOp::NotContains => format!("(positionCaseInsensitive({}, ?) = 0)", fe.str_sql),
                _ => format!("startsWith({}, ?)", fe.str_sql),
            }
        }
        FilterOp::In | FilterOp::NotIn => {
            let arr = v.and_then(|v| v.as_array()).ok_or_else(|| QueryError::Invalid("IN needs an array".into()))?;
            if arr.is_empty() {
                return Ok(if f.op == FilterOp::In { "0".into() } else { "1".into() });
            }
            let all_num = arr.iter().all(is_numeric_value) && fe.ty != FieldType::String;
            let placeholders = vec!["?"; arr.len()].join(", ");
            for item in arr {
                params.push(if all_num { SqlValue::Float(item.as_f64().unwrap_or(0.0)) } else { json_to_param(item) });
            }
            let expr = if all_num { &fe.num_sql } else { &fe.str_sql };
            let not = if f.op == FilterOp::NotIn { "NOT " } else { "" };
            format!("({expr} {not}IN ({placeholders}))")
        }
    })
}

impl FieldExpr {
    /// For timestamp columns the raw column name (str_sql wraps it in toString()).
    fn name_col(&self) -> String {
        self.str_sql.trim_start_matches("toString(").trim_end_matches(')').to_string()
    }
}

fn search_sql(ds: Dataset, search: &str, params: &mut Vec<SqlValue>) -> String {
    let s = search.trim();
    match ds {
        Dataset::Logs => {
            let tokens: Vec<String> = s
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|t| t.len() >= 2)
                .map(|t| t.to_string())
                .collect();
            if tokens.is_empty() {
                params.push(SqlValue::Str(s.to_string()));
                return "(positionCaseInsensitive(body, ?) > 0)".into();
            }
            let parts: Vec<String> = tokens
                .iter()
                .map(|t| {
                    params.push(SqlValue::Str(t.clone()));
                    "hasTokenCaseInsensitive(body, ?)".to_string()
                })
                .collect();
            format!("({})", parts.join(" AND "))
        }
        Dataset::Spans => {
            params.push(SqlValue::Str(s.to_string()));
            params.push(SqlValue::Str(s.to_string()));
            "(positionCaseInsensitive(name, ?) > 0 OR positionCaseInsensitive(url_path, ?) > 0)".into()
        }
        Dataset::Metrics => {
            params.push(SqlValue::Str(s.to_string()));
            "(positionCaseInsensitive(name, ?) > 0)".into()
        }
    }
}

/// Shared WHERE for every query shape: project, time range, filters, search.
pub fn where_clause(q: &Query, project_id: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Where> {
    let mut params = vec![SqlValue::from(project_id), SqlValue::from(start), SqlValue::from(end)];
    let mut parts = vec![
        "project_id = ?".to_string(),
        "timestamp >= fromUnixTimestamp64Nano(?)".to_string(),
        "timestamp < fromUnixTimestamp64Nano(?)".to_string(),
    ];
    if !q.filters.is_empty() {
        let mut fparts = Vec::with_capacity(q.filters.len());
        for f in &q.filters {
            fparts.push(filter_sql(q.dataset, f, &mut params)?);
        }
        let joiner = match q.filter_combination {
            Combination::And => " AND ",
            Combination::Or => " OR ",
        };
        parts.push(format!("({})", fparts.join(joiner)));
    }
    if let Some(s) = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(search_sql(q.dataset, s, &mut params));
    }
    Ok(Where { sql: parts.join(" AND "), params })
}

/// SQL for one calculation. `bucket_secs` is used by RATE_PER_SEC (None = totals over range).
pub fn calc_sql(ds: Dataset, c: &Calculation, range_secs: f64) -> Result<String> {
    let num = |f: &Option<String>| -> Result<String> {
        let name = f.as_deref().ok_or_else(|| QueryError::Invalid("missing field".into()))?;
        Ok(fields::resolve(ds, name)?.num_sql)
    };
    let strf = |f: &Option<String>| -> Result<String> {
        let name = f.as_deref().ok_or_else(|| QueryError::Invalid("missing field".into()))?;
        Ok(fields::resolve(ds, name)?.str_sql)
    };
    Ok(match c.op {
        CalcOp::Count => "toFloat64(count())".into(),
        CalcOp::RatePerSec => format!("toFloat64(count()) / {}", range_secs.max(1.0)),
        CalcOp::CountDistinct => format!("toFloat64(uniq({}))", strf(&c.field)?),
        CalcOp::Sum => format!("sum({})", num(&c.field)?),
        CalcOp::Avg => format!("avg({})", num(&c.field)?),
        CalcOp::Min => format!("min({})", num(&c.field)?),
        CalcOp::Max => format!("max({})", num(&c.field)?),
        CalcOp::P50 => format!("quantileTDigest(0.5)({})", num(&c.field)?),
        CalcOp::P75 => format!("quantileTDigest(0.75)({})", num(&c.field)?),
        CalcOp::P90 => format!("quantileTDigest(0.9)({})", num(&c.field)?),
        CalcOp::P95 => format!("quantileTDigest(0.95)({})", num(&c.field)?),
        CalcOp::P99 => format!("quantileTDigest(0.99)({})", num(&c.field)?),
        CalcOp::P999 => format!("quantileTDigest(0.999)({})", num(&c.field)?),
        // HEATMAP is executed by its own query shape; as a totals column it degrades to count.
        CalcOp::Heatmap => "toFloat64(count())".into(),
    })
}

fn order_sql(q: &Query) -> Result<String> {
    if q.orders.is_empty() {
        // Default: first calculation descending, which is what people expect from "top N".
        return Ok(if q.calculations.is_empty() { "timestamp DESC".into() } else { "c0 DESC".into() });
    }
    let mut parts = Vec::new();
    for o in &q.orders {
        let dir = match o.direction {
            Direction::Asc => "ASC",
            Direction::Desc => "DESC",
        };
        let col = if let Some(idx) = o.field.strip_prefix("calc:") {
            let i: usize = idx.parse().map_err(|_| QueryError::Invalid("bad calc order".into()))?;
            if i >= q.calculations.len() {
                return Err(QueryError::Invalid("calc order index out of range".into()));
            }
            format!("c{i}")
        } else if let Some(i) = q.calculations.iter().position(|c| c.label() == o.field) {
            format!("c{i}")
        } else if let Some(i) = q.breakdowns.iter().position(|b| *b == o.field) {
            format!("b{i}")
        } else if q.is_raw() {
            fields::resolve(q.dataset, &o.field)?.str_sql
        } else {
            return Err(QueryError::Invalid(format!("cannot order by '{}': not a breakdown or calculation", o.field)));
        };
        parts.push(format!("{col} {dir}"));
    }
    Ok(parts.join(", "))
}

/// Totals per group over the whole range (the table under the chart, and the top-N picker).
pub fn totals_query(q: &Query, project_id: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<SqlQuery> {
    let w = where_clause(q, project_id, start, end)?;
    let range_secs = (end - start).num_seconds() as f64;
    let mut select = Vec::new();
    for (i, b) in q.breakdowns.iter().enumerate() {
        select.push(format!("{} AS b{i}", fields::resolve(q.dataset, b)?.str_sql));
    }
    for (i, c) in q.calculations.iter().enumerate() {
        select.push(format!("{} AS c{i}", calc_sql(q.dataset, c, range_secs)?));
    }
    let group_by = if q.breakdowns.is_empty() {
        String::new()
    } else {
        format!(" GROUP BY {}", (0..q.breakdowns.len()).map(|i| format!("b{i}")).collect::<Vec<_>>().join(", "))
    };
    let limit = q.limit.unwrap_or(DEFAULT_GROUPS).clamp(1, MAX_GROUPS);
    let sql = format!(
        "SELECT {} FROM {} WHERE {}{} ORDER BY {} LIMIT {limit}",
        select.join(", "),
        q.dataset.table(),
        w.sql,
        group_by,
        order_sql(q)?
    );
    Ok(SqlQuery { sql, params: w.params })
}

/// Time series per group. When `groups` is given (from the totals query), only those groups
/// are computed so the chart shows exactly the rows in the table.
pub fn series_query(
    q: &Query,
    project_id: ProjectId,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    granularity: u32,
    groups: Option<&[Vec<String>]>,
) -> Result<SqlQuery> {
    let mut w = where_clause(q, project_id, start, end)?;
    let mut select = vec![format!("toUnixTimestamp(toStartOfInterval(timestamp, INTERVAL {granularity} SECOND)) AS bucket")];
    let mut bexprs = Vec::new();
    for (i, b) in q.breakdowns.iter().enumerate() {
        let e = fields::resolve(q.dataset, b)?.str_sql;
        select.push(format!("{e} AS b{i}"));
        bexprs.push(e);
    }
    for (i, c) in q.calculations.iter().enumerate() {
        select.push(format!("{} AS c{i}", calc_sql(q.dataset, c, granularity as f64)?));
    }
    let mut extra = String::new();
    if let Some(groups) = groups {
        if !bexprs.is_empty() && !groups.is_empty() {
            let tuple = format!("({})", bexprs.join(", "));
            let mut items = Vec::with_capacity(groups.len());
            for g in groups {
                let ph = vec!["?"; g.len()].join(", ");
                items.push(if g.len() == 1 { ph } else { format!("({ph})") });
                for v in g {
                    w.params.push(SqlValue::Str(v.clone()));
                }
            }
            let lhs = if bexprs.len() == 1 { bexprs[0].clone() } else { tuple };
            extra = format!(" AND {lhs} IN ({})", items.join(", "));
        }
    }
    let group_cols: Vec<String> = std::iter::once("bucket".to_string())
        .chain((0..q.breakdowns.len()).map(|i| format!("b{i}")))
        .collect();
    let sql = format!(
        "SELECT {} FROM {} WHERE {}{} GROUP BY {} ORDER BY bucket ASC",
        select.join(", "),
        q.dataset.table(),
        w.sql,
        extra,
        group_cols.join(", ")
    );
    Ok(SqlQuery { sql, params: w.params })
}

/// Raw events: the newest rows matching the filters.
pub fn raw_query(q: &Query, project_id: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<(SqlQuery, Vec<String>)> {
    let w = where_clause(q, project_id, start, end)?;
    let mut names: Vec<String> = fields::default_raw_columns(q.dataset).iter().map(|s| s.to_string()).collect();
    for c in &q.columns {
        if !names.contains(c) {
            names.push(c.clone());
        }
    }
    let mut select = Vec::with_capacity(names.len() + 1);
    for (i, n) in names.iter().enumerate() {
        let fe = fields::resolve(q.dataset, n)?;
        let expr = match fe.ty {
            FieldType::Number => fe.num_sql,
            FieldType::Timestamp => fe.name_col(),
            _ => fe.str_sql,
        };
        select.push(format!("{expr} AS r{i}"));
    }
    if q.dataset != Dataset::Metrics {
        select.push("attrs AS r_attrs".into());
    }
    let limit = q.limit.unwrap_or(DEFAULT_RAW_ROWS).clamp(1, MAX_RAW_ROWS);
    let sql = format!(
        "SELECT {} FROM {} WHERE {} ORDER BY {} LIMIT {limit}",
        select.join(", "),
        q.dataset.table(),
        w.sql,
        order_sql(q)?
    );
    if q.dataset != Dataset::Metrics {
        names.push("attrs".into());
    }
    Ok((SqlQuery { sql, params: w.params }, names))
}

/// Heatmap step 1: value range of the field.
pub fn heatmap_range_query(q: &Query, field: &str, project_id: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<SqlQuery> {
    let w = where_clause(q, project_id, start, end)?;
    let num = fields::resolve(q.dataset, field)?.num_sql;
    let sql = format!(
        "SELECT min({num}) AS lo, max({num}) AS hi, count() AS n FROM {} WHERE {} AND {num} IS NOT NULL",
        q.dataset.table(),
        w.sql
    );
    Ok(SqlQuery { sql, params: w.params })
}

/// Heatmap step 2: counts per (time bucket, value bin).
#[allow(clippy::too_many_arguments)]
pub fn heatmap_query(
    q: &Query,
    field: &str,
    project_id: ProjectId,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    granularity: u32,
    lo: f64,
    width: f64,
    log_scale: bool,
    bins: u32,
) -> Result<SqlQuery> {
    let w = where_clause(q, project_id, start, end)?;
    let num = fields::resolve(q.dataset, field)?.num_sql;
    let bin = if log_scale {
        format!("least({bins} - 1, greatest(0, toInt32(floor((log(greatest({num}, 1e-9)) - {lo}) / {width}))))")
    } else {
        format!("least({bins} - 1, greatest(0, toInt32(floor(({num} - {lo}) / {width}))))")
    };
    let sql = format!(
        "SELECT toUnixTimestamp(toStartOfInterval(timestamp, INTERVAL {granularity} SECOND)) AS bucket, {bin} AS bin, count() AS n \
         FROM {} WHERE {} AND {num} IS NOT NULL GROUP BY bucket, bin ORDER BY bucket, bin",
        q.dataset.table(),
        w.sql
    );
    Ok(SqlQuery { sql, params: w.params })
}

/// Attribute keys seen in the range, with row counts, for the field picker.
pub fn keys_query(ds: Dataset, project_id: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> SqlQuery {
    let maps = if ds == Dataset::Metrics {
        "arrayJoin(mapKeys(attrs))"
    } else {
        "arrayJoin(arrayConcat(mapKeys(attrs), arrayMap(k -> concat('resource.', k), mapKeys(resource))))"
    };
    SqlQuery {
        sql: format!(
            "SELECT {maps} AS k, count() AS n FROM {} WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) \
             AND timestamp < fromUnixTimestamp64Nano(?) GROUP BY k ORDER BY n DESC LIMIT 1000",
            ds.table()
        ),
        params: vec![project_id.into(), start.into(), end.into()],
    }
}

/// Top values of one field, optionally filtered by prefix, for autocomplete.
pub fn values_query(ds: Dataset, field: &str, prefix: Option<&str>, project_id: ProjectId, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<SqlQuery> {
    let fe = fields::resolve(ds, field)?;
    let mut params = vec![project_id.into(), start.into(), end.into()];
    let mut extra = String::new();
    if let Some(p) = prefix.map(str::trim).filter(|p| !p.is_empty()) {
        params.push(SqlValue::Str(p.to_string()));
        extra = format!(" AND positionCaseInsensitive({}, ?) > 0", fe.str_sql);
    }
    Ok(SqlQuery {
        sql: format!(
            "SELECT {} AS v, count() AS n FROM {} WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) \
             AND timestamp < fromUnixTimestamp64Nano(?) AND ({}){extra} GROUP BY v ORDER BY n DESC LIMIT 50",
            fe.str_sql,
            ds.table(),
            fe.exists_sql
        ),
        params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Order, TimeRange};

    fn range() -> (DateTime<Utc>, DateTime<Utc>) {
        let end = DateTime::parse_from_rfc3339("2026-09-02T12:00:00Z").unwrap().with_timezone(&Utc);
        (end - chrono::Duration::hours(1), end)
    }

    fn q() -> Query {
        Query {
            calculations: vec![
                Calculation { op: CalcOp::Count, field: None },
                Calculation { op: CalcOp::P99, field: Some("duration_ms".into()) },
            ],
            filters: vec![
                Filter::new("http.route", FilterOp::Eq, "/pets"),
                Filter::new("app.cart_size", FilterOp::Gt, 3),
                Filter::exists("user.id"),
                Filter::new("service.name", FilterOp::In, serde_json::json!(["api", "web"])),
            ],
            breakdowns: vec!["service.name".into(), "app.tier".into()],
            orders: vec![Order { field: "P99(duration_ms)".into(), direction: Direction::Desc }],
            limit: Some(5),
            time_range: TimeRange::Relative { last_seconds: 3600 },
            ..Default::default()
        }
    }

    #[test]
    fn totals_sql_shape() {
        let (s, e) = range();
        let t = totals_query(&q(), ProjectId::new(), s, e).unwrap();
        assert!(t.sql.starts_with("SELECT service_name AS b0, attrs['app.tier'] AS b1, toFloat64(count()) AS c0, quantileTDigest(0.99)((duration_ns / 1000000)) AS c1 FROM spans WHERE project_id = ?"));
        assert!(t.sql.contains("(http_route = ?)"));
        assert!(t.sql.contains("attrs_num['app.cart_size']"));
        assert!(t.sql.contains("(user_id != '')"));
        assert!(t.sql.contains("(service_name IN (?, ?))"));
        assert!(t.sql.ends_with("GROUP BY b0, b1 ORDER BY c1 DESC LIMIT 5"));
        // project, start, end, route, cart_size, api, web
        assert_eq!(t.params.len(), 7);
        assert_eq!(t.params[4], SqlValue::Float(3.0));
    }

    #[test]
    fn series_sql_restricts_to_groups() {
        let (s, e) = range();
        let groups = vec![vec!["api".to_string(), "gold".to_string()], vec!["web".to_string(), "".to_string()]];
        let t = series_query(&q(), ProjectId::new(), s, e, 60, Some(&groups)).unwrap();
        assert!(t.sql.contains("toStartOfInterval(timestamp, INTERVAL 60 SECOND)"));
        assert!(t.sql.contains("AND (service_name, attrs['app.tier']) IN ((?, ?), (?, ?))"));
        assert!(t.sql.ends_with("GROUP BY bucket, b0, b1 ORDER BY bucket ASC"));
        assert_eq!(t.params.len(), 7 + 4);
    }

    #[test]
    fn raw_sql_and_search() {
        let (s, e) = range();
        let mut rq = Query { calculations: vec![], dataset: Dataset::Logs, ..Default::default() };
        rq.search = Some("timeout db.pool".into());
        rq.columns = vec!["k8s.pod".into()];
        let (t, names) = raw_query(&rq, ProjectId::new(), s, e).unwrap();
        assert!(t.sql.contains("hasTokenCaseInsensitive(body, ?) AND hasTokenCaseInsensitive(body, ?) AND hasTokenCaseInsensitive(body, ?)"));
        assert!(t.sql.contains("attrs['k8s.pod'] AS r10"));
        assert!(t.sql.ends_with("ORDER BY timestamp DESC LIMIT 200"));
        assert_eq!(names.last().map(String::as_str), Some("attrs"));
    }

    #[test]
    fn rate_and_distinct() {
        let (s, e) = range();
        let rq = Query {
            calculations: vec![
                Calculation { op: CalcOp::RatePerSec, field: None },
                Calculation { op: CalcOp::CountDistinct, field: Some("user.id".into()) },
            ],
            ..Default::default()
        };
        let t = series_query(&rq, ProjectId::new(), s, e, 30, None).unwrap();
        assert!(t.sql.contains("toFloat64(count()) / 30 AS c0"));
        assert!(t.sql.contains("toFloat64(uniq(user_id)) AS c1"));
    }

    #[test]
    fn order_by_unknown_field_is_rejected() {
        let (s, e) = range();
        let mut bad = q();
        bad.orders = vec![Order { field: "nope".into(), direction: Direction::Asc }];
        assert!(totals_query(&bad, ProjectId::new(), s, e).is_err());
    }

    #[test]
    fn heatmap_bins() {
        let (s, e) = range();
        let hq = Query { calculations: vec![Calculation { op: CalcOp::Heatmap, field: Some("duration_ms".into()) }], ..Default::default() };
        let t = heatmap_query(&hq, "duration_ms", ProjectId::new(), s, e, 60, 0.0, 2.5, true, 32).unwrap();
        assert!(t.sql.contains("log(greatest((duration_ns / 1000000), 1e-9))"));
        assert!(t.sql.contains("GROUP BY bucket, bin"));
    }
}
