CREATE TABLE backup_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    frequency_hours INTEGER NOT NULL DEFAULT 24,
    retention_count INTEGER NOT NULL DEFAULT 7,
    enabled INTEGER NOT NULL DEFAULT 0
);

INSERT INTO backup_config (id) VALUES (1);

CREATE TABLE backup_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    status TEXT NOT NULL,
    trigger_source TEXT NOT NULL,
    error_message TEXT,
    size_bytes INTEGER,
    s3_key TEXT,
    manifest TEXT
);

CREATE INDEX idx_backup_history_started ON backup_history (started_at DESC);
