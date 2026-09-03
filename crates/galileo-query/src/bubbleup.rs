//! BubbleUp: given a base query and a selection (extra filters that describe "the interesting
//! events"), compare the distribution of every attribute inside vs outside the selection and
//! rank attributes by how different they are. This is the fastest route from "p99 spiked"
//! to "it's tenant X on route Y".

use std::collections::BTreeMap;

use chrono::Utc;
use galileo_core::ProjectId;
use galileo_storage::{SqlQuery, Storage};
use serde::{Deserialize, Serialize};

use crate::sql::{filter_sql, where_clause};
use crate::{Dataset, Filter, Query, QueryError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct BubbleUpRequest {
    pub query: Query,
    /// Filters that define the selection, e.g. `duration_ms > 800` or a heatmap rectangle
    /// expressed as two filters.
    pub selection: Vec<Filter>,
    #[serde(default = "default_keys")]
    pub max_keys: usize,
    #[serde(default = "default_values")]
    pub max_values: usize,
}
fn default_keys() -> usize {
    30
}
fn default_values() -> usize {
    10
}

#[derive(Debug, Clone, Serialize)]
pub struct ValueRow {
    pub value: String,
    pub inside: u64,
    pub outside: u64,
    pub inside_pct: f64,
    pub outside_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyResult {
    pub key: String,
    /// Total variation distance between inside/outside distributions (0..1).
    pub score: f64,
    pub inside_total: u64,
    pub outside_total: u64,
    pub values: Vec<ValueRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BubbleUpResponse {
    pub inside_count: u64,
    pub outside_count: u64,
    pub keys: Vec<KeyResult>,
    pub sql: String,
}

fn hot_pairs(ds: Dataset) -> &'static str {
    match ds {
        Dataset::Spans => "[('name', name), ('service.name', service_name), ('kind', kind), ('status_code', status_code), \
            ('http.request.method', http_method), ('http.route', http_route), ('http.response.status_code', toString(http_status_code)), \
            ('user.id', user_id), ('tenant.id', tenant_id), ('exception.type', exception_type), ('gen_ai.response.model', gen_ai_model), \
            ('deployment.environment.name', deployment_env), ('host.name', host_name), ('service.version', service_version), \
            ('is_root', toString(parent_span_id = '')), ('code.function.name', code_function), ('code.namespace', code_namespace), \
            ('db.operation', db_operation), ('db.table', db_table), ('exception.culprit', exception_culprit)]",
        Dataset::Logs => "[('severity', severity), ('service.name', service_name), ('deployment.environment.name', deployment_env), \
            ('host.name', host_name), ('user.id', user_id), ('tenant.id', tenant_id), ('scope', scope_name)]",
        Dataset::Metrics => "[('name', name), ('service.name', service_name), ('kind', kind), ('unit', unit)]",
    }
}

pub fn build_sql(req: &BubbleUpRequest, project_id: ProjectId) -> Result<SqlQuery> {
    if req.selection.is_empty() {
        return Err(QueryError::Invalid("selection must have at least one filter".into()));
    }
    let q = &req.query;
    let (start, end) = q.time_range.resolve(Utc::now());
    let w = where_clause(q, project_id, start, end)?;
    // The selection expression appears in the SELECT list, before the WHERE clause, so its
    // parameters must be bound first: placeholders are positional.
    let mut sel_params = Vec::new();
    let mut sel_parts = Vec::new();
    for f in &req.selection {
        sel_parts.push(filter_sql(q.dataset, f, &mut sel_params)?);
    }
    let sel = sel_parts.join(" AND ");
    let mut params = sel_params;
    params.extend(w.params);
    let maps = if q.dataset == Dataset::Metrics {
        "arrayZip(mapKeys(attrs), mapValues(attrs))".to_string()
    } else {
        "arrayZip(mapKeys(attrs), mapValues(attrs)), arrayZip(mapKeys(resource), mapValues(resource))".to_string()
    };
    let max_values = req.max_values.clamp(2, 50);
    let sql = format!(
        "SELECT k, v, countIf(sel) AS inside, countIf(NOT sel) AS outside FROM ( \
            SELECT arrayJoin(arrayDistinct(arrayConcat({maps}, {hot}))) AS kv, kv.1 AS k, kv.2 AS v, ({sel}) AS sel \
            FROM {table} WHERE {base} \
         ) WHERE v != '' GROUP BY k, v ORDER BY inside DESC, outside DESC LIMIT {max_values} BY k LIMIT 20000",
        hot = hot_pairs(q.dataset),
        table = q.dataset.table(),
        base = w.sql,
    );
    Ok(SqlQuery { sql, params })
}

pub async fn run(storage: &dyn Storage, project_id: ProjectId, req: &BubbleUpRequest) -> Result<BubbleUpResponse> {
    req.query.validate()?;
    let sq = build_sql(req, project_id)?;
    let res = storage.query(&sq).await?;

    // Totals for the selection come from a dedicated count so per-key percentages are
    // relative to events, not to attribute occurrences.
    let (start, end) = req.query.time_range.resolve(Utc::now());
    let w = where_clause(&req.query, project_id, start, end)?;
    // `sel` is used twice in the SELECT list, so its params are bound twice, then the base.
    let mut sel_params = Vec::new();
    let mut sel_parts = Vec::new();
    for f in &req.selection {
        sel_parts.push(filter_sql(req.query.dataset, f, &mut sel_params)?);
    }
    let mut params = sel_params.clone();
    params.extend(sel_params);
    params.extend(w.params);
    let count_sql = SqlQuery {
        sql: format!(
            "SELECT countIf(({sel})) AS inside, countIf(NOT ({sel})) AS outside FROM {} WHERE {}",
            req.query.dataset.table(),
            w.sql,
            sel = sel_parts.join(" AND ")
        ),
        params,
    };
    let c = storage.query(&count_sql).await?;
    let num = |v: &serde_json::Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0) as u64;
    let (inside_count, outside_count) = c.rows.first().map(|r| (num(&r[0]), num(&r[1]))).unwrap_or((0, 0));

    let mut by_key: BTreeMap<String, Vec<ValueRow>> = BTreeMap::new();
    for r in &res.rows {
        let k = r[0].as_str().unwrap_or("").to_string();
        let v = r[1].as_str().map(str::to_owned).unwrap_or_else(|| r[1].to_string());
        let inside = num(&r[2]);
        let outside = num(&r[3]);
        by_key.entry(k).or_default().push(ValueRow {
            value: v,
            inside,
            outside,
            inside_pct: if inside_count > 0 { inside as f64 / inside_count as f64 } else { 0.0 },
            outside_pct: if outside_count > 0 { outside as f64 / outside_count as f64 } else { 0.0 },
        });
    }

    let mut keys: Vec<KeyResult> = by_key
        .into_iter()
        .map(|(key, values)| {
            let inside_total: u64 = values.iter().map(|v| v.inside).sum();
            let outside_total: u64 = values.iter().map(|v| v.outside).sum();
            // TVD over the observed values plus the mass of "attribute absent".
            let mut tvd = 0.0;
            for v in &values {
                tvd += (v.inside_pct - v.outside_pct).abs();
            }
            let absent_in = 1.0 - (inside_total as f64 / inside_count.max(1) as f64).min(1.0);
            let absent_out = 1.0 - (outside_total as f64 / outside_count.max(1) as f64).min(1.0);
            tvd += (absent_in - absent_out).abs();
            let score = (tvd / 2.0).clamp(0.0, 1.0);
            KeyResult { key, score, inside_total, outside_total, values }
        })
        // keys present in neither side of the selection carry no signal
        .filter(|k| k.inside_total > 0 || k.outside_total > 0)
        .collect();
    keys.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    keys.truncate(req.max_keys.clamp(1, 100));

    Ok(BubbleUpResponse { inside_count, outside_count, keys, sql: sq.sql })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilterOp;

    #[test]
    fn sql_shape() {
        let req = BubbleUpRequest {
            query: Query { filters: vec![Filter::new("service.name", FilterOp::Eq, "api")], ..Default::default() },
            selection: vec![Filter::new("duration_ms", FilterOp::Gt, 500)],
            max_keys: 10,
            max_values: 5,
        };
        let s = build_sql(&req, ProjectId::new()).unwrap();
        assert!(s.sql.contains("countIf(sel) AS inside"));
        assert!(s.sql.contains("((duration_ns / 1000000) > ?)) AS sel"));
        assert!(s.sql.contains("LIMIT 5 BY k"));
        assert_eq!(s.params.len(), 5);
        // selection param (500) is bound first because it appears first in the SQL text
        assert_eq!(s.params[0], galileo_storage::SqlValue::Float(500.0));
    }
}
