-- Share links: a saved query with a frozen absolute time range, addressable by slug.
CREATE TABLE query_shares (
    id           UUID PRIMARY KEY,
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    slug         TEXT NOT NULL,
    title        TEXT NOT NULL DEFAULT '',
    kind         TEXT NOT NULL DEFAULT 'query',   -- query | traces | logs
    query        JSONB NOT NULL,
    frozen_start TIMESTAMPTZ,
    frozen_end   TIMESTAMPTZ,
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, slug)
);
