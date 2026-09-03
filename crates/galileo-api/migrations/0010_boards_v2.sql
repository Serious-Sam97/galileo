-- Boards V2: variables, board time range/compare; annotations; query history.
ALTER TABLE boards ADD COLUMN IF NOT EXISTS variables  JSONB NOT NULL DEFAULT '[]';   -- [{name, field, default, label}]
ALTER TABLE boards ADD COLUMN IF NOT EXISTS time_range JSONB;                         -- galileo_query::TimeRange or null (= panel's own)
ALTER TABLE boards ADD COLUMN IF NOT EXISTS compare    BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE boards ADD COLUMN IF NOT EXISTS template   TEXT NOT NULL DEFAULT '';

CREATE TABLE annotations (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL DEFAULT 'chart' CHECK (kind IN ('chart', 'query', 'board')),
    board_id    UUID REFERENCES boards(id) ON DELETE CASCADE,
    panel_id    TEXT NOT NULL DEFAULT '',
    query_text  TEXT NOT NULL DEFAULT '',
    at          TIMESTAMPTZ,                       -- the point in time the note is pinned to
    text        TEXT NOT NULL,
    mentions    UUID[] NOT NULL DEFAULT '{}',
    author_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX annotations_project_at ON annotations (project_id, at);
CREATE INDEX annotations_board ON annotations (board_id);

CREATE TABLE query_history (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    query       JSONB NOT NULL,
    text        TEXT NOT NULL DEFAULT '',
    ran_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX query_history_user ON query_history (user_id, project_id, ran_at DESC);
