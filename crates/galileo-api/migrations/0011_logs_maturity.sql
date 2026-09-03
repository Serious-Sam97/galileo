-- Log pipelines, log-based metrics, ingest quotas.
CREATE TABLE log_pipelines (
    project_id  UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    pipeline    JSONB NOT NULL DEFAULT '{"enabled": true, "processors": []}',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE log_metrics (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    rule        JSONB NOT NULL,   -- galileo_core::LogMetric
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);
ALTER TABLE project_settings ADD COLUMN IF NOT EXISTS quotas JSONB NOT NULL DEFAULT '{}';   -- {spans_per_day, logs_per_day, metrics_per_day, mode: warn|hard}
ALTER TABLE budget_events DROP CONSTRAINT IF EXISTS budget_events_kind_check;
ALTER TABLE budget_events ADD CONSTRAINT budget_events_kind_check CHECK (kind IN ('warn80', 'exhausted', 'anomaly', 'ingest_quota'));
