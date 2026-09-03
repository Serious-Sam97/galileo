//! HTTP API consumed by the Galileo UI. Metadata lives in Postgres; event data is read
//! through the query layer.

pub mod audit;
pub mod auth;
pub mod db;
pub mod error;
pub mod resolver;
pub mod routes;
pub mod state;

pub use error::{ApiError, ApiResult};
pub use resolver::PgResolver;
pub use state::AppState;

use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

/// Run Postgres migrations embedded at compile time.
pub async fn migrate(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// The full `/api` router plus CORS and request tracing.
pub fn router(state: AppState) -> Router {
    let origins: Vec<axum::http::HeaderValue> = state
        .config
        .server
        .cors_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
        ])
        .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION]);

    let gateway = galileo_gateway::router(state.gateway.clone());
    Router::new()
        .nest("/api", routes::api_router())
        // the browser script lives next to the UI/API origin: <script src="…/rum.js">
        .route("/rum.js", axum::routing::get(routes::sessions::rum_js))
        .with_state(state)
        .nest("/gw", gateway)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}
