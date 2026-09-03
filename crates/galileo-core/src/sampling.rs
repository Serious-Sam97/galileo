//! Tail-based sampling policy, stored per project (`project_settings.sampling`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sampling {
    /// Share of ordinary traces to keep, 0.0–1.0. 1.0 disables buffering entirely.
    #[serde(default = "d_rate")]
    pub rate: f64,
    /// Always keep traces with an error span.
    #[serde(default = "d_true")]
    pub keep_errors: bool,
    /// Always keep traces whose root span is slower than this (0 = off).
    #[serde(default = "d_slow")]
    pub slow_ms: u64,
    /// Always keep traces with LLM (gen_ai) or browser (session.id) spans.
    #[serde(default = "d_true")]
    pub keep_llm: bool,
    /// How long a trace must be quiet before the keep/drop decision.
    #[serde(default = "d_delay")]
    pub decision_delay_secs: u64,
}
fn d_rate() -> f64 { 1.0 }
fn d_true() -> bool { true }
fn d_slow() -> u64 { 2000 }
fn d_delay() -> u64 { 10 }

impl Default for Sampling {
    fn default() -> Self {
        Self { rate: 1.0, keep_errors: true, slow_ms: 2000, keep_llm: true, decision_delay_secs: 10 }
    }
}

impl Sampling {
    pub fn is_passthrough(&self) -> bool {
        self.rate >= 1.0
    }
    pub fn normalized(mut self) -> Self {
        self.rate = self.rate.clamp(0.0, 1.0);
        self.decision_delay_secs = self.decision_delay_secs.clamp(1, 300);
        self
    }
    /// Deterministic per-trace coin flip: the same trace id always gets the same answer.
    pub fn coin(&self, trace_id: &[u8]) -> bool {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in trace_id { h ^= *b as u64; h = h.wrapping_mul(0x100000001b3); }
        ((h % 1_000_000) as f64) < self.rate * 1_000_000.0
    }
}
