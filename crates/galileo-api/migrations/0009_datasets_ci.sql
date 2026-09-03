-- Golden datasets and prompt CI.
CREATE TABLE gateway_datasets (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);
CREATE TABLE gateway_dataset_items (
    id          UUID PRIMARY KEY,
    dataset_id  UUID NOT NULL REFERENCES gateway_datasets(id) ON DELETE CASCADE,
    input       JSONB NOT NULL,                 -- {messages: [{role, content}]}
    expected    TEXT NOT NULL DEFAULT '',       -- reference answer (optional)
    rubric      TEXT NOT NULL DEFAULT '',       -- per-item grading hint (optional)
    source_span TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX gateway_dataset_items_ds ON gateway_dataset_items (dataset_id);

ALTER TABLE prompts ADD COLUMN IF NOT EXISTS ci JSONB NOT NULL DEFAULT '{}';          -- {dataset_id, judge_route, run_route, min_score, required}
ALTER TABLE prompts ADD COLUMN IF NOT EXISTS promoted_version INTEGER;
ALTER TABLE prompt_versions ADD COLUMN IF NOT EXISTS ci_status TEXT NOT NULL DEFAULT 'none' CHECK (ci_status IN ('none', 'running', 'passed', 'failed', 'error'));
ALTER TABLE prompt_versions ADD COLUMN IF NOT EXISTS ci_score DOUBLE PRECISION;
ALTER TABLE prompt_versions ADD COLUMN IF NOT EXISTS ci_run_id UUID;
ALTER TABLE gateway_eval_runs ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'sample';   -- sample | prompt_ci
ALTER TABLE gateway_eval_runs ADD COLUMN IF NOT EXISTS prompt_id UUID;
ALTER TABLE gateway_eval_runs ADD COLUMN IF NOT EXISTS prompt_version INTEGER;
ALTER TABLE gateway_eval_runs ADD COLUMN IF NOT EXISTS dataset_id UUID;

-- cost anomaly events share the budget_events table
ALTER TABLE budget_events DROP CONSTRAINT IF EXISTS budget_events_kind_check;
ALTER TABLE budget_events ADD CONSTRAINT budget_events_kind_check CHECK (kind IN ('warn80', 'exhausted', 'anomaly'));
