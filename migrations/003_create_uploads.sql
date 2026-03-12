CREATE TABLE IF NOT EXISTS uploads (
    hash TEXT PRIMARY KEY,
    filename TEXT NOT NULL,
    mime TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS upload_references (
    upload_hash TEXT NOT NULL REFERENCES uploads(hash) ON DELETE CASCADE,
    schema_slug TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    PRIMARY KEY (upload_hash, schema_slug, entry_id)
);
