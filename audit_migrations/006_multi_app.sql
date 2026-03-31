-- Multi-app support for audit.db

-- Assign all NULL-app_id deployments to the default app (id=1)
UPDATE deployments SET app_id = 1 WHERE app_id IS NULL;

-- Allow the same deployment slug across different apps
DROP INDEX IF EXISTS idx_deployments_slug;
CREATE UNIQUE INDEX idx_deployments_slug_app ON deployments (app_id, slug);

-- Add app_id to audit_log
ALTER TABLE audit_log ADD COLUMN app_id INTEGER;
CREATE INDEX idx_audit_app_id ON audit_log (app_id);
