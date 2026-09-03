//! Tail-based sampler: spans are held per (project, trace) until the trace has been quiet for the
//! project's decision delay, then the whole trace is kept or dropped at once. Keep when any span is
//! an error, the root is slow, or the trace carries LLM / browser spans; otherwise a deterministic
//! coin flip on the trace id at the project's rate.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use galileo_core::{ProjectId, Sampling, Span, StatusCode, TraceId};

/// Hard cap on buffered spans; when exceeded, the oldest traces are decided early.
pub const MAX_BUFFERED_SPANS: usize = 200_000;

struct TraceBuf {
    spans: Vec<Span>,
    policy: Sampling,
    last_seen: Instant,
    first_seen: Instant,
}

#[derive(Default)]
pub struct Sampler {
    traces: HashMap<(ProjectId, TraceId), TraceBuf>,
    buffered: usize,
    pub kept: u64,
    pub dropped: u64,
}

pub enum Decision { Keep, Drop }

pub fn decide(policy: &Sampling, spans: &[Span]) -> Decision {
    let mut interesting = false;
    for s in spans {
        if policy.keep_errors && s.status.code == StatusCode::Error { interesting = true; break; }
        if policy.slow_ms > 0 && s.parent_span_id.is_none() && s.duration_ns() >= policy.slow_ms * 1_000_000 { interesting = true; break; }
        if policy.keep_llm && (s.attributes.contains_key("gen_ai.system") || s.attributes.contains_key("session.id")) { interesting = true; break; }
    }
    if interesting { return Decision::Keep; }
    if let Some(s) = spans.first() {
        if policy.coin(&s.trace_id.0) { return Decision::Keep; }
    }
    Decision::Drop
}

impl Sampler {
    pub fn push(&mut self, project: ProjectId, policy: &Sampling, spans: Vec<Span>, now: Instant) {
        for s in spans {
            let e = self.traces.entry((project, s.trace_id)).or_insert_with(|| TraceBuf { spans: Vec::new(), policy: policy.clone(), last_seen: now, first_seen: now });
            e.last_seen = now;
            e.spans.push(s);
            self.buffered += 1;
        }
    }

    pub fn buffered(&self) -> usize { self.buffered }

    /// Decide every trace that has been quiet long enough (or everything, on `force`), returning
    /// the spans to write.
    pub fn drain(&mut self, now: Instant, force: bool) -> Vec<Span> {
        let mut out = Vec::new();
        let over_cap = self.buffered > MAX_BUFFERED_SPANS;
        let mut oldest: Vec<((ProjectId, TraceId), Instant)> = Vec::new();
        let keys: Vec<(ProjectId, TraceId)> = self.traces.iter().filter_map(|(k, b)| {
            let quiet = now.duration_since(b.last_seen) >= Duration::from_secs(b.policy.decision_delay_secs);
            let too_old = now.duration_since(b.first_seen) >= Duration::from_secs(b.policy.decision_delay_secs * 6 + 60);
            if over_cap { oldest.push((*k, b.first_seen)); }
            (force || quiet || too_old).then_some(*k)
        }).collect();
        let mut keys = keys;
        if over_cap {
            oldest.sort_by_key(|(_, t)| *t);
            let need = self.buffered.saturating_sub(MAX_BUFFERED_SPANS / 2);
            let mut freed = 0;
            for (k, _) in oldest {
                if freed >= need { break; }
                if let Some(b) = self.traces.get(&k) { freed += b.spans.len(); }
                if !keys.contains(&k) { keys.push(k); }
            }
        }
        for k in keys {
            if let Some(b) = self.traces.remove(&k) {
                self.buffered -= b.spans.len();
                match decide(&b.policy, &b.spans) {
                    Decision::Keep => { self.kept += 1; out.extend(b.spans); }
                    Decision::Drop => { self.dropped += 1; }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as CD, Utc};
    use galileo_core::{Attributes, SpanId, SpanKind, SpanStatus};

    fn span(trace: u8, root: bool, ms: i64, err: bool) -> Span {
        let now = Utc::now();
        Span {
            project_id: ProjectId(Default::default()), trace_id: TraceId([trace; 16]), span_id: SpanId::random(),
            parent_span_id: if root { None } else { Some(SpanId::random()) }, name: "x".into(), kind: SpanKind::Server,
            start_time: now - CD::milliseconds(ms), end_time: now,
            status: SpanStatus { code: if err { StatusCode::Error } else { StatusCode::Ok }, message: String::new() },
            service_name: "svc".into(), scope_name: String::new(), scope_version: String::new(),
            resource: Attributes::default(), attributes: Attributes::default(), events: vec![], links: vec![],
        }
    }
    fn policy(rate: f64) -> Sampling { Sampling { rate, keep_errors: true, slow_ms: 1000, keep_llm: true, decision_delay_secs: 1 } }

    #[test]
    fn keeps_errors_and_slow() {
        assert!(matches!(decide(&policy(0.0), &[span(1, true, 10, true)]), Decision::Keep));
        assert!(matches!(decide(&policy(0.0), &[span(2, true, 5000, false)]), Decision::Keep));
        assert!(matches!(decide(&policy(0.0), &[span(3, true, 10, false)]), Decision::Drop));
        assert!(matches!(decide(&policy(1.0), &[span(3, true, 10, false)]), Decision::Keep));
    }
    #[test]
    fn rate_is_deterministic_and_roughly_right() {
        let p = policy(0.3);
        let a: Vec<bool> = (0..200u8).map(|i| matches!(decide(&p, &[span(i, true, 1, false)]), Decision::Keep)).collect();
        let b: Vec<bool> = (0..200u8).map(|i| matches!(decide(&p, &[span(i, true, 1, false)]), Decision::Keep)).collect();
        assert_eq!(a, b);
        let kept = a.iter().filter(|x| **x).count();
        assert!((30..=90).contains(&kept), "kept {kept} of 200 at 30%");
    }
    #[test]
    fn drains_only_quiet_traces() {
        let mut s = Sampler::default();
        let t0 = Instant::now();
        s.push(ProjectId(Default::default()), &policy(1.0), vec![span(1, true, 1, false), span(1, false, 1, false)], t0);
        assert!(s.drain(t0, false).is_empty());
        let out = s.drain(t0 + Duration::from_secs(2), false);
        assert_eq!(out.len(), 2);
        assert_eq!(s.buffered(), 0);
        assert_eq!(s.kept, 1);
    }
}
