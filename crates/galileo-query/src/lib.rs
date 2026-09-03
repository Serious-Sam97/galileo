//! Query DSL and its translation to storage SQL.
//!
//! The DSL is deliberately Honeycomb-shaped: a dataset, a time range, a list of calculations
//! (VISUALIZE), filters (WHERE), breakdowns (GROUP BY), orders and a limit. The UI, saved
//! queries, boards and triggers all speak this one structure.

pub mod bubbleup;
pub mod expr;
pub mod fields;
pub mod rollup;
pub mod run;
pub mod sql;
pub mod text;
pub mod trace;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("{0}")]
    Invalid(String),
    #[error("storage: {0}")]
    Storage(#[from] galileo_storage::StorageError),
}

pub type Result<T> = std::result::Result<T, QueryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Dataset {
    #[default]
    Spans,
    Logs,
    Metrics,
}

impl Dataset {
    pub fn table(&self) -> &'static str {
        match self {
            Dataset::Spans => "spans",
            Dataset::Logs => "logs",
            Dataset::Metrics => "metrics",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CalcOp {
    Count,
    CountDistinct,
    Sum,
    Avg,
    Min,
    Max,
    P50,
    P75,
    P90,
    P95,
    P99,
    P999,
    Heatmap,
    /// Rate per second of COUNT over the bucket (series only).
    RatePerSec,
}

impl CalcOp {
    pub fn needs_field(&self) -> bool {
        !matches!(self, CalcOp::Count | CalcOp::RatePerSec)
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            CalcOp::Count => "COUNT",
            CalcOp::CountDistinct => "COUNT_DISTINCT",
            CalcOp::Sum => "SUM",
            CalcOp::Avg => "AVG",
            CalcOp::Min => "MIN",
            CalcOp::Max => "MAX",
            CalcOp::P50 => "P50",
            CalcOp::P75 => "P75",
            CalcOp::P90 => "P90",
            CalcOp::P95 => "P95",
            CalcOp::P99 => "P99",
            CalcOp::P999 => "P999",
            CalcOp::Heatmap => "HEATMAP",
            CalcOp::RatePerSec => "RATE_PER_SEC",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calculation {
    pub op: CalcOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl Calculation {
    /// Column name used in results, e.g. `P99(duration_ms)`.
    pub fn label(&self) -> String {
        match &self.field {
            Some(f) => format!("{}({})", self.op.as_str(), f),
            None => self.op.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    #[serde(alias = "=")]
    Eq,
    #[serde(alias = "!=")]
    Ne,
    #[serde(alias = ">")]
    Gt,
    #[serde(alias = ">=")]
    Gte,
    #[serde(alias = "<")]
    Lt,
    #[serde(alias = "<=")]
    Lte,
    Contains,
    NotContains,
    StartsWith,
    Exists,
    NotExists,
    In,
    NotIn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

impl Filter {
    pub fn new(field: impl Into<String>, op: FilterOp, value: impl Into<serde_json::Value>) -> Self {
        Self { field: field.into(), op, value: Some(value.into()) }
    }
    pub fn exists(field: impl Into<String>) -> Self {
        Self { field: field.into(), op: FilterOp::Exists, value: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Combination {
    #[default]
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Asc,
    #[default]
    Desc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    /// Either a breakdown field or a calculation label (`P99(duration_ms)`), or a calculation
    /// index as `calc:0`.
    pub field: String,
    #[serde(default)]
    pub direction: Direction,
}

/// Absolute or relative time range. Relative is what saved queries and triggers use so they
/// stay meaningful over time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TimeRange {
    Absolute { start: DateTime<Utc>, end: DateTime<Utc> },
    Relative { last_seconds: i64 },
}

impl Default for TimeRange {
    fn default() -> Self {
        TimeRange::Relative { last_seconds: 3600 }
    }
}

impl TimeRange {
    pub fn resolve(&self, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        match self {
            TimeRange::Absolute { start, end } => (*start, *end),
            TimeRange::Relative { last_seconds } => (now - Duration::seconds((*last_seconds).max(1)), now),
        }
    }
}

/// A column computed from other columns with arithmetic, e.g. cost per output token.
/// In aggregate mode the expression references calculation labels (`SUM(gen_ai.usage.cost_usd)`)
/// and breakdown-independent numbers; in raw mode it references row fields (`duration_ms`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Derived {
    pub name: String,
    pub expr: String,
}

/// A post-aggregation filter (`HAVING count() > 100`). `target` is a calculation label or a
/// derived column name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Having {
    pub target: HavingTarget,
    pub op: FilterOp,
    pub value: f64,
}

/// Wrapper so a `Having.target` accepts either a plain string or `{ "calc": 0 }` from older clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HavingTarget(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    #[serde(default)]
    pub dataset: Dataset,
    #[serde(default)]
    pub time_range: TimeRange,
    /// Empty = raw events mode (rows instead of aggregates).
    #[serde(default)]
    pub calculations: Vec<Calculation>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub filter_combination: Combination,
    #[serde(default)]
    pub breakdowns: Vec<String>,
    #[serde(default)]
    pub orders: Vec<Order>,
    /// Max groups (aggregate mode) or max rows (raw mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Time bucket in seconds; auto when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<u32>,
    /// Free-text search: log bodies / span names / metric names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// Raw mode only: extra columns to return besides the defaults.
    #[serde(default)]
    pub columns: Vec<String>,
    /// Columns computed from other columns with arithmetic.
    #[serde(default)]
    pub derived: Vec<Derived>,
    /// Post-aggregation filters on calculation/derived values.
    #[serde(default)]
    pub having: Vec<Having>,
    /// Draw a comparison series from an earlier, equal-length window. `"previous"` = immediately
    /// before this range; a number = that many seconds earlier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare_to: Option<String>,
    /// Answer from the per-minute RED rollup (long retention) even inside the raw window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer_rollup: Option<bool>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            dataset: Dataset::Spans,
            time_range: TimeRange::default(),
            calculations: vec![Calculation { op: CalcOp::Count, field: None }],
            filters: vec![],
            filter_combination: Combination::And,
            breakdowns: vec![],
            orders: vec![],
            limit: None,
            granularity: None,
            search: None,
            columns: vec![],
            derived: vec![],
            having: vec![],
            compare_to: None,
            prefer_rollup: None,
        }
    }
}

pub const MAX_GROUPS: u32 = 1000;
pub const MAX_RAW_ROWS: u32 = 5000;
pub const DEFAULT_GROUPS: u32 = 50;
pub const DEFAULT_RAW_ROWS: u32 = 200;

impl Query {
    pub fn from_json(v: serde_json::Value) -> Result<Self> {
        let q: Query = serde_json::from_value(v).map_err(|e| QueryError::Invalid(e.to_string()))?;
        q.validate()?;
        Ok(q)
    }

    pub fn dataset_name(&self) -> &'static str {
        match self.dataset { Dataset::Spans => "spans", Dataset::Logs => "logs", Dataset::Metrics => "metrics" }
    }

    pub fn is_raw(&self) -> bool {
        self.calculations.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        for c in &self.calculations {
            if c.op.needs_field() && c.field.as_deref().map(str::trim).unwrap_or("").is_empty() {
                return Err(QueryError::Invalid(format!("{} requires a field", c.op.as_str())));
            }
            if let Some(f) = &c.field {
                fields::validate_name(f)?;
            }
        }
        let heatmaps = self.calculations.iter().filter(|c| c.op == CalcOp::Heatmap).count();
        if heatmaps > 1 {
            return Err(QueryError::Invalid("at most one HEATMAP per query".into()));
        }
        if heatmaps == 1 && !self.breakdowns.is_empty() {
            return Err(QueryError::Invalid("HEATMAP cannot be combined with breakdowns".into()));
        }
        for f in &self.filters {
            fields::validate_name(&f.field)?;
            let needs_value = !matches!(f.op, FilterOp::Exists | FilterOp::NotExists);
            if needs_value && f.value.is_none() {
                return Err(QueryError::Invalid(format!("filter on {} needs a value", f.field)));
            }
            if matches!(f.op, FilterOp::In | FilterOp::NotIn) && !f.value.as_ref().map(|v| v.is_array()).unwrap_or(false) {
                return Err(QueryError::Invalid(format!("filter on {} needs an array value", f.field)));
            }
        }
        for b in &self.breakdowns {
            fields::validate_name(b)?;
        }
        for c in &self.columns {
            fields::validate_name(c)?;
        }
        if self.breakdowns.len() > 8 {
            return Err(QueryError::Invalid("at most 8 breakdowns".into()));
        }
        for d in &self.derived {
            if d.name.trim().is_empty() {
                return Err(QueryError::Invalid("derived column needs a name".into()));
            }
            crate::expr::Expr::parse(&d.expr)?;
        }
        for h in &self.having {
            if !h.value.is_finite() {
                return Err(QueryError::Invalid("having value must be a finite number".into()));
            }
        }
        if !self.having.is_empty() && self.calculations.is_empty() {
            return Err(QueryError::Invalid("having needs at least one aggregation".into()));
        }
        let (s, e) = self.time_range.resolve(Utc::now());
        if e <= s {
            return Err(QueryError::Invalid("time range end must be after start".into()));
        }
        if let Some(g) = self.granularity {
            if g == 0 {
                return Err(QueryError::Invalid("granularity must be > 0".into()));
            }
        }
        Ok(())
    }

    /// Pick a bucket size that yields roughly 100–150 points across the range.
    pub fn effective_granularity(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> u32 {
        if let Some(g) = self.granularity {
            return g;
        }
        let secs = (end - start).num_seconds().max(1);
        const NICE: &[i64] = &[1, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 14400, 21600, 43200, 86400];
        let target = secs / 120;
        NICE.iter().copied().find(|n| *n >= target).unwrap_or(86400) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_honeycomb_shape() {
        let q = Query::from_json(serde_json::json!({
            "dataset": "spans",
            "time_range": { "last_seconds": 7200 },
            "calculations": [{ "op": "COUNT" }, { "op": "P99", "field": "duration_ms" }],
            "filters": [{ "field": "http.route", "op": "=", "value": "/x" }, { "field": "user.id", "op": "exists" }],
            "breakdowns": ["service.name"],
            "orders": [{ "field": "P99(duration_ms)", "direction": "desc" }],
            "limit": 10
        }))
        .unwrap();
        assert_eq!(q.calculations[1].label(), "P99(duration_ms)");
        assert_eq!(q.filters[0].op, FilterOp::Eq);
        assert_eq!(q.effective_granularity(Utc::now() - Duration::seconds(7200), Utc::now()), 60);
    }

    #[test]
    fn rejects_bad_queries() {
        assert!(Query::from_json(serde_json::json!({ "calculations": [{ "op": "P99" }] })).is_err());
        assert!(Query::from_json(serde_json::json!({ "filters": [{ "field": "a", "op": "=" }] })).is_err());
        assert!(Query::from_json(serde_json::json!({ "filters": [{ "field": "a; drop", "op": "exists" }] })).is_err());
        assert!(Query::from_json(serde_json::json!({
            "calculations": [{ "op": "HEATMAP", "field": "duration_ms" }], "breakdowns": ["x"]
        }))
        .is_err());
    }
}
