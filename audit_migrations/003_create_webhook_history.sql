CREATE TABLE IF NOT EXISTS webhook_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    environment TEXT NOT NULL,
    trigger_source TEXT NOT NULL,
    status TEXT NOT NULL,
    http_status INTEGER,
    error_message TEXT,
    response_time_ms INTEGER,
    attempt INTEGER NOT NULL DEFAULT 1,
    group_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_webhook_history_env ON webhook_history (environment, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_webhook_history_group ON webhook_history (group_id);
