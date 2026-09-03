# Galileo V3 — recap (2026-09-03)

V3 delivered all eight phases of `docs/v3-plan.md`, validated against the melea project
(Django, real traffic) and the Default project (Galileo's own gateway calls), with local Ollama
models as the assistant, judge and experiment targets.

| Phase | What shipped |
| --- | --- |
| 1 · Ask Galileo | Assistant drawer (⌘J) with a JSON-action tool loop through the gateway; natural language → text DSL → results; Investigate (BubbleUp + N+1 + deploys → issue note); Explain this trace; org-level model settings; every call recorded with cost and 👍/👎 feedback. |
| 2 · Agent control plane | Agent runs by conversation id with the tool loop per turn; guardrails (PII redaction, prompt-injection heuristics, deny lists, per-user daily spend); semantic cache; golden datasets and prompt CI with a judge and promotion gates; smart routing by live health; spend anomaly alerts. |
| 6 · SDKs | Generic Python (FastAPI, Flask, SQLAlchemy, Celery, agent helpers), Rust (`tracing` layer, `sql!`, axum middleware, identity), Android (screens, OkHttp, crashes and ANRs, vitals), plus Connect snippets in Settings. |
| 3 · Boards V2 | Grid layout with drag/resize, eight panel types, board variables with pickers, board-level time range and compare, one-click templates (RED, LLM, browser), annotations with @mentions, per-user query history, PNG rendering and send-to-channel. |
| 4 · Logs and metrics | Log pipelines with live preview, log-based metrics, 1-minute RED and DB rollups used automatically for wide ranges, cardinality and usage reports, soft warnings and hard 429 quotas. |
| 5 · Monitoring V2 | Anomaly (seasonal baseline, MAD) and outlier trigger modes, composites, maintenance windows, group mutes, incident timeline with acknowledge links, on-call rotations and escalation. |
| 7 · Platform | Per-project roles, OIDC SSO (PKCE, JWKS/HS256), TOTP two-factor, config as code (export/import with dry run), the `galileo` CLI, OpenAPI at `/api/docs`, scale-out notes. |
| 8 · UX | ⌘K palette with cross-entity search, keyboard navigation, light theme, pt-BR, drawer layout for small screens, trace diff, onboarding wizard, in-app changelog. |

## Numbers

- Rust crates: 8 in the workspace plus the SDK crate and the CLI; Postgres migrations 0001–0013; ClickHouse schema V5.
- SDKs: Django, Python, Node, PHP, Rust, Android, browser.
- Docs: `docs/*.md` per area, `docs/openapi.yaml`, `docs/CHANGELOG.md`.

## Known limitations carried forward

- Composite `all_of` triggers ignore muted/windowed members; anomaly mode needs a week of history.
- Quotas are counted in memory since the last restart.
- The semantic cache uses a hashed-token embedding when no embedding route is configured.
- Express route names under ESM loaders are generic; PHP SDK needs Guzzle.
- Scale-out is documented, not built: one server process per deployment.
- Translation covers the chrome, not every string; the query builder stays in English.

## What V4 could be

Session replay, continuous profiling, SCIM, a Helm chart with replicated ClickHouse, iOS SDK,
and a hosted multi-tenant edition with billing — all explicitly out of scope for V3.
