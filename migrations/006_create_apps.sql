-- Multi-app support: apps, app_access, and app-scoped tokens/uploads

CREATE TABLE apps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slug TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE app_access (
    app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (app_id, user_id)
);

-- Seed the default app for migration from single-app
INSERT INTO apps (slug, name) VALUES ('default', 'Default');

-- Add app_id to api_tokens
ALTER TABLE api_tokens ADD COLUMN app_id INTEGER REFERENCES apps(id) ON DELETE CASCADE;
UPDATE api_tokens SET app_id = 1;

-- Rebuild uploads with (app_id, hash) composite PK
CREATE TABLE uploads_new (
    app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    hash TEXT NOT NULL,
    filename TEXT NOT NULL,
    mime TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (app_id, hash)
);

INSERT INTO uploads_new (app_id, hash, filename, mime, size, created_at)
    SELECT 1, hash, filename, mime, size, created_at FROM uploads;

DROP TABLE uploads;
ALTER TABLE uploads_new RENAME TO uploads;

-- Rebuild upload_references with app_id in composite PK
CREATE TABLE upload_references_new (
    app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    upload_hash TEXT NOT NULL,
    schema_slug TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    PRIMARY KEY (app_id, upload_hash, schema_slug, entry_id)
);

INSERT INTO upload_references_new (app_id, upload_hash, schema_slug, entry_id)
    SELECT 1, upload_hash, schema_slug, entry_id FROM upload_references;

DROP TABLE upload_references;
ALTER TABLE upload_references_new RENAME TO upload_references;
