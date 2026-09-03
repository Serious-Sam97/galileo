-- Error tracking: issues are groups of exceptions sharing a fingerprint (type + culprit + route).
-- Counts and time series come from ClickHouse; this holds state, notes and history.

CREATE TABLE issues (
    id               UUID PRIMARY KEY,
    project_id       UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    fingerprint      TEXT NOT NULL,
    title            TEXT NOT NULL,             -- "ExceptionType: message"
    exception_type   TEXT NOT NULL,
    culprit          TEXT NOT NULL DEFAULT '',
    route            TEXT NOT NULL DEFAULT '',
    service_name     TEXT NOT NULL DEFAULT '',
    status           TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved', 'ignored')),
    first_seen       TIMESTAMPTZ NOT NULL,
    last_seen        TIMESTAMPTZ NOT NULL,
    count            BIGINT NOT NULL DEFAULT 0,
    users            BIGINT NOT NULL DEFAULT 0,
    last_trace_id    TEXT NOT NULL DEFAULT '',
    last_version     TEXT NOT NULL DEFAULT '',
    resolved_at      TIMESTAMPTZ,
    resolved_version TEXT NOT NULL DEFAULT '',
    resolved_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    notes            TEXT NOT NULL DEFAULT '',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, fingerprint)
);
CREATE INDEX issues_project_status_idx ON issues(project_id, status, last_seen DESC);

CREATE TABLE issue_events (
    id         UUID PRIMARY KEY,
    issue_id   UUID NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('new', 'regressed', 'resolved', 'reopened', 'ignored', 'note')),
    at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    message    TEXT NOT NULL DEFAULT '',
    user_id    UUID REFERENCES users(id) ON DELETE SET NULL
);
CREATE INDEX issue_events_issue_idx ON issue_events(issue_id, at DESC);

CREATE TABLE deploys (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service     TEXT NOT NULL DEFAULT '',
    version     TEXT NOT NULL,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    note        TEXT NOT NULL DEFAULT '',
    url         TEXT NOT NULL DEFAULT ''
);
CREATE INDEX deploys_project_idx ON deploys(project_id, at DESC);

CREATE TABLE project_settings (
    project_id        UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    issue_recipients  JSONB NOT NULL DEFAULT '[]',
    issues_last_run   TIMESTAMPTZ,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
