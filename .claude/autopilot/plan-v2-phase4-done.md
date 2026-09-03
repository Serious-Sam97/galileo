# Galileo V2 — Phase 4 (organization and people)

Rules: tick only when it compiles/tests pass. Never git commit/push.

## Server
- [x] Postgres migration 0003: org_invites, api_tokens (personal, `glt_` prefix), audit_log, project_settings retention columns (spans/logs/metrics days, nullable = global)
- [x] org overview API: GET /orgs/{id}/overview?last_seconds → per-project requests, error rate, p95, LLM calls/cost, open issues, last seen (one ClickHouse query grouped by project_id)
- [x] members + invites API: GET /orgs/{id}/members, PATCH role, DELETE member (owners only, never the last owner); POST/GET/DELETE /orgs/{id}/invites (link-based; accept flow GET /auth/invite/{token} + POST /auth/invite/{token}/accept creating or attaching the user)
- [x] personal API tokens: POST/GET/DELETE /auth/tokens; CurrentUser extractor accepts `Authorization: Bearer glt_…` (hash lookup, last_used_at)
- [x] audit log: helper + calls in every mutating config handler (keys, rules, saved queries, boards, triggers, SLOs, providers, routes, prompts, issues status, members, invites, settings); GET /orgs/{id}/audit and /projects/{id}/audit
- [x] per-project retention: GET/PUT /projects/{id}/settings (retention days); hourly job in galileo-alerts deleting rows older than the project's retention (ClickHouse lightweight DELETE)
- [x] cargo test + clippy green; server image rebuilt

## UI
- [x] org overview page /org/[orgId]: project cards (requests, errors, p95, LLM cost, open issues) linking into each project; "All projects" link above the project switcher
- [x] Settings → Organization tab: members with role select, invites with copyable link; /invite/[token] accept page (works logged out)
- [x] Settings → Tokens tab (create/revoke personal tokens, shown once) and Audit tab (who changed what, when); Project tab gains retention fields
- [x] pnpm build green; web image rebuilt

## Validate
- [x] create an invite, accept it in a second browser session as a new user, see the project as a viewer; create a token and call the API with it; change retention and see the job run; audit shows all of it; docs/organization.md + memory
