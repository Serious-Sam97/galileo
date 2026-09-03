# Galileo V2 — Phase 3 (error tracking: issues)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea (chaos on).

## Ingest / storage
- [x] rows.rs: parse the exception event → exception_message, exception_culprit (deepest application frame `file:function` from the stacktrace, falling back to code.*), exception_fingerprint (hash of type + culprit + route); ClickHouse migration v3 adds the columns + index; unit tests for the traceback parser
- [x] field catalog: exception.message / exception.culprit / exception.fingerprint queryable; bubbleup hot pairs include culprit

## Issues (Postgres + evaluator)
- [x] migration: issues (project, fingerprint, title, exception_type, culprit, route, status open|resolved|ignored, first/last seen, count, users, last_trace_id, last_version, resolved_at, resolved_version, notes), issue_events (issue, kind new|regressed|resolved|reopened|ignored, at, message), deploys (project, version, at, note), project_settings (project, issue_recipients jsonb)
- [x] evaluator in galileo-alerts: every tick, roll up new exceptions per fingerprint from ClickHouse since the last run; create/update issues; resolved + new occurrence → regressed (reopen); notify recipients on new/regressed via existing notify module; idempotent + unit test for the state machine
- [x] API: GET /projects/{id}/issues (status filter, sort, window counts), GET /issues/{id} (series by hour, top routes/users/tenants/versions, latest stacktrace, sample traces), POST /issues/{id}/{resolve|ignore|reopen}, GET/PUT /projects/{id}/issue-settings, GET/POST /projects/{id}/deploys
- [x] cargo test + clippy green; server image rebuilt

## UI
- [x] Issues page (nav item with open count): tabs open/resolved/ignored, list with sparkline counts, culprit, last seen, users; detail page: header (status, first/last seen, count, users), stacktrace viewer, 24h chart, breakdown tables, sample traces, resolve/ignore/reopen buttons
- [x] deploy markers on Overview/Query charts (vertical lines from /deploys); Settings → Issues tab for notification recipients
- [x] pnpm build green; web image rebuilt

## Validate on melea
- [x] chaos errors appear as 3 issues (InvoiceSyncError, ReportTimeout, ZeroDivisionError) with correct culprits; resolve one, re-trigger it, see it regress; a deploy marker posted via API shows on charts; memory + docs (docs/issues.md) updated
