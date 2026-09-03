//! Rate limits (in-memory token buckets) and budgets (spend so far, read from ClickHouse and
//! cached for a short time).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use galileo_core::ProjectId;
use galileo_storage::{DynStorage, SqlQuery};
use uuid::Uuid;

const BUDGET_TTL: Duration = Duration::from_secs(20);

struct Bucket {
    tokens: f64,
    last: Instant,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Spend {
    pub day_usd: f64,
    pub day_tokens: u64,
    pub month_usd: f64,
}

struct SpendEntry {
    spend: Spend,
    at: Instant,
}

#[derive(Default)]
pub struct Limiter {
    buckets: DashMap<(Uuid, Uuid), Mutex<Bucket>>,
    spend: DashMap<(Uuid, Uuid), SpendEntry>,
}

impl Limiter {
    /// Token bucket: `rpm` capacity refilled at rpm/60 per second. Returns false when the
    /// request should be rejected.
    pub fn allow(&self, project: Uuid, route: Uuid, rpm: u32) -> bool {
        if rpm == 0 {
            return true;
        }
        let cap = rpm as f64;
        let rate = cap / 60.0;
        let entry = self
            .buckets
            .entry((project, route))
            .or_insert_with(|| Mutex::new(Bucket { tokens: cap, last: Instant::now() }));
        let mut b = entry.lock().unwrap();
        let now = Instant::now();
        b.tokens = (b.tokens + now.duration_since(b.last).as_secs_f64() * rate).min(cap);
        b.last = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Spend so far today / this month for one route.
    pub async fn spend(&self, storage: &DynStorage, project: ProjectId, route_id: Uuid, alias: &str) -> Spend {
        if let Some(e) = self.spend.get(&(project.0, route_id)) {
            if e.at.elapsed() < BUDGET_TTL {
                return e.spend;
            }
        }
        let q = SqlQuery {
            sql: "SELECT sumIf(gen_ai_cost_usd, timestamp >= toStartOfDay(now())) AS day_usd, \
                  sumIf(gen_ai_input_tokens + gen_ai_output_tokens, timestamp >= toStartOfDay(now())) AS day_tokens, \
                  sum(gen_ai_cost_usd) AS month_usd \
                  FROM spans WHERE project_id = ? AND timestamp >= toStartOfMonth(now()) AND attrs['gen_ai.galileo.route'] = ?"
                .into(),
            params: vec![project.into(), alias.into()],
        };
        let spend = match storage.query(&q).await {
            Ok(r) => r
                .rows
                .first()
                .map(|row| {
                    let f = |v: &serde_json::Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
                    Spend { day_usd: f(&row[0]), day_tokens: f(&row[1]) as u64, month_usd: f(&row[2]) }
                })
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!(error = %e, "budget lookup failed; allowing request");
                Spend::default()
            }
        };
        self.spend.insert((project.0, route_id), SpendEntry { spend, at: Instant::now() });
        spend
    }

    /// Add a just-finished call to the cached spend so bursts cannot blow through a budget
    /// during the cache TTL.
    pub fn record(&self, project: Uuid, route_id: Uuid, usd: f64, tokens: u64) {
        if let Some(mut e) = self.spend.get_mut(&(project, route_id)) {
            e.spend.day_usd += usd;
            e.spend.month_usd += usd;
            e.spend.day_tokens += tokens;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_limits_bursts() {
        let l = Limiter::default();
        let p = Uuid::new_v4();
        let r = Uuid::new_v4();
        assert!((0..3).all(|_| l.allow(p, r, 3)));
        assert!(!l.allow(p, r, 3));
        assert!(l.allow(p, r, 0), "0 = unlimited");
    }
}
