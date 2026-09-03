# Organization, people, retention, audit

## Hierarchy and roles

organization → projects → API keys. Roles are per organization:

| role | can |
|---|---|
| owner | everything, including member roles; an org always keeps at least one owner |
| admin | create projects, invite/remove members, all configuration |
| member | configure (keys, routes, triggers, boards…) and query |
| viewer | read everything, change nothing (write endpoints return 403) |

## Invites

Settings → Organization → *Invite someone* produces a link valid for 7 days
(`/invite/<token>`). Opening it shows the organization and role; a new e-mail creates an account
with the chosen password, an existing account joins after entering its password. Links are
single-use. E-mail delivery of invites arrives with Phase 5 (notifications); until then, copy
the link.

## All-projects overview

`/org/<org id>` (the "All projects" link above the project switcher) shows a card per project:
requests, error rate, p95, LLM calls and cost, users, services, open issues, last seen. One
ClickHouse query grouped by project.

## Personal API tokens

Settings → Personal tokens. Tokens look like `glt_…`, are shown once, may expire, and act as
the user who created them: `Authorization: Bearer glt_…` on any `/api` call. They are for
scripts and CI (deploy markers, saved-query exports), separate from project ingest keys.

## Per-project retention

Settings → Project → Retention. Days for spans / logs / metrics; blank uses the server default
from `galileo.toml`. Shorter values are enforced hourly by deleting older rows for that project
(ClickHouse lightweight DELETE); the table TTL still bounds everything globally.

## Audit log

Every configuration change is recorded with who, when, what and the new values: API keys,
redaction rules, providers, routes, triggers, SLOs, issue status changes, members, invites,
retention. Settings → Audit shows the project's log; `GET /api/orgs/{id}/audit` the whole
organization (owners and admins).

## API

* `GET /orgs/{id}/overview?last_seconds=`
* `GET /orgs/{id}/members` · `PATCH /orgs/{id}/members/{user_id} {role}` · `DELETE …`
* `GET|POST /orgs/{id}/invites` · `DELETE /orgs/{id}/invites/{invite_id}`
* `GET /auth/invite/{token}` · `POST /auth/invite/{token}/accept {name?, password}`
* `GET|POST /auth/tokens` · `DELETE /auth/tokens/{token_id}`
* `GET /orgs/{id}/audit` · `GET /projects/{id}/audit`
* `GET|PUT /projects/{id}/settings {retention_spans_days, retention_logs_days, retention_metrics_days}`

## Roles per project

Org roles stay the coarse control: **owner** and **admin** are admins on every project, a
**member** is an *editor* and a **viewer** is a *viewer* by default. Settings → Project →
Members lets a project admin override that per person (viewer / editor / admin). Viewer reads
everything; editor creates and edits triggers, SLOs, boards, routes, prompts, pipelines; admin
also changes quotas, retention, members and deletes. The API returns 403 for anything above
the effective role.

## Single sign-on (OIDC)

Any OpenID Connect provider works (Google, Keycloak, Authentik, Okta, Azure AD, GitHub via an
OIDC bridge). Configure it with environment variables in `deploy/.env`:

```
GALILEO_OIDC_ISSUER=https://accounts.google.com
GALILEO_OIDC_CLIENT_ID=…
GALILEO_OIDC_CLIENT_SECRET=…
GALILEO_OIDC_REDIRECT_URL=https://galileo.example.com/api/auth/oidc/callback
GALILEO_OIDC_DEFAULT_ORG=<org uuid>     # new users auto-join this org as members
GALILEO_OIDC_LABEL=Continue with Google
```

or `[auth.oidc]` in `galileo.toml` (`issuer`, `client_id`, `client_secret`, `redirect_url`,
`allowed_domains = ["example.com"]`, `default_org`, `default_role`, `label`). The login page
shows the button when SSO is configured. The flow is Authorization Code with PKCE, state and
nonce in a signed cookie, ID tokens verified against the provider's JWKS (RS256) or the client
secret (HS256). Users are matched by `(issuer, subject)` first and then by e-mail, so existing
password accounts can start signing in with SSO.

## Two-factor (TOTP)

Password accounts can enable TOTP in Settings → Personal tokens → Two-factor authentication:
scan or paste the secret in Google Authenticator / 1Password / Aegis, confirm a code, and every
login afterwards asks for a code. Disabling needs a current code. SSO logins are governed by
the identity provider and skip Galileo's 2FA.
