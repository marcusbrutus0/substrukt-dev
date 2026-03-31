-- Create deployments table
CREATE TABLE deployments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id INTEGER,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    webhook_url TEXT NOT NULL,
    webhook_auth_token TEXT,
    include_drafts INTEGER NOT NULL DEFAULT 0,
    auto_deploy INTEGER NOT NULL DEFAULT 0,
    debounce_seconds INTEGER NOT NULL DEFAULT 300,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX idx_deployments_slug ON deployments (slug);

-- Create new deployment_state table
CREATE TABLE deployment_state (
    deployment_id INTEGER PRIMARY KEY REFERENCES deployments(id) ON DELETE CASCADE,
    last_fired_at TEXT
);

-- Recreate webhook_history with deployment_id
CREATE TABLE webhook_history_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    deployment_id INTEGER NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    trigger_source TEXT NOT NULL,
    status TEXT NOT NULL,
    http_status INTEGER,
    error_message TEXT,
    response_time_ms INTEGER,
    attempt INTEGER NOT NULL DEFAULT 1,
    group_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_webhook_history_new_deploy ON webhook_history_new (deployment_id, created_at DESC);
CREATE INDEX idx_webhook_history_new_group ON webhook_history_new (group_id);

-- Note: existing webhook_history rows are NOT migrated because they reference
-- environment strings ("staging"/"production") that may not correspond to any
-- deployment. The old data is dropped. This is acceptable because webhook history
-- is informational, not critical. The old table is dropped after the new one is created.
DROP TABLE IF EXISTS webhook_history;
ALTER TABLE webhook_history_new RENAME TO webhook_history;

-- Drop old webhook_state
DROP TABLE IF EXISTS webhook_state;
