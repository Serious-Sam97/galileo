-- Accounts managed by the Master: temporary passwords, forced change, disabling, lockout, and
-- per-user permission overrides on top of the role presets.

ALTER TABLE users ADD COLUMN IF NOT EXISTS is_master                BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN IF NOT EXISTS must_change_password     BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN IF NOT EXISTS temp_password_expires_at TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_changed_at      TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN IF NOT EXISTS disabled_at              TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN IF NOT EXISTS last_login_at            TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN IF NOT EXISTS failed_logins            INT NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN IF NOT EXISTS locked_until             TIMESTAMPTZ;

-- The first account of the instance is its Master.
UPDATE users SET is_master = true
 WHERE id = (SELECT id FROM users ORDER BY created_at LIMIT 1)
   AND NOT EXISTS (SELECT 1 FROM users WHERE is_master);

-- Grants (allow = true) and removals (allow = false) of single permissions for one user in one
-- organization, applied over the role preset in every project of that organization.
CREATE TABLE IF NOT EXISTS member_permissions (
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission  TEXT NOT NULL,
    allow       BOOLEAN NOT NULL,
    PRIMARY KEY (org_id, user_id, permission)
);
