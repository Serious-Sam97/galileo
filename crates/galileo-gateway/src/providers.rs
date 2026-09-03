//! Building and sending the upstream request.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::formats::Format;
use crate::routing::{Provider, ProviderKind};

pub struct Upstream {
    pub url: String,
    pub headers: HeaderMap,
    pub body: Value,
}

pub fn build(provider: &Provider, body: Value, client_headers: &axum::http::HeaderMap) -> Upstream {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("accept", HeaderValue::from_static("application/json, text/event-stream"));
    for (k, v) in &provider.headers {
        if let (Ok(k), Ok(v)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            headers.insert(k, v);
        }
    }
    let base = provider.base_url.trim_end_matches('/');
    let url = match provider.kind.format() {
        Format::Anthropic => {
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            if let Some(k) = &provider.api_key {
                if let Ok(v) = HeaderValue::from_str(k) {
                    headers.insert("x-api-key", v);
                }
            }
            // Beta features the client opted into still apply upstream.
            if let Some(b) = client_headers.get("anthropic-beta") {
                headers.insert("anthropic-beta", b.clone());
            }
            let base = base.strip_suffix("/v1").unwrap_or(base);
            format!("{base}/v1/messages")
        }
        Format::Openai => {
            if let Some(k) = &provider.api_key {
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {k}")) {
                    headers.insert("authorization", v);
                }
            }
            let base = base.strip_suffix("/v1").unwrap_or(base);
            format!("{base}/v1/chat/completions")
        }
    };
    Upstream { url, headers, body }
}

pub fn openai_strict(kind: ProviderKind) -> bool {
    kind == ProviderKind::Openai
}

/// Failures that justify trying the next target.
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504 | 529)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn urls_and_headers() {
        let p = Provider { id: Uuid::new_v4(), name: "a".into(), kind: ProviderKind::Anthropic, base_url: "https://api.anthropic.com/v1/".into(), api_key: Some("k".into()), headers: vec![] };
        let u = build(&p, serde_json::json!({}), &axum::http::HeaderMap::new());
        assert_eq!(u.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(u.headers["x-api-key"], "k");
        assert_eq!(u.headers["anthropic-version"], "2023-06-01");
        let o = Provider { id: Uuid::new_v4(), name: "o".into(), kind: ProviderKind::Ollama, base_url: "http://127.0.0.1:11434".into(), api_key: None, headers: vec![] };
        let u = build(&o, serde_json::json!({}), &axum::http::HeaderMap::new());
        assert_eq!(u.url, "http://127.0.0.1:11434/v1/chat/completions");
        assert!(u.headers.get("authorization").is_none());
    }
}
