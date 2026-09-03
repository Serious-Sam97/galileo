# Galileo V3 — Phase 7 (platform)

Rules: tick only when it compiles/tests pass. Never git commit/push. Single-server deployment stays the target: roles, SSO, config-as-code, CLI and OpenAPI; scale-out is documented, not built.

## Roles per project
- [x] `project_members` (Postgres 0013): `(project_id, user_id, role viewer|editor|admin)`; org owners/admins are implicit admins everywhere; ProjectAccess resolves the effective role (project role, else org role mapped: member→editor, viewer→viewer); `require_write` = editor+, destructive/settings endpoints require admin; API `GET/PUT/DELETE /projects/{id}/members`; Settings → Project → Members card; audit entries
- [x] tests for the role resolution table

## SSO (OIDC)
- [x] `[auth.oidc]` config (issuer, client_id, client_secret, redirect_url, allowed_domains, default_role) + `GET /api/auth/oidc/start` → provider authorize URL (PKCE + state cookie) and `GET /api/auth/oidc/callback` exchanging the code, verifying the ID token (issuer discovery, JWKS, nonce), creating the user on first login (auto-join the configured org with `default_role`), then the normal session cookie; login page shows "Continue with SSO" when configured; docs/organization.md (Google/GitHub examples)
- [x] optional 2FA (TOTP) for password logins: `POST /auth/2fa/setup` (secret + otpauth URL), `/auth/2fa/enable` (verify code), login returns `needs_2fa` and `POST /auth/2fa/verify`; Settings → Personal → Two-factor card

## Config as code + CLI
- [x] `GET /projects/{id}/export` → YAML/JSON bundle (triggers, SLOs, boards, gateway routes/providers (secrets redacted), prompts, redaction rules, log pipeline, log metrics, channels (secrets redacted), quotas, sampling); `POST /projects/{id}/import` applies a bundle idempotently (match by name/alias), dry-run mode returns the diff
- [x] `cli/` — `galileo` binary (Rust, in the workspace): `galileo login`, `galileo query "spans | …" --project`, `galileo tail logs --filter`, `galileo deploy <version> --service`, `galileo export > galileo.yaml`, `galileo import galileo.yaml [--dry-run]`, `galileo trigger test <name>`; config in `~/.galileo/config.toml` (api url + personal token)
- [x] OpenAPI 3 document generated for the API (utoipa or a hand-maintained `docs/openapi.yaml` served at `/api/openapi.json`) + `/api/docs` (Scalar/Redoc page); docs/api.md

## Scale-out (documented)
- [x] docs/operations.md: running several servers behind a load balancer (sticky routing by trace id for tail sampling or `sampling.rate = 1` on replicas), ClickHouse replication/sharding notes, Postgres HA, Helm-shaped compose override example; no code changes

## Validation + docs
- [x] melea: ana as project viewer cannot edit a trigger (403) but can read; promote to editor → can; OIDC flow against a local mock provider (a tiny OIDC server in the scratchpad) logs a new user in and auto-joins the org; 2FA setup + login; export → import dry-run shows no diff, edited YAML applies; CLI query and tail work; /api/docs renders; memory; plan archived
