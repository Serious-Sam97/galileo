//! Exact-match response cache for non-streaming gateway calls. Keyed by the upstream body (which
//! already includes the resolved model and the injected prompt), per project. In memory, TTL per
//! route, bounded size.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde_json::Value;
use uuid::Uuid;

use crate::formats::Observed;

const MAX_ENTRIES: usize = 5000;

pub struct Entry {
    pub body: Value,
    pub observed: Observed,
    pub at: Instant,
    pub ttl: Duration,
}

#[derive(Default)]
pub struct ResponseCache {
    map: DashMap<(Uuid, u64), Entry>,
}

pub fn key(body: &Value) -> u64 {
    let s = body.to_string();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl ResponseCache {
    pub fn get(&self, project: Uuid, k: u64) -> Option<(Value, Observed)> {
        let e = self.map.get(&(project, k))?;
        if e.at.elapsed() > e.ttl {
            drop(e);
            self.map.remove(&(project, k));
            return None;
        }
        Some((e.body.clone(), e.observed.clone()))
    }

    pub fn put(&self, project: Uuid, k: u64, body: Value, observed: Observed, ttl: Duration) {
        if self.map.len() >= MAX_ENTRIES {
            // cheap eviction: drop expired, then anything
            self.map.retain(|_, e| e.at.elapsed() <= e.ttl);
            if self.map.len() >= MAX_ENTRIES {
                if let Some(k0) = self.map.iter().next().map(|e| *e.key()) {
                    self.map.remove(&k0);
                }
            }
        }
        self.map.insert((project, k), Entry { body, observed, at: Instant::now(), ttl });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_expiry() {
        let c = ResponseCache::default();
        let p = Uuid::new_v4();
        let k = key(&serde_json::json!({"a": 1}));
        assert_eq!(k, key(&serde_json::json!({"a": 1})));
        assert_ne!(k, key(&serde_json::json!({"a": 2})));
        c.put(p, k, serde_json::json!({"x": 1}), Observed::default(), Duration::from_millis(30));
        assert!(c.get(p, k).is_some());
        assert!(c.get(Uuid::new_v4(), k).is_none());
        std::thread::sleep(Duration::from_millis(40));
        assert!(c.get(p, k).is_none());
    }
}
