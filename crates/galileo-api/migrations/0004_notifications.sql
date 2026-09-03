-- Notification channels, trigger upgrades, daily digest.

CREATE TABLE notification_channels (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('webhook', 'slack', 'discord', 'telegram', 'email')),
    config      JSONB NOT NULL DEFAULT '{}',   -- {url} | {bot_token, chat_id} | {to: [emails]}
    enabled     BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

ALTER TABLE triggers
    ADD COLUMN warn_threshold   DOUBLE PRECISION,
    ADD COLUMN for_secs         INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN mute_until       TIMESTAMPTZ,
    ADD COLUMN per_group        BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN mode             TEXT NOT NULL DEFAULT 'threshold' CHECK (mode IN ('threshold', 'baseline')),
    ADD COLUMN baseline_factor  DOUBLE PRECISION NOT NULL DEFAULT 2.0,
    ADD COLUMN baseline_min_delta DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD COLUMN breaching_since  TIMESTAMPTZ,
    ADD COLUMN severity         TEXT NOT NULL DEFAULT 'ok' CHECK (severity IN ('ok', 'warn', 'critical'));
ALTER TABLE triggers DROP CONSTRAINT IF EXISTS triggers_state_check;
ALTER TABLE triggers ADD CONSTRAINT triggers_state_check CHECK (state IN ('ok', 'triggered', 'error', 'muted'));

CREATE TABLE trigger_group_states (
    trigger_id       UUID NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    group_key        TEXT NOT NULL,
    severity         TEXT NOT NULL DEFAULT 'ok',
    last_value       DOUBLE PRECISION,
    breaching_since  TIMESTAMPTZ,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (trigger_id, group_key)
);

ALTER TABLE project_settings
    ADD COLUMN digest_channels  JSONB NOT NULL DEFAULT '[]',
    ADD COLUMN digest_hour_utc  INTEGER NOT NULL DEFAULT 8 CHECK (digest_hour_utc BETWEEN 0 AND 23),
    ADD COLUMN digest_last_sent DATE;
