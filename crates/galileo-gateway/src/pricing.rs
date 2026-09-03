//! USD per million tokens. Matched by longest prefix so dated snapshots and provider prefixes
//! (`anthropic.claude-...`) resolve. Unknown models cost 0 and are flagged so the UI can say
//! "price unknown" instead of "free".

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Price {
    pub const fn anthropic(input: f64, output: f64) -> Self {
        Self { input, output, cache_read: input * 0.1, cache_write: input * 1.25 }
    }
    pub const fn simple(input: f64, output: f64) -> Self {
        Self { input, output, cache_read: input * 0.5, cache_write: input }
    }
    pub const fn free() -> Self {
        Self { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0 }
    }

    pub fn cost(&self, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
        (input as f64 * self.input + output as f64 * self.output + cache_read as f64 * self.cache_read + cache_write as f64 * self.cache_write)
            / 1_000_000.0
    }
}

// (prefix, price). Order matters only for equal-length prefixes; lookup picks the longest match.
const TABLE: &[(&str, Price)] = &[
    // Anthropic (first-party rates, 2026-06)
    ("claude-fable-5-1", Price { input: 10.0, output: 50.0, cache_read: 0.25, cache_write: 12.5 }),
    ("claude-fable-5", Price::anthropic(10.0, 50.0)),
    ("claude-mythos-5", Price::anthropic(10.0, 50.0)),
    ("claude-opus-5", Price::anthropic(5.0, 25.0)),
    ("claude-opus-4-8", Price::anthropic(5.0, 25.0)),
    ("claude-opus-4-7", Price::anthropic(5.0, 25.0)),
    ("claude-opus-4-6", Price::anthropic(5.0, 25.0)),
    ("claude-opus-4-5", Price::anthropic(5.0, 25.0)),
    ("claude-opus-4", Price::anthropic(15.0, 75.0)),
    ("claude-sonnet-5", Price::anthropic(2.0, 10.0)),
    ("claude-sonnet-4-6", Price::anthropic(3.0, 15.0)),
    ("claude-sonnet-4-5", Price::anthropic(3.0, 15.0)),
    ("claude-sonnet-4", Price::anthropic(3.0, 15.0)),
    ("claude-haiku-4-5", Price::anthropic(1.0, 5.0)),
    ("claude-3-5-haiku", Price::anthropic(0.8, 4.0)),
    // OpenAI (best effort; override per route target when they change)
    ("gpt-4o-mini", Price::simple(0.15, 0.60)),
    ("gpt-4o", Price::simple(2.5, 10.0)),
    ("gpt-4.1-nano", Price::simple(0.10, 0.40)),
    ("gpt-4.1-mini", Price::simple(0.40, 1.60)),
    ("gpt-4.1", Price::simple(2.0, 8.0)),
    ("gpt-5-nano", Price::simple(0.05, 0.40)),
    ("gpt-5-mini", Price::simple(0.25, 2.0)),
    ("gpt-5", Price::simple(1.25, 10.0)),
    ("o4-mini", Price::simple(1.1, 4.4)),
    ("o3", Price::simple(2.0, 8.0)),
    // embeddings (output is 0)
    ("text-embedding-3-small", Price::simple(0.02, 0.0)),
    ("text-embedding-3-large", Price::simple(0.13, 0.0)),
    ("text-embedding-ada-002", Price::simple(0.10, 0.0)),
    ("voyage-3", Price::simple(0.06, 0.0)),
];

/// Audio transcription is priced per minute; returned as USD per minute.
pub fn audio_price_per_minute(model: &str) -> Option<f64> {
    let m = model.to_ascii_lowercase();
    if m.contains("whisper-large-v3-turbo") { Some(0.04 / 60.0 * 60.0 * 0.0011) } // Groq: $0.04/hour
    else if m.contains("whisper-large-v3") { Some(0.111 / 60.0) }                   // Groq: $0.111/hour
    else if m.contains("whisper") { Some(0.006) }                                    // OpenAI whisper-1: $0.006/min
    else if m.contains("gpt-4o-transcribe") || m.contains("gpt-4o-mini-transcribe") { Some(0.003) }
    else { None }
}

pub struct Lookup {
    pub price: Price,
    pub known: bool,
}

pub fn lookup(model: &str) -> Lookup {
    let m = model.trim().to_ascii_lowercase();
    // strip provider prefixes like "anthropic." or "openai/"
    let m = m.rsplit(['.', '/']).next().map(str::to_owned).unwrap_or(m.clone());
    let m = if m.starts_with("claude") || m.starts_with("gpt") || m.starts_with('o') { m } else { model.trim().to_ascii_lowercase() };
    let mut best: Option<(&str, Price)> = None;
    for (prefix, price) in TABLE {
        if m.starts_with(prefix) && best.map(|(b, _)| prefix.len() > b.len()).unwrap_or(true) {
            best = Some((prefix, *price));
        }
    }
    match best {
        Some((_, p)) => Lookup { price: p, known: true },
        None => Lookup { price: Price::free(), known: false },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_wins() {
        assert_eq!(lookup("claude-opus-5").price.input, 5.0);
        assert_eq!(lookup("claude-opus-4-6").price.input, 5.0);
        assert_eq!(lookup("claude-opus-4-1-20250805").price.input, 15.0);
        assert_eq!(lookup("gpt-4o-mini-2024-07-18").price.output, 0.60);
        assert_eq!(lookup("gpt-4o").price.output, 10.0);
        assert!(!lookup("llama3.2").known);
        assert_eq!(lookup("llama3.2").price.cost(1000, 1000, 0, 0), 0.0);
    }

    #[test]
    fn cost_math() {
        let p = lookup("claude-sonnet-5").price;
        let c = p.cost(1_000_000, 100_000, 0, 0);
        assert!((c - 3.0).abs() < 1e-9);
    }
}
