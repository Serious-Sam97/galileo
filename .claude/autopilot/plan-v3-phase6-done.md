# Galileo V3 — Phase 6 (SDKs for your stacks)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate each SDK against the running Galileo (Default project key) with a small example.

## Python (generic)
- [x] `sdk/python/galileo_python/` package (`galileo-python`): `galileo.init(endpoint, api_key, service, env, release)`; OTel SDK setup + OTLP/HTTP exporters (traces + logs); identity contextvar (`set_identity()`); call-site `code.*` on client spans via a span processor (reusing `galileo_django.callsite/frames` logic, framework-agnostic skip list); `@traced` and `capture_exception()`; logging handler with trace ids + identity; guardrail-free gateway helper `galileo.gateway.headers()`; agent run/step helpers shared with the Django package
- [x] framework hooks: FastAPI/Starlette middleware (route template names, user from `request.state.user` or a resolver), Flask (before/after request), SQLAlchemy engine events → DB spans with call site, httpx + requests instrumentation via OTel contrib, Celery task spans; each optional via extras
- [x] tests (pytest) for call-site, identity propagation, logging handler; example FastAPI app sends a request with a SQLAlchemy query and an exception; docs/sdk-python.md gains the generic section; `galileo_django` re-exports shared pieces

## Rust
- [x] `sdk/rust/galileo` crate: `galileo::init(Config { endpoint, api_key, service, env })` builds a `tracing` subscriber layer + OTLP/HTTP exporter (opentelemetry-otlp) with resource attributes; `galileo::identity::set(user, tenant)` (task-local) attached to every span; call-site `code.*` from `tracing` span metadata (file/line/module); axum middleware (`galileo::axum::layer()` → server span per request with route template, status, identity from an extension); sqlx query spans via a `tracing` span helper (`galileo::sql!` / `traced_query`) with normalised statement and table; `capture_error()`; log bridge from `tracing` events to OTLP logs
- [x] example axum app (in the crate's examples) + tests; docs/sdk-rust.md; Galileo's own server can enable self-tracing with it (feature flag `self-trace`, off by default)

## Android / Kotlin
- [x] `sdk/android/galileo/` Gradle library module (Kotlin, min SDK 24): `Galileo.init(context, endpoint, apiKey, service)`; OkHttp interceptor (client spans + `traceparent`), Retrofit works through it; Room/SQLite spans via a `traced` query helper; screen views from `ActivityLifecycleCallbacks` (page views with `session.id`, `screen.name`); crashes (uncaught handler) and ANR watchdog as `exception.*` error spans + logs; cold-start time and frame drops (`FrameMetrics` where available) as `browser.web_vital`-style gauges named `mobile.vital.*`; identity `Galileo.setUser()`; batching + OTLP/JSON over HTTP with retry and offline queue (file)
- [x] unit tests for the OTLP encoder and interceptor; sample app module; docs/sdk-android.md; Galileo UI: the Browser page treats `mobile.*` sessions the same as web (session list shows platform)

## Wrap-up
- [x] Settings → Connect shows Python (generic), Rust and Android snippets; docs/connecting.md links; memory; plan archived
