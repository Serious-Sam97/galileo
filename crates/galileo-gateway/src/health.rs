//! Live per-target health for smart routing: error rate and p95 over a sliding window.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

const WINDOW: Duration = Duration::from_secs(300);
const MAX_SAMPLES: usize = 500;

type Samples = VecDeque<(Instant, bool, f64)>;

#[derive(Default)]
pub struct Health { inner: Mutex<HashMap<(Uuid, Uuid, String), Samples>> }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats { pub calls: usize, pub error_rate: f64, pub p95_ms: f64 }

impl Health {
    pub fn record(&self, project: Uuid, route: Uuid, model: &str, ok: bool, ms: f64) {
        if let Ok(mut g) = self.inner.lock() {
            let q = g.entry((project, route, model.to_string())).or_default();
            q.push_back((Instant::now(), ok, ms));
            while q.len() > MAX_SAMPLES { q.pop_front(); }
        }
    }
    pub fn stats(&self, project: Uuid, route: Uuid, model: &str) -> Stats {
        let Ok(mut g) = self.inner.lock() else { return Stats { calls: 0, error_rate: 0.0, p95_ms: 0.0 } };
        let Some(q) = g.get_mut(&(project, route, model.to_string())) else { return Stats { calls: 0, error_rate: 0.0, p95_ms: 0.0 } };
        let cutoff = Instant::now() - WINDOW;
        while q.front().map(|(t, _, _)| *t < cutoff).unwrap_or(false) { q.pop_front(); }
        if q.is_empty() { return Stats { calls: 0, error_rate: 0.0, p95_ms: 0.0 }; }
        let errors = q.iter().filter(|(_, ok, _)| !ok).count();
        let mut lat: Vec<f64> = q.iter().filter(|(_, ok, _)| *ok).map(|(_, _, ms)| *ms).collect();
        lat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p95 = if lat.is_empty() { 0.0 } else { lat[((lat.len() as f64 * 0.95).ceil() as usize).clamp(1, lat.len()) - 1] };
        Stats { calls: q.len(), error_rate: errors as f64 / q.len() as f64, p95_ms: p95 }
    }
    /// Order target indices by health: unhealthy last (skipped first), then lowest error rate, then p95.
    /// Targets without data keep their configured position among the healthy ones.
    pub fn order(&self, project: Uuid, route: Uuid, models: &[String]) -> Vec<usize> {
        let stats: Vec<Stats> = models.iter().map(|m| self.stats(project, route, m)).collect();
        let unhealthy = |s: &Stats| s.calls >= 5 && s.error_rate > 0.5;
        let mut idx: Vec<usize> = (0..models.len()).collect();
        idx.sort_by(|&a, &b| {
            let (sa, sb) = (&stats[a], &stats[b]);
            unhealthy(sa).cmp(&unhealthy(sb))
                .then(sa.error_rate.partial_cmp(&sb.error_rate).unwrap_or(std::cmp::Ordering::Equal))
                .then(sa.p95_ms.partial_cmp(&sb.p95_ms).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.cmp(&b))
        });
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unhealthy_target_goes_last() {
        let h = Health::default();
        let (p, r) = (Uuid::nil(), Uuid::nil());
        for _ in 0..6 { h.record(p, r, "a", false, 10.0); }
        for _ in 0..6 { h.record(p, r, "b", true, 300.0); }
        for _ in 0..6 { h.record(p, r, "c", true, 50.0); }
        assert_eq!(h.order(p, r, &["a".into(), "b".into(), "c".into()]), vec![2, 1, 0]);
        assert!(h.stats(p, r, "a").error_rate > 0.99);
    }
}
