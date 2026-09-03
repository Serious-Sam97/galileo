# Galileo V2 — Phase 1 (code-level traces) + Phase 2 (user context on logs)

Rules: tick only when it compiles/tests pass. Never git commit/push. Galileo repo: ~/Developer/galileo.
Connected app for validation: ~/Developer/limiar-core/melea-api (Docker, api :8003, chaos on).

## SDK: sdk/python/galileo_django
- [x] package skeleton (pyproject, README), `setup()` moved from melea config/telemetry.py with the same env vars; venv + pytest harness with in-memory exporter
- [x] DB spans from Django's `execute_wrapper` (replaces psycopg2 instrumentation): db.system, db.statement, db.operation, db.table, db.row_count, db.params_count, optional redacted params; unit tests
- [x] call-site attribution: SpanProcessor stamping code.function.name / code.namespace / code.file.path / code.line.number on DB, HTTP-client and gateway spans (first non-framework frame); unit tests
- [x] view spans: middleware wrapping the view call with a span named after the view (module.Class.method) + code.*; DRF view/serializer/permission classes as attributes
- [x] `@traced` decorator (arg names, allow-listed values, exception recording) and `GALILEO_TRACE_MODULES` auto-wrapping; unit tests
- [x] request span enrichment: request.id, body size/content type, response size, query keys, allow-listed headers (redacted)
- [x] logging filter injecting user.id/tenant.id/tenant.domain/request.id/http.route/code.function into every record; `extra` → attributes; unit tests
- [x] exception signal handler + route cleaning + identity (ported from melea) with tests; GALILEO_FORCE/pytest guards kept

## Server
- [x] ClickHouse migration v2: spans hot columns code_function, code_namespace, code_file, code_line, db_operation, db_table, request_id; logs hot columns request_id, code_function; extraction in rows.rs; field catalog aliases
- [x] N+1 detection: `GET /projects/{id}/db/nplusone` (top repeated statement+function per trace over a range) and per-trace `repeated_queries` in TraceView
- [x] callers endpoint: `GET /projects/{id}/db/callers?statement=` → functions issuing that statement (count, p95) + `GET /projects/{id}/users/{user_id}/timeline` (requests + logs + LLM calls merged)
- [x] cargo test + clippy green

## UI
- [x] span details: "Code" section and "called from module.func (file:line)" for DB/HTTP spans; SQL on hover in waterfall; repeated siblings collapsed ("SELECT pacientes ×48") with expand
- [x] trace header: N+1 badge + "Repeated queries" panel; DB span drawer gets a "Callers" tab
- [x] Overview: "Repeated queries (N+1 candidates)" card from the endpoint; Query page field picker shows code.* / db.* fields
- [x] Logs page: user / clinic / function columns and filters; user id links to the user timeline page `/p/[pid]/users/[userId]`
- [x] pnpm build green

## Integrate melea-api
- [x] build wheel into melea-api/vendor/, requirements + Dockerfile use it, config/telemetry.py replaced by `galileo_django` setup + settings (GALILEO_TRACE_MODULES for services), chaos kept
- [x] verify in Galileo: DB spans carry code.function + db.table, view spans present, N+1 trace flagged (melea's ~60 queries/request), logs show user/tenant on every line, user timeline works
- [x] docs: docs/sdk-python.md + update connecting.md; memory updated
