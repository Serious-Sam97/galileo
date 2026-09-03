//! Execute a `Query` against a `Storage` and shape the result for the UI: groups with totals
//! and zero-filled series, raw rows, or a heatmap grid.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use galileo_core::ProjectId;
use galileo_storage::{QueryResult, QueryStats, Storage};
use serde::{Deserialize, Serialize};

use crate::{expr::Expr, sql, CalcOp, FilterOp, Query, Result};

pub const HEATMAP_BINS: u32 = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    pub ts: i64,
    pub values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub key: Vec<String>,
    pub totals: Vec<Option<f64>>,
    pub series: Vec<Point>,
    /// Same-shape values from the comparison window, present only when `compare_to` was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare_totals: Option<Vec<Option<f64>>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compare_series: Vec<Point>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heatmap {
    pub field: String,
    pub log_scale: bool,
    /// Lower edge of each bin, in the field's units.
    pub bin_edges: Vec<f64>,
    pub buckets: Vec<i64>,
    /// counts[bucket_index][bin_index]
    pub counts: Vec<Vec<u64>>,
    pub max_count: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub mode: &'static str,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub granularity: u32,
    pub breakdowns: Vec<String>,
    pub calculations: Vec<String>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawRows>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heatmap: Option<Heatmap>,
    pub stats: QueryStats,
    #[serde(default)]
    pub sql: Vec<String>,
    /// The comparison window, when `compare_to` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare_start: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare_end: Option<DateTime<Utc>>,
}

fn f64_of(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        serde_json::Value::String(s) => s.parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

fn str_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn merge_stats(into: &mut QueryStats, s: &QueryStats) {
    into.elapsed += s.elapsed;
    into.rows_read += s.rows_read;
    into.bytes_read += s.bytes_read;
}

pub async fn run(storage: &dyn Storage, project_id: ProjectId, q: &Query) -> Result<QueryResponse> {
    q.validate()?;
    let now = Utc::now();
    let (start, end) = q.time_range.resolve(now);
    let granularity = q.effective_granularity(start, end);
    let mut stats = QueryStats::default();
    let mut sqls = Vec::new();

    if q.is_raw() {
        let (sq, names) = sql::raw_query(q, project_id, start, end)?;
        sqls.push(sq.sql.clone());
        let res = storage.query(&sq).await?;
        merge_stats(&mut stats, &res.stats);
        let mut names = names;
        let mut rows = res.rows;
        if !q.derived.is_empty() {
            let compiled: Vec<(String, Expr)> = q.derived.iter().filter_map(|d| Expr::parse(&d.expr).ok().map(|e| (d.name.clone(), e))).collect();
            for (name, _) in &compiled { names.push(name.clone()); }
            let col_idx: HashMap<String, usize> = names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
            for row in rows.iter_mut() {
                let base = row.clone();
                for (_, e) in &compiled {
                    let v = e.eval(&mut |n| col_idx.get(n).and_then(|&i| base.get(i)).and_then(f64_of));
                    row.push(v.map(|x| serde_json::json!(x)).unwrap_or(serde_json::Value::Null));
                }
            }
        }
        return Ok(QueryResponse {
            mode: "raw",
            start,
            end,
            granularity,
            breakdowns: vec![],
            calculations: vec![],
            groups: vec![],
            raw: Some(RawRows { columns: names, rows }),
            heatmap: None,
            stats,
            sql: sqls,
            compare_start: None,
            compare_end: None,
        });
    }

    if let Some(hc) = q.calculations.iter().find(|c| c.op == CalcOp::Heatmap) {
        let field = hc.field.clone().unwrap_or_default();
        let hm = run_heatmap(storage, project_id, q, &field, start, end, granularity, &mut stats, &mut sqls).await?;
        // Other calculations still get series (no breakdowns allowed with heatmap).
        let others: Vec<_> = q.calculations.iter().filter(|c| c.op != CalcOp::Heatmap).cloned().collect();
        let mut groups = vec![];
        if !others.is_empty() {
            let sub = Query { calculations: others.clone(), ..q.clone() };
            let (g, _) = run_groups(storage, project_id, &sub, start, end, granularity, &mut stats, &mut sqls).await?;
            groups = g;
        }
        return Ok(QueryResponse {
            mode: "heatmap",
            start,
            end,
            granularity,
            breakdowns: vec![],
            calculations: others.iter().map(|c| c.label()).collect(),
            groups,
            raw: None,
            heatmap: Some(hm),
            stats,
            sql: sqls,
            compare_start: None,
            compare_end: None,
        });
    }

    let (mut groups, _) = run_groups(storage, project_id, q, start, end, granularity, &mut stats, &mut sqls).await?;

    // labels: calculations then derived columns
    let mut labels: Vec<String> = q.calculations.iter().map(|c| c.label()).collect();
    let derived: Vec<(String, Expr)> = q.derived.iter().filter_map(|d| Expr::parse(&d.expr).ok().map(|e| (d.name.clone(), e))).collect();
    if !derived.is_empty() {
        apply_derived(&mut groups, &labels, &derived);
        for (name, _) in &derived { labels.push(name.clone()); }
    }
    if !q.having.is_empty() {
        apply_having(&mut groups, &labels, q);
    }

    // comparison window
    let (mut compare_start, mut compare_end) = (None, None);
    if let Some(spec) = q.compare_to.as_deref() {
        let span = end - start;
        let offset = if spec.eq_ignore_ascii_case("previous") || spec.is_empty() {
            span
        } else {
            chrono::Duration::seconds(spec.parse::<i64>().map_err(|_| crate::QueryError::Invalid("compare_to must be 'previous' or a number of seconds".into()))?.max(1))
        };
        let (cs, ce) = (start - offset, end - offset);
        compare_start = Some(cs);
        compare_end = Some(ce);
        let sub = Query { compare_to: None, having: vec![], ..q.clone() };
        if let Ok((cgroups, _)) = run_groups(storage, project_id, &sub, cs, ce, granularity, &mut stats, &mut sqls).await {
            let mut cg = cgroups;
            if !derived.is_empty() { apply_derived(&mut cg, &q.calculations.iter().map(|c| c.label()).collect::<Vec<_>>(), &derived); }
            let by_key: HashMap<Vec<String>, usize> = cg.iter().enumerate().map(|(i, g)| (g.key.clone(), i)).collect();
            let g = granularity as i64;
            let shift = (start - cs).num_seconds();
            for grp in groups.iter_mut() {
                if let Some(&ci) = by_key.get(&grp.key) {
                    grp.compare_totals = Some(cg[ci].totals.clone());
                    // realign the comparison buckets onto the main axis for the ghost line
                    grp.compare_series = cg[ci].series.iter().map(|p| Point { ts: ((p.ts + shift) / g) * g, values: p.values.clone() }).collect();
                }
            }
        }
    }

    Ok(QueryResponse {
        mode: "series",
        start,
        end,
        granularity,
        breakdowns: q.breakdowns.clone(),
        calculations: labels,
        groups,
        raw: None,
        heatmap: None,
        stats,
        sql: sqls,
        compare_start,
        compare_end,
    })
}

/// Append derived-column values (computed from calculation values by label) to each group's
/// totals and every series point.
fn apply_derived(groups: &mut [Group], labels: &[String], derived: &[(String, Expr)]) {
    let idx: HashMap<String, usize> = labels.iter().cloned().enumerate().map(|(i, l)| (l, i)).collect();
    for g in groups.iter_mut() {
        let base = g.totals.clone();
        for (_, e) in derived {
            g.totals.push(e.eval(&mut |n| idx.get(n).and_then(|&i| base.get(i).copied().flatten())));
        }
        for p in g.series.iter_mut() {
            let bp = p.values.clone();
            for (_, e) in derived {
                p.values.push(e.eval(&mut |n| idx.get(n).and_then(|&i| bp.get(i).copied().flatten())));
            }
        }
    }
}

/// Drop groups that fail any HAVING clause (evaluated against totals by label).
fn apply_having(groups: &mut Vec<Group>, labels: &[String], q: &Query) {
    let idx: HashMap<String, usize> = labels.iter().cloned().enumerate().map(|(i, l)| (l, i)).collect();
    groups.retain(|g| {
        q.having.iter().all(|h| {
            let key = &h.target.0;
            let v = idx.get(key)
                .or_else(|| key.strip_prefix("calc:").and_then(|s| s.parse::<usize>().ok()).and_then(|i| labels.get(i).and_then(|l| idx.get(l))))
                .and_then(|&i| g.totals.get(i).copied().flatten());
            match v {
                Some(v) => match h.op {
                    FilterOp::Gt => v > h.value, FilterOp::Gte => v >= h.value, FilterOp::Lt => v < h.value,
                    FilterOp::Lte => v <= h.value, FilterOp::Eq => (v - h.value).abs() < f64::EPSILON, FilterOp::Ne => (v - h.value).abs() >= f64::EPSILON,
                    _ => true,
                },
                None => false,
            }
        })
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_groups(
    storage: &dyn Storage,
    project_id: ProjectId,
    q: &Query,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    granularity: u32,
    stats: &mut QueryStats,
    sqls: &mut Vec<String>,
) -> Result<(Vec<Group>, QueryResult)> {
    let nb = q.breakdowns.len();
    let nc = q.calculations.len();
    let use_rollup = crate::rollup::should_use(q, start, end);

    let totals_q = if use_rollup { crate::rollup::totals_query(q, project_id, start, end)? } else { sql::totals_query(q, project_id, start, end)? };
    sqls.push(totals_q.sql.clone());
    let totals = storage.query(&totals_q).await?;
    merge_stats(stats, &totals.stats);

    let mut groups: Vec<Group> = totals
        .rows
        .iter()
        .map(|r| Group {
            key: r[..nb].iter().map(str_of).collect(),
            totals: r[nb..nb + nc].iter().map(f64_of).collect(),
            series: vec![],
            compare_totals: None,
            compare_series: vec![],
        })
        .collect();

    if nb > 0 && groups.is_empty() {
        return Ok((groups, totals));
    }
    if nb == 0 && groups.is_empty() {
        groups.push(Group { key: vec![], totals: vec![None; nc], series: vec![], compare_totals: None, compare_series: vec![] });
    }

    let keys: Vec<Vec<String>> = groups.iter().map(|g| g.key.clone()).collect();
    let series_q = if use_rollup { crate::rollup::series_query(q, project_id, start, end, granularity, if nb > 0 { Some(&keys) } else { None })? } else { sql::series_query(q, project_id, start, end, granularity, if nb > 0 { Some(&keys) } else { None })? };
    sqls.push(series_q.sql.clone());
    let series = storage.query(&series_q).await?;
    merge_stats(stats, &series.stats);

    // bucket grid
    let g = granularity as i64;
    let first = (start.timestamp() / g) * g;
    let last = ((end.timestamp() - 1) / g) * g;
    let buckets: Vec<i64> = (0..).map(|i| first + i * g).take_while(|t| *t <= last).collect();
    let index: HashMap<Vec<String>, usize> = keys.iter().cloned().enumerate().map(|(i, k)| (k, i)).collect();

    let mut grids: Vec<HashMap<i64, Vec<Option<f64>>>> = vec![HashMap::new(); groups.len()];
    for r in &series.rows {
        let ts = f64_of(&r[0]).unwrap_or(0.0) as i64;
        let key: Vec<String> = r[1..1 + nb].iter().map(str_of).collect();
        let Some(&gi) = index.get(&key) else { continue };
        let vals: Vec<Option<f64>> = r[1 + nb..1 + nb + nc].iter().map(f64_of).collect();
        grids[gi].insert(ts, vals);
    }
    for (gi, grp) in groups.iter_mut().enumerate() {
        grp.series = buckets
            .iter()
            .map(|ts| Point {
                ts: *ts,
                values: grids[gi].get(ts).cloned().unwrap_or_else(|| {
                    q.calculations
                        .iter()
                        .map(|c| if matches!(c.op, CalcOp::Count | CalcOp::CountDistinct | CalcOp::RatePerSec) { Some(0.0) } else { None })
                        .collect()
                }),
            })
            .collect();
    }
    Ok((groups, totals))
}

#[allow(clippy::too_many_arguments)]
async fn run_heatmap(
    storage: &dyn Storage,
    project_id: ProjectId,
    q: &Query,
    field: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    granularity: u32,
    stats: &mut QueryStats,
    sqls: &mut Vec<String>,
) -> Result<Heatmap> {
    let rq = sql::heatmap_range_query(q, field, project_id, start, end)?;
    sqls.push(rq.sql.clone());
    let r = storage.query(&rq).await?;
    merge_stats(stats, &r.stats);
    let (lo, hi, n) = r
        .rows
        .first()
        .map(|row| (f64_of(&row[0]).unwrap_or(0.0), f64_of(&row[1]).unwrap_or(0.0), f64_of(&row[2]).unwrap_or(0.0) as u64))
        .unwrap_or((0.0, 0.0, 0));

    let g = granularity as i64;
    let first = (start.timestamp() / g) * g;
    let last = ((end.timestamp() - 1) / g) * g;
    let buckets: Vec<i64> = (0..).map(|i| first + i * g).take_while(|t| *t <= last).collect();

    if n == 0 || hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
        return Ok(Heatmap {
            field: field.into(),
            log_scale: false,
            bin_edges: vec![lo; HEATMAP_BINS as usize],
            counts: vec![vec![0; HEATMAP_BINS as usize]; buckets.len()],
            buckets,
            max_count: 0,
            total: n,
        });
    }

    let log_scale = lo > 0.0 && hi / lo > 50.0;
    let (lo_t, hi_t) = if log_scale { (lo.ln(), hi.ln()) } else { (lo, hi) };
    let width = (hi_t - lo_t) / HEATMAP_BINS as f64;
    let bin_edges: Vec<f64> = (0..HEATMAP_BINS)
        .map(|i| {
            let e = lo_t + width * i as f64;
            if log_scale { e.exp() } else { e }
        })
        .collect();

    let hq = sql::heatmap_query(q, field, project_id, start, end, granularity, lo_t, width, log_scale, HEATMAP_BINS)?;
    sqls.push(hq.sql.clone());
    let res = storage.query(&hq).await?;
    merge_stats(stats, &res.stats);

    let bindex: HashMap<i64, usize> = buckets.iter().enumerate().map(|(i, t)| (*t, i)).collect();
    let mut counts = vec![vec![0u64; HEATMAP_BINS as usize]; buckets.len()];
    let mut max_count = 0u64;
    for row in &res.rows {
        let ts = f64_of(&row[0]).unwrap_or(0.0) as i64;
        let bin = f64_of(&row[1]).unwrap_or(0.0) as usize;
        let c = f64_of(&row[2]).unwrap_or(0.0) as u64;
        if let Some(&bi) = bindex.get(&ts) {
            if bin < HEATMAP_BINS as usize {
                counts[bi][bin] += c;
                max_count = max_count.max(counts[bi][bin]);
            }
        }
    }
    Ok(Heatmap { field: field.into(), log_scale, bin_edges, buckets, counts, max_count, total: n })
}
