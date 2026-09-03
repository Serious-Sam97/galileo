-- Org-level settings; first use is the assistant (which gateway route answers, where calls are recorded).
CREATE TABLE IF NOT EXISTS org_settings (
    org_id      UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    assistant   JSONB NOT NULL DEFAULT '{"enabled": false, "project_id": null, "route_alias": "", "max_steps": 6}',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
