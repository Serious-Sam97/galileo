-- Organization features: invites, personal API tokens, audit log, per-project retention.

CREATE TABLE org_invites (
    id           UUID PRIMARY KEY,
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email        TEXT NOT NULL,
    role         TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('admin', 'member', 'viewer')),
    token_hash   TEXT NOT NULL UNIQUE,
    invited_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,
    accepted_at  TIMESTAMPTZ,
    accepted_by  UUID REFERENCES users(id) ON DELETE SET NULL
);
CREATE INDEX org_invites_org_idx ON org_invites(org_id, created_at DESC);

CREATE TABLE api_tokens (
    id            UUID PRIMARY KEY,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    token_hash    TEXT NOT NULL UNIQUE,
    token_prefix  TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ,
    last_used_at  TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);
CREATE INDEX api_tokens_user_idx ON api_tokens(user_id);

CREATE TABLE audit_log (
    id           UUID PRIMARY KEY,
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id   UUID REFERENCES projects(id) ON DELETE SET NULL,
    user_id      UUID REFERENCES users(id) ON DELETE SET NULL,
    user_email   TEXT NOT NULL DEFAULT '',
    action       TEXT NOT NULL,             -- e.g. "route.update"
    target_type  TEXT NOT NULL DEFAULT '',
    target_id    TEXT NOT NULL DEFAULT '',
    details      JSONB NOT NULL DEFAULT '{}',
    at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_org_idx ON audit_log(org_id, at DESC);
CREATE INDEX audit_log_project_idx ON audit_log(project_id, at DESC);

ALTER TABLE project_settings
    ADD COLUMN retention_spans_days   INTEGER CHECK (retention_spans_days IS NULL OR retention_spans_days BETWEEN 1 AND 3650),
    ADD COLUMN retention_logs_days    INTEGER CHECK (retention_logs_days IS NULL OR retention_logs_days BETWEEN 1 AND 3650),
    ADD COLUMN retention_metrics_days INTEGER CHECK (retention_metrics_days IS NULL OR retention_metrics_days BETWEEN 1 AND 3650),
    ADD COLUMN retention_last_run     TIMESTAMPTZ;
