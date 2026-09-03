# Galileo — observability + AI control plane (Rust backend, Next.js frontend)

Rules: tick boxes only when the item is really done and compiles/tests pass. Never git commit or push.
Project root: ~/Developer/galileo. Backend in crates/, frontend in web/, infra in deploy/.

## Phase 0 — scaffold
- [x] Cargo workspace with crates: galileo-core, galileo-storage, galileo-otlp, galileo-query, galileo-gateway, galileo-alerts, galileo-api, galileo-server
- [x] deploy/docker-compose.yml with ClickHouse (8123/9000) and Postgres (5433); .env.example; config loading (galileo.toml + env)
- [x] README.md with architecture overview and how to run
- [x] `cargo build` green on the empty workspace

## Phase 1 — core domain + storage
- [x] galileo-core: Span, SpanEvent, SpanLink, LogRecord, MetricPoint, AttributeValue, ProjectId, redaction rules (regex + key based) with unit tests
- [x] galileo-storage: Storage trait (write_spans/logs/metrics, query_sql, fetch_trace); ClickHouse impl using `clickhouse` crate
- [x] ClickHouse schema migrations (spans, logs, metrics_* tables) with Map attrs + materialized hot columns (service, http.route, http.status, user.id, tenant.id, gen_ai.*), project_id partitioning, TTL retention
- [x] Storage integration test against docker ClickHouse (ignored by default, `cargo test -- --ignored`)

## Phase 2 — OTLP ingest
- [x] galileo-otlp: gRPC receiver (tonic, opentelemetry-proto) for Traces/Logs/Metrics services on :4317
- [x] HTTP receiver (axum) for /v1/traces /v1/logs /v1/metrics on :4318, protobuf + JSON bodies
- [x] proto -> core conversion with unit tests; API-key auth (Authorization: Bearer / x-galileo-key) resolving project_id
- [x] batching writer (channel + flush by size/time) with backpressure; ingest metrics counters
- [x] verify: send spans with otel-cli or a small Rust example and see rows in ClickHouse

## Phase 3 — metadata + API server
- [x] Postgres via sqlx: orgs, users, projects, api_keys (hashed), saved_queries, boards, triggers, slos, gateway_routes, prompts, redaction_rules; embedded migrations
- [x] galileo-api: axum router, session auth (email+password, argon2, cookie), project scoping middleware, JSON error type
- [x] CRUD endpoints: projects, api keys (show once), redaction rules, saved queries, boards
- [x] galileo-server binary wires otlp + api + gateway + alerts on one process; graceful shutdown; tracing logs

## Phase 4 — query engine
- [x] galileo-query: Query DSL (time range, calculations COUNT/COUNT_DISTINCT/SUM/AVG/MIN/MAX/P50/P90/P95/P99/HEATMAP, filters =,!=,>,<,>=,<=,contains,exists,in, breakdowns, orders, limit, granularity) -> safe ClickHouse SQL; unit tests on generated SQL
- [x] endpoints: POST /api/projects/:id/query (time series + table), GET trace by id (waterfall tree), GET spans raw, field/value autocomplete (column + attr keys with counts)
- [x] BubbleUp: POST /api/projects/:id/bubbleup — attribute distributions inside vs outside selection, ranked by divergence
- [x] logs search endpoint (full-text + filters) and metrics explorer endpoint (series by metric name + labels)
- [x] verify with curl against ingested demo data

## Phase 5 — LLM gateway
- [x] galileo-gateway: provider adapters — Anthropic Messages API, OpenAI-compatible chat completions (OpenAI, Ollama, vLLM); streaming passthrough (SSE) and non-streaming
- [x] routes: alias model -> provider+model, ordered fallbacks, per-route and per-project budgets (tokens/USD per day), rate limits; provider keys stored encrypted
- [x] every call recorded as a span using gen_ai.* semantic conventions (model, tokens in/out, cost, latency, finish reason, prompt/completion with redaction) joined to the caller's trace via traceparent
- [x] endpoints exposed: POST /gw/v1/messages and POST /gw/v1/chat/completions; cost table for known models
- [x] prompt registry: versioned prompts with variables, GET by name@version, usage recorded on spans
- [x] verify against local Ollama (11434) streaming and non-streaming

## Phase 6 — alerts + SLOs
- [x] triggers: saved query + threshold + frequency, evaluator loop, state (ok/triggered), notifications via webhook + Slack webhook, dedupe
- [x] SLOs: SLI from query (good/total), target, time window, budget remaining, burn-rate alerts
- [x] endpoints for triggers/SLOs CRUD + history

## Phase 7 — frontend (web/, Next.js)
- [x] scaffold Next.js App Router + TS + Tailwind + TanStack Query + ECharts; API client; auth pages; project switcher
- [x] Query builder page: visualize/where/group-by/order/limit, time picker, results as time series + heatmap + table, run/save
- [x] Trace waterfall page with span details drawer, attributes, events, links
- [x] BubbleUp view (select region on heatmap/time series -> ranked attribute charts)
- [x] Logs page (search, live tail via polling) and Metrics page
- [x] AI page: gateway routes editor, calls explorer, cost/token dashboards, prompt registry
- [x] Triggers + SLOs pages, Boards (dashboards of saved queries), Settings (api keys, redaction rules)

## Phase 8 — end to end
- [x] examples/demo-app: small Rust axum app instrumented with OTel SDK + calling the gateway (Ollama) so traces contain LLM spans
- [x] run stack via docker compose + `cargo run` + `pnpm dev`; walk through: ingest -> query -> trace -> bubbleup -> AI calls -> trigger fires
- [x] docs/: connecting an app (OTel SDK snippets for Rust/Node/Python/PHP), gateway usage, query DSL reference
- [x] final: cargo clippy clean, cargo test green, pnpm build green
