# Galileo V2 plan

V1 (2026-09-02) delivered the spine: OTLP ingest, ClickHouse storage, the query DSL with
BubbleUp, the LLM gateway, triggers/SLOs/boards, the UI, Docker packaging, and the first
connected app (melea-api). V2 is about depth: knowing *which code* did what, who it happened
to, and letting the tool work for more than one person and one app.

Ordering is by value to the melea integration first, then breadth. Each phase ships on its own.

---

## Phase 1 — Code-level traces ("which function ran this query?")

**Goal:** every span answers *where in the code* it came from, and DB spans say *which function
triggered the query*. Traces read like a call tree, not an HTTP-and-SQL list.

### 1a. `galileo-django` SDK (Python package, in this repo under `sdk/python/`)
Move what melea-api's `config/telemetry.py` does today into a reusable package, and add:

* **Call-site attribution on DB spans.** A span processor that, when a `psycopg2`/`django.db`
  span starts, walks the Python stack, skips frames in `site-packages`/Django/DRF, and stamps
  the first application frame as `code.function.name`, `code.namespace` (module + class),
  `code.file.path`, `code.line.number`. Same for outbound HTTP (`requests`) and gateway spans.
* **Function-level spans.** Three ways, all opt-in:
  - `@galileo.traced` decorator for services/helpers (records args names, not values, by default;
    `capture_args=[...]` allow-list for safe ones such as ids);
  - automatic spans for DRF view methods (`ViewSet.list/retrieve/create…`, `APIView.get/post`)
    with `code.*` + `http.route`, so the view is a real node in the tree;
  - `GALILEO_TRACE_MODULES=clinica.services,faturamento.*` to auto-wrap public functions of
    listed modules at import time (`sys.setprofile` is rejected: too slow).
* **Richer request span.** Request body size and content type, response size, selected
  request headers (allow-list, redacted), query string keys, `request.id`, Django `view_name`,
  DRF serializer/permission classes that ran, template name for HTML views.
* **SQL detail.** Full statement (already), plus row count, parameter *count*, and with
  `GALILEO_SQL_PARAMS=1` the parameters after redaction. `db.operation` and table name parsed
  from the SQL for grouping.
* **Exceptions everywhere.** Already on request spans; add for `@traced` functions and for
  Celery/RQ/management commands when `GALILEO_FORCE=1`.

### 1b. Galileo server/UI
* **Span details:** a "Code" section (`module.Class.function` · `file:line`) with the call
  site of DB/HTTP spans shown as "called from `faturamento.services.fechar_fatura` (line 88)".
* **Query fields:** `code.function`, `code.namespace`, `db.operation`, `db.table` as hot columns
  so "P95 by function" and "queries per table" are one click.
* **N+1 detector:** per trace, group DB spans by normalized SQL + call site; ≥ 10 repeats gets
  a badge on the trace and a `db.n_plus_one` attribute on the root span, so it is queryable and
  triggerable.
* **Trace view:** collapse repeated siblings ("SELECT pacientes ×48"), SQL text in the row on
  hover, a "callers" tab on a DB span listing the top functions issuing that statement across
  the time range.

**Effort:** SDK ~4 days, server/UI ~4 days. **Depends on:** nothing.

---

## Phase 2 — User context on logs (and everywhere else)

**Goal:** every log line says who and where; logs filter and group by user and clinic.

* **SDK logging filter:** a `logging.Filter` that injects `user.id`, `user.email` (hashed by
  default, plain with an opt-in), `tenant.id`, `tenant.domain`, `request.id`, `http.route`,
  `code.function` into every record emitted during a request, in any logger, not only
  `melea.request`. Outside a request (workers) it injects `job.name` and `job.id`.
* **Structured `extra`:** encourage `log.info("…", extra={...})` — everything in `extra` becomes
  a log attribute automatically (today only a fixed set).
* **Server:** `user.id`/`tenant.id` are already hot columns on logs; add `request.id` and
  `code.function`, and index `user.id` on logs for "everything this user did" in one query.
* **UI:** user and clinic columns on the logs page, a "user timeline" view (logs + requests +
  LLM calls for one `user.id`, chronological), and log ↔ trace jump both ways (already one way).

**Effort:** ~3 days. **Depends on:** Phase 1a for `code.function` (optional).

---

## Phase 3 — Error tracking ("issues")

**Goal:** the Sentry job, inside Galileo, using the exceptions already on spans and logs.

* Group exceptions by fingerprint (exception type + top application frame + route) into
  **issues** with first/last seen, count, affected users/tenants, and a sample trace.
* Issue list and detail pages; resolve/ignore; regressions reopen automatically.
* Trigger type "new issue" and "issue regression" with the same notification channels.
* Release/version awareness: `service.version` on the issue timeline, and deployment markers on
  charts (`POST /api/projects/{id}/deploys` from CI).

**Effort:** ~5 days. **Depends on:** Phase 1 (stack trace + code frames).

---

## Phase 4 — Organization and people

* Org-level **overview across projects** (all apps on one page, per-project cards).
* **Invites and roles** (owner/admin/member/viewer already in the schema): invite by e-mail,
  accept flow, per-project visibility for viewers.
* **Per-project retention** and sampling settings (schema TTL is global today).
* **Programmatic API tokens** (personal, scoped) for CI/scripts, separate from ingest keys.
* **Audit log** of configuration changes (routes, keys, triggers).

**Effort:** ~5 days. **Depends on:** nothing.

---

## Phase 5 — Notifications and alert quality

* Channels: **e-mail** (SMTP), Slack app (proper, not only webhooks), Discord/Telegram webhooks,
  PagerDuty-style escalation later.
* Trigger improvements: multiple thresholds (warn/critical), "for at least N minutes",
  anomaly baseline ("3σ above the same hour last week"), muting windows, per-group alert state
  (today the worst group decides).
* Alert on **logs** (count of matching log lines) and on **issues** (Phase 3).
* Digest: daily summary e-mail per project (requests, errors, p95, LLM spend, budget burn).

**Effort:** ~4 days.

---

## Phase 6 — Gateway V2

* **Tools and images across formats:** translate `tools`/`tool_choice`/`tool_result` and image
  blocks between Anthropic and OpenAI shapes (today text-only when formats differ).
* **Semantic cache** (optional per route): identical prompts within a TTL answered from cache.
* **Prompt experiments:** route a percentage of traffic to prompt version B, compare
  latency/cost/finish reasons in the AI tab.
* **Evals:** store user feedback (`POST /gw/v1/feedback` with a span id) and simple LLM-as-judge
  scoring jobs over recorded completions; show quality next to cost.
* **Embeddings and audio passthrough** so Whisper calls (melea) also go through the gateway
  and get recorded.
* Budget alerts before exhaustion (80%) via Phase 5 channels.

**Effort:** ~7 days.

---

## Phase 7 — Frontend telemetry (RUM)

* A small browser script (or OTel web SDK config) reporting page loads, route changes, fetch
  timings, JS errors, and Web Vitals, with `traceparent` propagation so a slow page links to
  the backend request that made it slow.
* Session view: one user's page timeline with the backend traces underneath.
* Out of scope: session replay.

**Effort:** ~5 days. **Depends on:** Phase 2 (user context).

---

## Phase 8 — Query and UI polish

* Time comparison ("vs. yesterday / last week") as a ghost line.
* Derived columns (`duration_ms / gen_ai.usage.output_tokens`) and `HAVING`.
* Drag-and-drop board layout, board-level time range and variables (`$tenant`).
* Service map from span parent/child across services.
* Keyboard-first query editor with a text form of the DSL (`spans | where route = "/x" | p95(duration_ms) by tenant`).
* Export (CSV) and share links with frozen time ranges.

**Effort:** ~6 days.

---

## Phase 9 — Operations

* Tail-based sampling at ingest (keep every error/slow trace, sample the rest) with per-project rates.
* Exponential-histogram bucket fidelity for metrics.
* ClickHouse tiering to object storage for cold data; backup/restore commands.
* Health page for Galileo itself (ingest lag, writer queue, ClickHouse parts, gateway latency).
* SDKs for Node and PHP mirroring `galileo-django` (call-site attribution, identity, logging filter).

**Effort:** ~6 days.

---

## Suggested order

1 → 2 → 3 (the debugging story, all driven by melea), then 4 → 5 (multi-app, multi-person),
then 6 (gateway depth), then 7 → 8 → 9. Each phase gets its own autopilot plan file.
