# Galileo

Observability and AI control plane for your own apps. Honeycomb-style high-cardinality
tracing, logs and metrics; Datadog-style triggers, SLOs and boards; and an LLM gateway that
proxies every model call, records it as a span in the same trace as the request that caused
it, and lets you switch models, set budgets and version prompts without redeploying.

## Architecture

```
your app ── OTel SDK ──► OTLP gRPC :4317 / HTTP :4318 ─┐
                                                       ├──► galileo-server ──► ClickHouse (events)
your app ── LLM calls ──► gateway :8080/gw/v1/* ────────┘         │             Postgres  (metadata)
                                                                  ▼
                                              web/ (Next.js) ◄── API :8080/api/*
```

| crate              | responsibility                                                                 |
|--------------------|--------------------------------------------------------------------------------|
| `galileo-core`     | domain types (spans, logs, metrics, attributes), config, redaction rules       |
| `galileo-storage`  | `Storage` trait + ClickHouse implementation, schema migrations                 |
| `galileo-otlp`     | OTLP gRPC + HTTP receivers, proto → domain conversion, batching writer         |
| `galileo-query`    | query DSL → safe ClickHouse SQL, trace assembly, BubbleUp                      |
| `galileo-gateway`  | LLM gateway: Anthropic + OpenAI-compatible adapters, routes, budgets, prompts  |
| `galileo-alerts`   | trigger and SLO evaluation, notifications                                      |
| `galileo-api`      | HTTP API for the UI, auth, metadata CRUD                                       |
| `galileo-server`   | the single binary wiring everything together                                   |
| `web/`             | Next.js frontend                                                               |

Data model: **organization → project → API keys**. Every event row carries a `project_id`.

## Running locally

```bash
cp deploy/.env.example deploy/.env      # set GALILEO_SECRET_KEY (openssl rand -hex 32)
docker compose -f deploy/docker-compose.yml up -d --build
```

That starts ClickHouse (:8123/:9000), Postgres (:5433), the Galileo server (API + gateway :8080,
OTLP :4317/:4318) and the UI (:3000). For local development of the Rust or Next.js code, stop the
two app containers and run them from source:

```bash
docker compose -f deploy/docker-compose.yml stop server web
cargo run -p galileo-server                             # API :8080, OTLP :4317/:4318
cd web && pnpm install && pnpm dev                      # UI :3000
```

Inside Docker, a provider running on the Mac (e.g. Ollama) is reachable as `http://host.docker.internal:11434`.

Configuration lives in `galileo.toml`; any key can be overridden with
`GALILEO_<SECTION>__<KEY>` environment variables (see `.env.example`).

## Docs

* [docs/connecting.md](docs/connecting.md) — pointing an OpenTelemetry SDK at Galileo, identity attributes, redaction
* [docs/gateway.md](docs/gateway.md) — LLM gateway: providers, routes, fallbacks, budgets, prompt registry, trace joining
* [docs/sdk-python.md](docs/sdk-python.md) — `galileo-django`: code-level traces, query call sites, identity on logs
* [docs/issues.md](docs/issues.md) — error tracking: exception grouping, lifecycle, deploy markers
* [docs/organization.md](docs/organization.md) — roles, invites, all-projects overview, personal tokens, retention, audit
* [docs/notifications.md](docs/notifications.md) — channels (Slack, Discord, Telegram, e-mail), warn/critical, baselines, digest
* [docs/query-dsl.md](docs/query-dsl.md) — the query language behind every chart, board, trigger and SLO
* [examples/demo-app](examples/demo-app) — an instrumented axum app calling the gateway; `examples/seed_otlp.py` seeds demo data

## Connecting an app

Point any OpenTelemetry SDK at `http://localhost:4318` (HTTP) or `http://localhost:4317` (gRPC)
with the header `Authorization: Bearer <project api key>`. For LLM calls, replace the provider
base URL with `http://localhost:8080/gw` and use the same API key. See `docs/`.

- [Browser telemetry (RUM)](docs/rum.md)
- [Operations: sampling, tiering, backup, health](docs/operations.md)
- [Ask Galileo assistant](docs/assistant.md)
- [Boards V2 and collaboration](docs/boards.md)
- [Logs: pipelines, log metrics, usage and quotas](docs/logs.md)

## Integrating your app

See [docs/integrating.md](docs/integrating.md) for the step-by-step guide (keys, endpoints, SDKs per stack, gateway, deploy markers) and [docs/melea.md](docs/melea.md) for the melea setup and for hosting Galileo on two domains (UI at https://galileo.serious-sam.dev, API at https://galileo-api.serious-sam.dev).
