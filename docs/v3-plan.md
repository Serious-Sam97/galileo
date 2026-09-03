# Galileo V3 plan

V2 (2026-09-03) made Galileo deep: code-level traces, identity everywhere, issues, org and
people, notifications, gateway V2, browser telemetry, query polish, and operations (sampling,
histograms, tiering, backups, health, Node/PHP SDKs). V3 is about three things:

1. **Galileo that thinks** — an assistant inside the tool that turns "why is checkout slow for
   tenant X" into a query, a root-cause write-up and a fix suggestion, dogfooding the gateway.
2. **AI apps as first-class citizens** — agent runs, guardrails, datasets and prompt CI, so the
   gateway is a control plane and not only a proxy.
3. **Your other apps** — SDKs for the stacks you actually run (generic Python, Rust, Android),
   plus the platform work (roles, SSO, config-as-code, scale-out) that a second team or a second
   server needs.

Ordering favours what changes day-to-day debugging first, then what makes Galileo usable by
more apps and people. Each phase ships on its own and gets its own autopilot plan file.

---

## Phase 1 — Ask Galileo (assistant)

* A chat panel on every page, backed by a gateway route (`galileo-assistant`, any provider you
  configure — Ollama works). Grounded with the field catalog, the current project's services,
  routes and top attributes, and the page context (the open trace, issue, query or session).
* Natural language → query DSL: "p95 by tenant for /api/consultas last 24h vs yesterday" builds
  the query, runs it and explains the result; the text DSL from V2 is the intermediate form, so
  every answer is reproducible and editable.
* **Investigate** on an issue or a fired trigger: BubbleUp against the baseline, the top
  differing attributes, sample traces, the N+1 detector, recent deploys — assembled into a
  root-cause write-up with links, stored on the issue as a note. **Explain this trace**
  summarises a waterfall in plain language (where time went, what repeated, what failed).
* Every assistant call is itself a gateway call in the Default project, so cost and quality are
  visible, and feedback (👍/👎) on answers feeds the evals from V2.
* Guardrails: read-only, project-scoped, never sees raw prompts/completions unless the route
  records content; answers cite the query they ran.

**Effort:** ~6 days. **Depends on:** gateway V2, text DSL.

---

## Phase 2 — Agent observability and control plane V3

* **Agent runs**: a conversation/run view that groups gateway spans by `gen_ai.conversation.id`
  and turn, showing the tool-call loop (model → tool → model), sub-agents, tokens and cost per
  turn, and where the run stalled. SDK helpers (`galileo.agent.run()` / `step()`) for apps that
  orchestrate their own loops; MCP tool calls appear as tool spans.
* **Guardrails on the gateway**: PII detection/redaction on prompts and completions (reusing
  the redaction engine), prompt-injection heuristics, allow/deny lists, max tokens/cost per
  user per day; violations are recorded as error spans with `gen_ai.galileo.guardrail` and can
  block, warn or just tag.
* **Semantic cache**: embeddings-based near-duplicate matching per route (threshold and TTL),
  on top of V2's exact-match cache.
* **Datasets and prompt CI**: golden datasets (inputs + expected/rubric) built from real calls
  or uploaded; creating a prompt version runs the judge over the dataset and compares with the
  current version; a route can require a passing score before a version is promotable.
* **Smart routing**: targets ordered by live health (p95, error rate, budget headroom) instead
  of a fixed list; cost anomaly triggers (spend rate vs baseline) with the V2 notifier.

**Effort:** ~8 days. **Depends on:** Phase 1 for the "explain this run" button (optional).

---

## Phase 3 — Boards V2 and collaboration

* Drag-and-drop grid layout, resizable panels, panel types: series, table, stat, heatmap,
  service map, markdown, issues list, SLO burn.
* Board variables (`$tenant`, `$service`, `$route`) with pickers fed by field values; board-level
  time range and comparison; templated boards (RED per service, LLM cost, browser vitals)
  created in one click for a project.
* Annotations: comments pinned to a time on a chart or to a query result, with @mentions that
  notify through channels; query history per user with "re-run" and "save".
* Board and query share links render a static PNG for Slack/Discord/e-mail (headless render in
  the web image).

**Effort:** ~6 days.

---

## Phase 4 — Logs and metrics maturity

* **Log pipelines**: per-project processors (JSON parse, regex/grok extract, key rename, drop,
  severity remap) applied at ingest, with a live preview on recent logs; log-based metrics
  (count/value of a pattern) as first-class metric series.
* **Span-based metrics**: pre-aggregated RED rollups (per service/route/tenant/status, 1-minute)
  kept for long retention, used automatically by Query when the range is wider than the raw
  retention; long-term boards stay cheap.
* **Cardinality and usage**: attribute cardinality report, per-project ingest volume, retention
  cost estimate, soft quotas with warnings and hard quotas with 429s; usage on the org overview.

**Effort:** ~6 days.

---

## Phase 5 — Monitoring V2

* Anomaly detection (seasonal baseline: same hour last 7 days, robust deviation) and outlier
  detection across groups ("which tenant behaves unlike the others") as trigger modes.
* Composite triggers (A and B within 10 min), maintenance windows, mute by attribute, and an
  incident timeline per trigger (fired → acknowledged → resolved) with acknowledge from the UI,
  Slack and e-mail links.
* On-call: simple rotations and escalation (channel A, then channel B after N minutes) — enough
  to not need PagerDuty for a small team.

**Effort:** ~5 days.

---

## Phase 6 — SDKs for your stacks

* **Python (generic)**: `galileo-python` — the Django package's call-site, identity, logging
  and exception features for FastAPI/Starlette, Flask, SQLAlchemy, httpx/requests, Celery, so
  interview-dojo and any FastAPI app get the same depth as melea.
* **Rust**: `galileo-rs` — a `tracing` layer + OTLP exporter with call-site attribution from
  span metadata, sqlx query spans, axum/actix middleware, identity extension; used by Odeon
  and by Galileo itself (self-tracing).
* **Android/Kotlin**: `galileo-android` — screens as page views, OkHttp/Retrofit and Room
  spans with `traceparent`, crashes and ANRs as issues, cold-start and frame-time vitals,
  session view shared with browser RUM; for Momentum and Savepoint.
* Gateway SDK helpers in each language (route alias, prompt injection, feedback, agent steps).

**Effort:** ~8 days.

---

## Phase 7 — Platform and scale-out

* Roles per project (viewer/editor/admin) on top of org roles; OIDC SSO (Google/GitHub/any) and
  optional 2FA; SCIM later.
* Config-as-code: export/import triggers, SLOs, boards, routes, prompts, redaction rules as
  YAML; `galileo` CLI (query, tail logs, deploy marker, apply config); OpenAPI spec and
  generated docs for the API.
* Scale-out: several server replicas behind a load balancer with trace-id-consistent routing
  for tail sampling (or a shared sampler), ClickHouse replication/sharding notes and a Helm
  chart; per-key ingest rate limits.

**Effort:** ~7 days.

---

## Phase 8 — UX

* ⌘K global search (traces, issues, users, boards, routes, pages), keyboard navigation
  everywhere, a light theme, pt-BR translation, tablet/phone layouts for triage.
* Trace diff (two traces side by side, highlighting what differs), onboarding wizard for a new
  project (key → snippet → first trace → first board), and an in-app changelog.

**Effort:** ~5 days.

---

## Suggested order

1 → 2 (the assistant and the AI control plane, both dogfooding the gateway), then 6 (the
other apps), then 3 → 4 → 5 (boards, data maturity, monitoring), then 7 → 8. Phase 6 can move
up if connecting Momentum or Odeon matters more than agent features.

## Out of scope for V3

Session replay, continuous profiling, hosted multi-tenant SaaS billing, mobile crash
symbolication for iOS.
