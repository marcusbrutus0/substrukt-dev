# Publish Workflow Design

## Problem

Substrukt is a backend CMS for static site generators. Admins edit content through the CMS UI, and an SSG pulls data from the API to build static sites. There is currently no mechanism to trigger SSG builds or separate staging from production deployments.

## Solution

Add a webhook-based publish system. A background cron detects content changes and auto-fires a staging webhook. Admins manually trigger production publishes via a UI button. Both staging and production are just different webhook targets pointing at the same Substrukt instance's API.

## Architecture

### Environments

- **Dev**: Local. No webhooks. Admins trigger SSG manually.
- **Staging**: Auto-triggered by background cron when dirty. Also manually triggerable via UI button.
- **Production**: Manually triggered only, via "Publish Production" button in admin UI.

All three environments read from the same Substrukt instance and API. The difference is only which webhook fires and when.

### Config (CLI args)

New flags on the `serve` command:

- `--staging-webhook-url <URL>` — URL to POST for staging builds. Optional.
- `--production-webhook-url <URL>` — URL to POST for production publishes. Optional.
- `--webhook-check-interval <SECONDS>` — How often the cron checks for dirty state. Default: 300 (5 minutes).

Webhook URLs are deployment config (secrets), never exposed in the UI.

### SQLite: `webhook_state` table

Lives in `audit.db` (same database as the audit log), so dirty detection can query both tables on the same connection pool.

```sql
CREATE TABLE webhook_state (
    environment TEXT PRIMARY KEY,  -- 'staging' or 'production'
    last_fired_at TEXT             -- ISO 8601 timestamp, nullable
);
```

Seeded on startup using `INSERT OR IGNORE` so that existing `last_fired_at` values survive server restarts.

Migration goes in `audit_migrations/`.

### Dirty Detection

Two queries on the `audit.db` pool (same connection):

```sql
SELECT MAX(timestamp) FROM audit_log
WHERE action IN (
    'content_create', 'content_update', 'content_delete',
    'schema_create', 'schema_update', 'schema_delete'
)
```

```sql
SELECT last_fired_at FROM webhook_state WHERE environment = ?
```

Compare in Rust:
- If `last_fired_at` is NULL → always dirty (never built).
- If latest mutation timestamp > `last_fired_at` → dirty.
- Otherwise → clean.

Only data mutation events count. Login, token creation, import/export, etc. do not trigger dirty state.

### AuditLogger: Expose Read Access

The existing `AuditLogger` only exposes a fire-and-forget `log()` method. The inner `SqlitePool` is not public. To support dirty detection queries, add a public method to `AuditLogger`:

```rust
impl AuditLogger {
    /// Returns whether there are data mutations since the given environment's last webhook fire.
    pub async fn is_dirty(&self, environment: &str) -> Result<bool> { ... }

    /// Updates last_fired_at for an environment. Returns the updated timestamp.
    pub async fn mark_fired(&self, environment: &str) -> Result<String> { ... }
}
```

This keeps the audit pool encapsulated — no need to expose the raw pool or store it separately in `AppState`.

### Background Cron (Staging Auto-fire)

A `tokio::spawn` task started at server boot. Captures clones of: `Config` (for webhook URLs and interval), `AuditLogger` (for dirty checks and marking fired), and a shared `reqwest::Client`.

1. Sleeps for `webhook_check_interval` seconds.
2. Checks if a staging webhook URL is configured. If not, skips.
3. Calls `audit_logger.is_dirty("staging")`.
4. If dirty: fires HTTP POST to staging webhook URL.
   - On success (2xx): calls `audit_logger.mark_fired("staging")`.
   - On failure: logs error, does NOT update `last_fired_at` (retries next cycle).
5. Loop.

The interval itself acts as the debounce window — rapid edits within one interval coalesce into a single webhook fire.

Production is never auto-fired. Only the staging cron runs automatically.

### Webhook HTTP Call

**Request**:
- Method: `POST`
- Headers: `Content-Type: application/json`, `User-Agent: Substrukt/0.1`
- Timeout: 10 seconds
- Body:
  ```json
  {
    "event_type": "substrukt-publish",
    "environment": "staging" | "production",
    "triggered_at": "2026-03-12T14:30:00Z",
    "triggered_by": "cron" | "manual"
  }
  ```

The `event_type` field makes the payload compatible with GitHub Actions `repository_dispatch` events. Generic enough for any webhook receiver.

No HMAC signing — the webhook URL itself is a secret (e.g., contains a GitHub token). Signing can be added later if needed.

**Error handling**:
- HTTP success (2xx): update `last_fired_at`.
- HTTP failure (non-2xx, timeout, network error): do NOT update `last_fired_at`. The CI never started, so content is still unpublished. Log the error.

### HTTP Client

A single `reqwest::Client` instance stored in `AppState`, shared between the cron task and the manual publish API/UI routes. Avoids creating a new connection pool per webhook call.

Add `reqwest` as a direct dependency with features: `json`, `rustls-tls`. The existing dev-dependency (used for integration tests with `cookies`, `multipart`) remains separate.

### API Endpoints

New endpoints, bearer token auth (same as existing API):

- `POST /api/v1/publish/staging` — fires staging webhook, updates timestamp on success.
- `POST /api/v1/publish/production` — fires production webhook, updates timestamp on success.

Both always fire regardless of dirty state. Return:
- `{"status": "triggered"}` on webhook success.
- `404` if webhook URL not configured.
- `502` with error details if webhook HTTP call fails.

### UI Routes

Session-auth, CSRF-protected:

- `POST /publish/staging` — fires staging webhook. Redirects back with flash message.
- `POST /publish/production` — fires production webhook. Redirects back with flash message.

Flash messages:
- Success: "Staging build triggered" / "Production publish triggered"
- Failure: "Webhook failed — check configuration"

### UI: Sidebar Nav Buttons

Two buttons in the sidebar nav (`_nav.html`), below the existing navigation items:

- **"Build Staging"** — POST to `/publish/staging`. Hidden if no staging webhook URL configured.
- **"Publish Production"** — POST to `/publish/production`. Hidden if no production webhook URL configured.

Each button has a dirty indicator dot:
- Amber/orange dot: data mutations exist since `last_fired_at`.
- Green dot: clean (no changes since last build).

Dirty state is injected via a minijinja global function (similar to existing `get_nav_schemas()`) so it is available in all templates without modifying every route handler. Buttons always fire when clicked (dirty indicator is informational only, not a gate).

### Audit Trail

Webhook fires are logged to the audit log with action `webhook_fire` and details including environment, trigger source (cron/manual), and success/failure. These events are excluded from dirty detection (only data mutations count).

## Files to Create/Modify

- `src/config.rs` — add webhook URL fields and check interval
- `src/webhooks.rs` — new module: webhook firing, dirty checking, cron task
- `src/audit.rs` — add `is_dirty()` and `mark_fired()` methods, `webhook_state` seeding
- `src/state.rs` — add `reqwest::Client` to `AppState`
- `src/routes/api.rs` — new publish API endpoints
- `src/routes/` — new UI publish routes (or add to existing admin routes)
- `templates/_nav.html` — add publish buttons with dirty indicators
- `audit_migrations/` — new migration for `webhook_state` table
- `Cargo.toml` — add `reqwest` as direct dependency
