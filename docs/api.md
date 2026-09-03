# API

Every UI action is an HTTP call to `/api/…` you can make yourself with a session cookie or a
personal token (`Authorization: Bearer glt_…`, Settings → Personal tokens). OTLP ingest lives on
`:4317` (gRPC) and `:4318` (HTTP) with a project API key; the LLM gateway is under `/gw/`.

- **OpenAPI**: `GET /api/openapi.json` (the spec, generated from `docs/openapi.yaml`) and
  `GET /api/docs` (interactive reference).
- **CLI**: `cli/` builds the `galileo` binary — `galileo login --url … --token glt_… --project …`,
  `galileo query "spans | p95(duration_ms), count() by http_route"`, `galileo tail --filter "severity = ERROR"`,
  `galileo deploy 1.4.4 --service melea-api`, `galileo export > galileo.yaml`,
  `galileo import galileo.yaml --dry-run`, `galileo trigger test "request volume"`, `galileo whoami`.
- **Config as code**: `GET /api/projects/{id}/export?format=yaml|json` returns triggers, SLOs,
  boards, gateway providers/routes/prompts (secrets redacted), redaction rules, the log pipeline,
  log metrics, channels (secrets redacted) and settings; `POST /api/projects/{id}/import[?dry_run=true]`
  applies a bundle matched by name/alias — `<redacted>` keeps the current secret, dry-run returns
  the list of creates/updates.
- **Roles**: org roles (owner/admin/member/viewer) plus per-project roles (viewer/editor/admin,
  Settings → Project → Members). Reads need viewer, writes editor, settings/deletes admin.
- **SSO**: `[auth.oidc]` in `galileo.toml` enables "Continue with SSO" (see organization.md);
  password accounts can add TOTP two-factor (Settings → Personal).
