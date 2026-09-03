-- Gateway V2: feedback, LLM-as-judge evals, budget warning events.

CREATE TABLE gateway_feedback (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    span_id     TEXT NOT NULL,
    trace_id    TEXT NOT NULL DEFAULT '',
    rating      SMALLINT NOT NULL CHECK (rating BETWEEN -1 AND 5),   -- -1 thumbs down, 1 thumbs up, or 1..5 stars
    comment     TEXT NOT NULL DEFAULT '',
    user_id     TEXT NOT NULL DEFAULT '',                             -- end-user id (from the app) or Galileo user email
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX gateway_feedback_project_idx ON gateway_feedback(project_id, created_at DESC);
CREATE INDEX gateway_feedback_span_idx ON gateway_feedback(project_id, span_id);

CREATE TABLE gateway_eval_runs (
    id            UUID PRIMARY KEY,
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    judge_route   TEXT NOT NULL,
    rubric        TEXT NOT NULL,
    sample_size   INTEGER NOT NULL,
    filter_route  TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'done', 'failed')),
    scored        INTEGER NOT NULL DEFAULT 0,
    avg_score     DOUBLE PRECISION,
    error         TEXT NOT NULL DEFAULT '',
    created_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at   TIMESTAMPTZ
);

CREATE TABLE gateway_evals (
    id          UUID PRIMARY KEY,
    run_id      UUID NOT NULL REFERENCES gateway_eval_runs(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    span_id     TEXT NOT NULL,
    trace_id    TEXT NOT NULL DEFAULT '',
    model       TEXT NOT NULL DEFAULT '',
    route       TEXT NOT NULL DEFAULT '',
    prompt_name TEXT NOT NULL DEFAULT '',
    prompt_version TEXT NOT NULL DEFAULT '',
    score       SMALLINT,
    reasoning   TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX gateway_evals_project_idx ON gateway_evals(project_id, created_at DESC);
CREATE INDEX gateway_evals_span_idx ON gateway_evals(project_id, span_id);

CREATE TABLE budget_events (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    route_alias TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('warn80', 'exhausted')),
    period      TEXT NOT NULL,                -- 'day:2026-09-03' or 'month:2026-09'
    spent_usd   DOUBLE PRECISION NOT NULL,
    cap_usd     DOUBLE PRECISION NOT NULL,
    notified    BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, route_alias, kind, period)
);
