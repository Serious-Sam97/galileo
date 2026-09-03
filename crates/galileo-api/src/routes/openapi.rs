//! OpenAPI document (from docs/openapi.yaml) and an interactive reference page.

use axum::response::{Html, IntoResponse};
use axum::Json;

const SPEC: &str = include_str!("../../../../docs/openapi.yaml");

pub async fn spec() -> axum::response::Response {
    match serde_yaml::from_str::<serde_json::Value>(SPEC) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("openapi.yaml: {e}")).into_response(),
    }
}

pub async fn docs() -> Html<&'static str> {
    Html(r#"<!doctype html><html><head><meta charset="utf-8"><title>Galileo API</title><meta name="viewport" content="width=device-width, initial-scale=1"></head>
<body style="margin:0"><script id="api-reference" data-url="/api/openapi.json"></script>
<script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script></body></html>"#)
}
