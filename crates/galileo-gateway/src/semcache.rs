//! Semantic response cache: near-duplicate prompts served from memory. Embeddings come from a
//! built-in hashed-token model (no network) unless the route names an embedding route.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::formats::Observed;

pub const DIMS: usize = 512;
const MAX_PER_ROUTE: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCache {
    /// Cosine similarity needed for a hit (0.85–0.99 sensible; 0.92 default).
    #[serde(default = "d_thr")] pub threshold: f32,
    #[serde(default = "d_ttl")] pub ttl_secs: u64,
    /// Gateway route whose first target serves `/embeddings`; absent = built-in hashed embedding.
    #[serde(default)] pub embed_route: Option<String>,
}
fn d_thr() -> f32 { 0.92 }
fn d_ttl() -> u64 { 3600 }

/// Hashed token n-grams (unigrams + bigrams), L2-normalised. Cheap, deterministic, good enough
/// for paraphrases that share vocabulary; a real embedding route beats it for synonyms.
pub fn embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIMS];
    let toks: Vec<String> = text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|t| t.len() > 1 && !STOP.contains(t)).map(str::to_owned).collect();
    let mut bump = |s: &str, w: f32| { let mut h: u64 = 0xcbf29ce484222325; for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); } let sign = if (h >> 63) == 1 { -1.0 } else { 1.0 }; v[(h % DIMS as u64) as usize] += sign * w; };
    for t in &toks { bump(t, 1.0); }
    for w in toks.windows(2) { bump(&format!("{} {}", w[0], w[1]), 0.6); }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 { for x in v.iter_mut() { *x /= norm; } }
    v
}

const STOP: &[&str] = &["the", "and", "for", "with", "that", "this", "you", "are", "please", "can", "what", "about", "how", "from", "into", "your", "our", "its"];

pub fn cosine(a: &[f32], b: &[f32]) -> f32 { a.iter().zip(b).map(|(x, y)| x * y).sum() }

/// Conversation bucket: system prompt + message count. Only the LAST user message is embedded,
/// so paraphrased questions match while different conversation positions never do.
pub fn bucket_and_text(req: &crate::formats::NormRequest) -> (u64, String) {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in req.system.as_deref().unwrap_or("").bytes().chain(req.messages.len().to_string().bytes()) { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); }
    let last_user = req.messages.iter().rev().find(|m| m.role == "user").map(|m| m.content()).unwrap_or_default();
    (h, last_user)
}

struct Entry { bucket: u64, vec: Vec<f32>, body: Value, observed: Observed, expires: Instant }

#[derive(Default)]
pub struct Store { inner: Mutex<HashMap<(Uuid, Uuid), Vec<Entry>>> }

impl Store {
    /// `bucket` separates conversations that must never share answers: it hashes the system prompt
    /// and the number of messages, so turn 3 of a chat cannot be served turn 2's reply.
    pub fn get(&self, project: Uuid, route: Uuid, bucket: u64, vec: &[f32], threshold: f32) -> Option<(Value, Observed, f32)> {
        let mut g = self.inner.lock().ok()?;
        let list = g.get_mut(&(project, route))?;
        let now = Instant::now();
        list.retain(|e| e.expires > now);
        let mut best: Option<(f32, usize)> = None;
        for (i, e) in list.iter().enumerate() { if e.bucket != bucket { continue; } let s = cosine(&e.vec, vec); if s >= threshold && best.map(|(b, _)| s > b).unwrap_or(true) { best = Some((s, i)); } }
        best.map(|(s, i)| (list[i].body.clone(), list[i].observed.clone(), s))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn put(&self, project: Uuid, route: Uuid, bucket: u64, vec: Vec<f32>, body: Value, observed: Observed, ttl: Duration) {
        if let Ok(mut g) = self.inner.lock() {
            let list = g.entry((project, route)).or_default();
            if list.len() >= MAX_PER_ROUTE { list.remove(0); }
            list.push(Entry { bucket, vec, body, observed, expires: Instant::now() + ttl });
        }
    }
    pub fn len(&self) -> usize { self.inner.lock().map(|g| g.values().map(|v| v.len()).sum()).unwrap_or(0) }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paraphrases_are_close_and_unrelated_far() {
        let a = embed("What is the return policy for damaged items?");
        let b = embed("what's the return policy for a damaged item");
        let c = embed("Schedule a vaccination appointment for my dog tomorrow");
        assert!(cosine(&a, &b) > 0.6, "sim {}", cosine(&a, &b));
        assert!(cosine(&a, &c) < 0.3, "sim {}", cosine(&a, &c));
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-4);
    }
    #[test]
    fn store_hits_and_expires() {
        let s = Store::default();
        let p = Uuid::nil(); let r = Uuid::nil();
        s.put(p, r, 1, embed("hello world"), serde_json::json!({"ok": 1}), Observed::default(), Duration::from_millis(50));
        assert!(s.get(p, r, 1, &embed("hello world"), 0.9).is_some());
        assert!(s.get(p, r, 2, &embed("hello world"), 0.9).is_none(), "different bucket must not hit");
        assert!(s.get(p, r, 1, &embed("completely different"), 0.9).is_none());
        std::thread::sleep(Duration::from_millis(60));
        assert!(s.get(p, r, 1, &embed("hello world"), 0.9).is_none());
    }
}
