-- Monitoring V2: detection modes, composites, maintenance windows, group mutes, incidents, on-call.
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS sensitivity INTEGER NOT NULL DEFAULT 3;           -- anomaly/outlier: 1 (loose) … 5 (tight)
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS min_value DOUBLE PRECISION NOT NULL DEFAULT 0;  -- anomaly: ignore deviations below this absolute value
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS composite JSONB;                                 -- {all_of|any_of: [trigger ids], within_secs}
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS mutes JSONB NOT NULL DEFAULT '[]';               -- [{group_key, until}]
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS last_baseline DOUBLE PRECISION;
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS last_band DOUBLE PRECISION;

CREATE TABLE maintenance_windows (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    starts_at   TIMESTAMPTZ NOT NULL,
    ends_at     TIMESTAMPTZ NOT NULL,
    trigger_ids UUID[] NOT NULL DEFAULT '{}',     -- empty = every trigger of the project
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX maintenance_windows_project ON maintenance_windows (project_id, ends_at);

CREATE TABLE trigger_incidents (
    id              UUID PRIMARY KEY,
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    trigger_id      UUID NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    group_key       TEXT NOT NULL DEFAULT '',
    severity        TEXT NOT NULL DEFAULT 'critical',
    fired_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    acknowledged_at TIMESTAMPTZ,
    acknowledged_by TEXT NOT NULL DEFAULT '',
    resolved_at     TIMESTAMPTZ,
    peak_value      DOUBLE PRECISION,
    last_value      DOUBLE PRECISION,
    notified        INTEGER NOT NULL DEFAULT 0,
    escalated       INTEGER NOT NULL DEFAULT 0,
    ack_token       TEXT NOT NULL,
    notes           TEXT NOT NULL DEFAULT ''
);
CREATE INDEX trigger_incidents_open ON trigger_incidents (trigger_id, group_key) WHERE resolved_at IS NULL;
CREATE INDEX trigger_incidents_project ON trigger_incidents (project_id, fired_at DESC);

CREATE TABLE oncall_schedules (
    id            UUID PRIMARY KEY,
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    members       UUID[] NOT NULL DEFAULT '{}',   -- user ids in rotation order
    rotation_days INTEGER NOT NULL DEFAULT 7,
    starts_on     DATE NOT NULL,
    escalation    JSONB NOT NULL DEFAULT '[]',    -- [{after_secs, channel_id}]
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
