-- Galileo metadata schema. Event data lives in ClickHouse; everything humans configure lives here.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE organizations (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id            UUID PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE org_members (
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id)
);

CREATE TABLE sessions (
    token_hash  TEXT PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    user_agent  TEXT NOT NULL DEFAULT ''
);
CREATE INDEX sessions_user_idx ON sessions(user_id);

CREATE TABLE projects (
    id          UUID PRIMARY KEY,
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, slug)
);

CREATE TABLE api_keys (
    id            UUID PRIMARY KEY,
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    key_hash      TEXT NOT NULL UNIQUE,     -- sha256(raw key), hex
    key_prefix    TEXT NOT NULL,            -- first chars shown in the UI
    scopes        TEXT[] NOT NULL DEFAULT '{ingest,gateway}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at  TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);
CREATE INDEX api_keys_project_idx ON api_keys(project_id);

CREATE TABLE redaction_rules (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    rule        JSONB NOT NULL,            -- galileo_core::RedactionRule
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX redaction_rules_project_idx ON redaction_rules(project_id);

CREATE TABLE saved_queries (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    query       JSONB NOT NULL,            -- galileo_query::Query
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX saved_queries_project_idx ON saved_queries(project_id);

CREATE TABLE boards (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    panels      JSONB NOT NULL DEFAULT '[]', -- [{id, title, saved_query_id | query, viz, w, h, x, y}]
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX boards_project_idx ON boards(project_id);

CREATE TABLE triggers (
    id                UUID PRIMARY KEY,
    project_id        UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    query             JSONB NOT NULL,        -- galileo_query::Query (single calculation)
    op                TEXT NOT NULL CHECK (op IN ('>', '>=', '<', '<=', '=', '!=')),
    threshold         DOUBLE PRECISION NOT NULL,
    frequency_secs    INTEGER NOT NULL DEFAULT 60,
    window_secs       INTEGER NOT NULL DEFAULT 300,
    enabled           BOOLEAN NOT NULL DEFAULT true,
    recipients        JSONB NOT NULL DEFAULT '[]', -- [{type: webhook|slack, url}]
    state             TEXT NOT NULL DEFAULT 'ok' CHECK (state IN ('ok', 'triggered', 'error')),
    last_value        DOUBLE PRECISION,
    last_evaluated_at TIMESTAMPTZ,
    last_triggered_at TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX triggers_project_idx ON triggers(project_id);

CREATE TABLE trigger_events (
    id          UUID PRIMARY KEY,
    trigger_id  UUID NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    fired_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    state       TEXT NOT NULL,
    value       DOUBLE PRECISION,
    message     TEXT NOT NULL DEFAULT ''
);
CREATE INDEX trigger_events_trigger_idx ON trigger_events(trigger_id, fired_at DESC);

CREATE TABLE slos (
    id                UUID PRIMARY KEY,
    project_id        UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    dataset           TEXT NOT NULL DEFAULT 'spans',
    total_filters     JSONB NOT NULL DEFAULT '[]',  -- galileo_query::Filter[]
    good_filters      JSONB NOT NULL DEFAULT '[]',
    target_pct        DOUBLE PRECISION NOT NULL,     -- e.g. 99.9
    window_days       INTEGER NOT NULL DEFAULT 30,
    burn_alerts       JSONB NOT NULL DEFAULT '[]',  -- [{name, long_window_mins, short_window_mins, burn_rate, recipients}]
    state             TEXT NOT NULL DEFAULT 'ok',
    last_evaluated_at TIMESTAMPTZ,
    last_result       JSONB,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX slos_project_idx ON slos(project_id);

CREATE TABLE gateway_providers (
    id           UUID PRIMARY KEY,
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('anthropic', 'openai', 'ollama', 'openai_compatible')),
    base_url     TEXT NOT NULL,
    api_key_enc  BYTEA,                    -- AES-GCM(secret_key) of the provider key; NULL for local
    headers      JSONB NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

CREATE TABLE gateway_routes (
    id           UUID PRIMARY KEY,
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    alias        TEXT NOT NULL,              -- the model name apps ask for
    description  TEXT NOT NULL DEFAULT '',
    targets      JSONB NOT NULL,             -- [{provider_id, model, weight?}] in fallback order
    budget       JSONB NOT NULL DEFAULT '{}',-- {daily_usd, daily_tokens, monthly_usd}
    rate_limit   JSONB NOT NULL DEFAULT '{}',-- {requests_per_minute}
    enabled      BOOLEAN NOT NULL DEFAULT true,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, alias)
);

CREATE TABLE prompts (
    id           UUID PRIMARY KEY,
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

CREATE TABLE prompt_versions (
    id           UUID PRIMARY KEY,
    prompt_id    UUID NOT NULL REFERENCES prompts(id) ON DELETE CASCADE,
    version      INTEGER NOT NULL,
    content      JSONB NOT NULL,             -- {system?, messages: [{role, content}], variables: [...]}
    note         TEXT NOT NULL DEFAULT '',
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (prompt_id, version)
);
