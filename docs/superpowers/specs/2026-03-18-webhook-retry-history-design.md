# Webhook Retry and History Design

## Goal

Add automatic retry with exponential backoff for failed webhooks, and a history page to view past webhook activity and manually retry failures.

## Current State

- `fire_webhook()` in `webhooks.rs` fires once, returns Ok/Err, no retry
- `webhook_state` table tracks only `last_fired_at` per environment
- Webhook results are logged to `audit_log` but there's no structured webhook history
- Cron fires staging webhook when dirty, no retry on failure
- UI shows a flash message on publish (success/failed), no history

## Design

### Database

New `webhook_history` table in the audit database (`audit_migrations/003_create_webhook_history.sql`):

```sql
CREATE TABLE webhook_history (
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

CREATE INDEX idx_webhook_history_env ON webhook_history (environment, created_at DESC);
```

- `environment`: "staging" or "production"
- `trigger_source`: "manual", "cron", or "retry"
- `status`: "success", "failed", or "retrying"
- `http_status`: HTTP response status code, NULL if network error
- `error_message`: NULL on success, error description on failure
- `response_time_ms`: round-trip time in milliseconds
- `attempt`: which attempt number (1, 2, or 3)
- `group_id`: UUID linking all attempts of one logical webhook fire

No retention limit — webhooks fire infrequently, table stays small.

### Retry Logic

Approach: background retry with `tokio::spawn`. First attempt is synchronous so the caller gets immediate feedback. Retries happen in the background.

Flow:
1. Generate a `group_id` (UUID) for this logical fire
2. Make the first HTTP attempt, measure response time
3. Record the attempt to `webhook_history`
4. Return the first attempt's result to the caller
5. If the first attempt failed, spawn a background task that:
   - Waits 5 seconds, then retries (attempt 2)
   - If that fails, waits 30 seconds, then retries (attempt 3)
   - Each retry is recorded to `webhook_history` with the same `group_id`
   - On any success, calls `mark_fired()` to update dirty state
   - The initial attempt's history row is left as "failed" (not updated to "retrying") — each attempt is its own row

Retry schedule: 3 total attempts (1 initial + 2 retries), delays of 5s and 30s.

Callers (publish routes, cron, API) don't change their control flow — they still get Ok/Err for the first attempt and show the same flash/response. Retries are transparent.

### New Methods on AuditLogger

```
record_webhook_attempt(environment, trigger_source, status, http_status, error_message, response_time_ms, attempt, group_id) -> Result<i64>
list_webhook_history(environment_filter, status_filter) -> Result<Vec<WebhookHistoryEntry>>
```

History recording failures are logged but never block the webhook flow.

### Settings Page

New admin-only route: `/settings/webhooks`

Page contents:
- Header: "Webhooks"
- Filter bar: environment dropdown (All / Staging / Production), status dropdown (All / Success / Failed)
- History table columns: Time, Environment, Source, Status, HTTP Status, Response Time, Attempts, Actions
- Rows grouped by `group_id` — latest attempt shows in main row, "N attempts" badge if retries occurred
- Failed entries (all attempts exhausted) show a "Retry" button
- Empty state: "No webhook activity yet"

Retry button: POST `/settings/webhooks/retry` with `environment` form field + CSRF. Calls `fire_webhook` with `TriggerSource::Manual`.

Nav: "Webhooks" link added between "API Tokens" and "Data", admin-only.

### Testing

Unit tests in `audit.rs`:
- `record_webhook_attempt` writes and returns row id
- `list_webhook_history` returns entries in descending order
- Filtering by environment and status works

Integration tests:
- Webhook fire records history entry
- Failed webhook triggers background retries (multiple entries with same `group_id`)
- Webhooks settings page loads and shows history
- Retry button fires new webhook with new history
- Non-admin cannot access webhooks page (403)

### Error Handling

- History recording failures are logged via `tracing::warn` but never block webhook delivery
- Background retry task logs errors but does not propagate them
- The retry spawned task is fire-and-forget — server continues normally regardless of retry outcomes

### Files Changed

- Create: `audit_migrations/003_create_webhook_history.sql`
- Modify: `src/audit.rs` — add `record_webhook_attempt`, `list_webhook_history` methods
- Modify: `src/webhooks.rs` — add retry loop, record history on each attempt
- Modify: `src/routes/settings.rs` — add webhooks page handler and retry handler
- Create: `templates/settings/webhooks.html`
- Modify: `templates/_nav.html` — add Webhooks link (admin-only)
- Modify: `tests/integration.rs` — add webhook history/retry tests
