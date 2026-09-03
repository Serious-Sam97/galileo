use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Attributes, ProjectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    #[default]
    Gauge,
    Sum,
    Histogram,
    ExponentialHistogram,
    Summary,
}

impl MetricKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricKind::Gauge => "gauge",
            MetricKind::Sum => "sum",
            MetricKind::Histogram => "histogram",
            MetricKind::ExponentialHistogram => "exponential_histogram",
            MetricKind::Summary => "summary",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => MetricKind::Sum,
            2 => MetricKind::Histogram,
            3 => MetricKind::ExponentialHistogram,
            4 => MetricKind::Summary,
            _ => MetricKind::Gauge,
        }
    }
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AggregationTemporality {
    #[default]
    Unspecified,
    Delta,
    Cumulative,
}

impl AggregationTemporality {
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => AggregationTemporality::Delta,
            2 => AggregationTemporality::Cumulative,
            _ => AggregationTemporality::Unspecified,
        }
    }
}

/// One data point of one metric. Gauges and sums fill `value`; histograms fill the
/// `count/sum/min/max/bucket_*` fields. One row shape keeps the store simple and lets the
/// explorer treat every metric the same way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricPoint {
    pub project_id: ProjectId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub unit: String,
    pub kind: MetricKind,
    #[serde(default)]
    pub temporality: AggregationTemporality,
    #[serde(default)]
    pub is_monotonic: bool,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_timestamp: Option<DateTime<Utc>>,
    pub service_name: String,
    #[serde(default)]
    pub scope_name: String,
    #[serde(default)]
    pub resource: Attributes,
    #[serde(default)]
    pub attributes: Attributes,
    #[serde(default)]
    pub value: f64,
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub sum: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default)]
    pub bucket_counts: Vec<u64>,
    #[serde(default)]
    pub explicit_bounds: Vec<f64>,
    /// Exponential histogram raw buckets (OTLP): base = 2^(2^-scale); bucket i (offset applied)
    /// covers (base^i, base^(i+1)]. Kept for fidelity; `bucket_counts`/`explicit_bounds` also
    /// carry an expanded form so every histogram query path works the same.
    #[serde(default)]
    pub exp_scale: i32,
    #[serde(default)]
    pub exp_zero_count: u64,
    #[serde(default)]
    pub exp_pos_offset: i32,
    #[serde(default)]
    pub exp_pos_counts: Vec<u64>,
    #[serde(default)]
    pub exp_neg_offset: i32,
    #[serde(default)]
    pub exp_neg_counts: Vec<u64>,
}

/// Upper bound of exponential bucket `index` at `scale`: base^(index+1) with base = 2^(2^-scale).
pub fn exp_bucket_upper(scale: i32, index: i64) -> f64 {
    let base = 2f64.powf(2f64.powi(-scale));
    base.powf((index + 1) as f64)
}

/// Expand OTLP exponential buckets into explicit (bounds, counts) so the explicit-histogram paths
/// (heatmaps, per-point quantiles) work unchanged. Negative buckets are folded into a single
/// leading bucket below zero; the zero bucket becomes [.., 0]-ish first positive bucket.
pub fn expand_exponential(scale: i32, zero_count: u64, pos_offset: i32, pos_counts: &[u64], neg_counts_total: u64) -> (Vec<f64>, Vec<u64>) {
    let mut bounds = Vec::with_capacity(pos_counts.len() + 2);
    let mut counts = Vec::with_capacity(pos_counts.len() + 2);
    if neg_counts_total > 0 {
        bounds.push(0.0);
        counts.push(neg_counts_total);
    }
    // zero bucket: everything up to the first positive bucket's lower edge
    let first_lower = if pos_counts.is_empty() { 0.0 } else { exp_bucket_upper(scale, pos_offset as i64 - 1) };
    if zero_count > 0 || !pos_counts.is_empty() {
        bounds.push(first_lower);
        counts.push(zero_count);
    }
    for (i, c) in pos_counts.iter().enumerate() {
        bounds.push(exp_bucket_upper(scale, pos_offset as i64 + i as i64));
        counts.push(*c);
    }
    // OTLP explicit histograms have len(counts) = len(bounds) + 1 (last = overflow); add an empty overflow bucket
    counts.push(0);
    (bounds, counts)
}

#[cfg(test)]
mod exp_tests {
    use super::*;
    #[test]
    fn bounds_follow_base() {
        // scale 0: base 2 → bucket 0 covers (1, 2], bucket 3 covers (8, 16]
        assert!((exp_bucket_upper(0, 0) - 2.0).abs() < 1e-9);
        assert!((exp_bucket_upper(0, 3) - 16.0).abs() < 1e-9);
        // scale 1: base sqrt(2)
        assert!((exp_bucket_upper(1, 1) - 2.0).abs() < 1e-9);
        // negative scale -1: base 4
        assert!((exp_bucket_upper(-1, 1) - 16.0).abs() < 1e-9);
    }
    #[test]
    fn expansion_shapes() {
        let (b, c) = expand_exponential(0, 2, 1, &[5, 7], 0);
        // zero bucket up to lower edge of bucket 1 (=2), then (2,4] and (4,8]
        assert_eq!(b, vec![2.0, 4.0, 8.0]);
        assert_eq!(c, vec![2, 5, 7, 0]);
        assert_eq!(c.len(), b.len() + 1);
    }
}
