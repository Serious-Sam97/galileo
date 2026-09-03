# Rust SDK (`galileo` crate, `sdk/rust/galileo`)

```toml
[dependencies]
galileo = { path = "../galileo/sdk/rust/galileo" }   # or the published crate
```

```rust
let _guard = galileo::init(galileo::Config::from_env().service("odeon"));
let app = axum::Router::new().route("/media/{id}", get(media)).layer(axum::middleware::from_fn(galileo::axum::middleware));

async fn media(Path(id): Path<u64>) -> Json<Media> {
    galileo::identity::set(Identity { user_id: Some(user.id.to_string()), ..Default::default() });
    let row = galileo::sql!("SELECT * FROM media WHERE id = $1", "postgresql", sqlx::query_as(...).fetch_one(&pool));
    let total = galileo::traced!("price", { compute(&row) });
    if let Err(e) = risky() { galileo::capture_error(&e); }
    ...
}
```

- `init` installs a `tracing` subscriber layer that exports spans and events (logs) over OTLP/HTTP
  with `service.name`, `deployment.environment`, `service.version` from `Config` or the env
  (`GALILEO_ENDPOINT`, `GALILEO_API_KEY`, `OTEL_SERVICE_NAME`, `GALILEO_ENV`, `GALILEO_RELEASE`,
  `GALILEO_FILTER`).
- `axum::middleware::from_fn(galileo::axum::middleware)` gives one SERVER span per request named by the route template, honours
  incoming `traceparent`, sets `x-galileo-trace-id` on the response, and scopes the request's identity
  (put a `galileo::identity::Identity` in the request extensions from your auth layer, or call
  `identity::set` inside the handler).
- `sql!(statement, system, future)` wraps a query future in a CLIENT span with `db.statement`,
  `db.operation`, `db.sql.table` and the call site (`code.file.path`, `code.line.number`,
  `code.namespace`), so N+1 detection and "callers" work like in the Python SDK.
- `traced!(name, { ... })` for your own functions; `capture_error(&err)` records `exception.*` and an
  error status so Issues groups it.
- `identity::gateway_headers()` gives `x-galileo-user-id` / `x-galileo-tenant-id` for gateway calls.

Example: `cargo run --example axum_app` (GET `:3007/orders/42`, `/orders/13` records an error).
