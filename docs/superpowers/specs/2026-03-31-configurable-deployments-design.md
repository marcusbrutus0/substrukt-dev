# Configurable Deployments Design

## Motivation

The current webhook system is hardcoded to two environments: staging and production. Webhook URLs and auth tokens are passed as CLI flags, which means:

1. **No runtime configuration.** Adding or changing a deployment target requires restarting the server with different flags.
2. **Fixed to two slots.** Users who need three targets (e.g., staging, preview, production) or only one cannot express that.
3. **No per-deployment settings.** The staging cron fires on a fixed interval for all content. There is no way to configure debounce per target or toggle draft inclusion.
4. **No UI management.** Webhook configuration is invisible to the user — it lives in CLI args or environment variables, not in the admin interface.

This spec replaces the hardcoded system with a "Deployments" section where admins create named deployment endpoints through the UI. Each deployment has its own webhook URL, auth token, include_drafts toggle, and optional auto-deploy with configurable debounce. The CLI flags for webhook URLs and the fixed cron are removed entirely.

## Goals

- Admins can create, edit, and delete deployment targets through the UI.
- Each deployment has independent configuration: webhook URL, auth token, include_drafts, auto_deploy, debounce.
- Editors can manually fire any deployment.
- Auto-deploy replaces the fixed staging cron with per-deployment configurable debounce.
- Webhook history and retry logic continue to work, keyed by deployment rather than environment string.
- The design works with the current single-app architecture but can be extended to per-app deployments when multi-app support arrives.

## Non-Goals (Out of Scope)

- **Multi-app scoping.** Deployments are global for now. The multi-app spec will add `app_id` scoping later.
- **Deployment-aware API filtering.** The `include_drafts` flag is metadata in the webhook payload — the API does not gain deployment-context-aware filtering. Webhook consumers use `?status=all` themselves if they want drafts.
- **Encrypted webhook auth tokens.** Tokens are stored as plaintext in SQLite, matching the security posture of the current CLI flag approach. The SQLite file should be protected by filesystem permissions.
- **Deployment pipelines or chaining.** No "deploy staging then production" workflows.
- **Webhook payload customization.** The payload shape stays fixed.
- **Preview/draft URL generation.** No shareable preview links tied to deployments.

## Architecture Decision: Single-App Now, Multi-App Later

The multi-app spec introduces an `apps` table with `app_id` foreign keys everywhere. That table does not exist yet. Rather than creating a dummy "default app" row or adding a non-existent FK, the `deployments` table uses a nullable `app_id` column with no foreign key constraint. All deployments created before multi-app have `app_id = NULL`, meaning they are global.

When multi-app arrives, a migration will:
1. Create the `apps` table.
2. Add a FK constraint to `deployments.app_id`.
3. Assign all existing NULL-app_id deployments to the default app (or require admin action).

This approach avoids coupling to a table that does not exist while making the upgrade path straightforward.

## Architecture Decision: Background Task Strategy

**Option A: One tokio task per auto-deploy deployment.** Each task has its own poll/debounce loop and CancellationToken. Task lifecycle tied to deployment CRUD.

**Option B: Single coordinator task** that iterates all auto-deploy deployments in a loop, sleeping for the shortest debounce interval.

**Option C: Channel-based notification** where content mutations push events to a channel, and a coordinator decides which deployments to fire.

**Decision: Option A (one task per deployment).**

Rationale:
- Isolation: each deployment's debounce timer is independent. A slow webhook in one deployment does not delay another.
- Simplicity: CancellationToken from `tokio-util` provides clean task lifecycle management. The current `spawn_cron` uses a bare `tokio::spawn` loop with no cancellation — the new approach is strictly better because tasks can be stopped on CRUD operations.
- Scalability: in practice, a Substrukt instance will have 1-5 deployments. The overhead of a few tokio tasks is negligible.
- Clean lifecycle: create deployment with auto_deploy -> spawn task. Delete deployment -> cancel token. Toggle auto_deploy off -> cancel token. Toggle on -> spawn. Update any field -> cancel + respawn. No complex iteration logic.

Option B was rejected because a single loop creates coupling between deployments and complicates debounce tracking. Option C was rejected because it requires a pub/sub system that adds complexity without benefit for the expected deployment count.

## Architecture Decision: include_drafts Semantics

**Option A: Deployment-aware API filtering.** The API learns about deployments and filters drafts based on deployment context (via a query parameter or token scope).

**Option B: Metadata in webhook payload.** `include_drafts` is included in the webhook payload. The webhook consumer decides whether to use `?status=all` when calling the content API.

**Decision: Option B (metadata in payload).**

Rationale:
- The API is currently stateless with respect to deployments. Adding deployment awareness would require either scoped API tokens (complex) or a query parameter that duplicates existing `?status=all` functionality (redundant).
- The webhook consumer already knows its deployment context — it received the webhook. It can append `?status=all` to its API calls if include_drafts is true.
- This keeps the API surface unchanged and the deployment system self-contained.

## Data Model

### New Table: `deployments` (audit database)

```sql
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
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_deployments_slug ON deployments (slug);
```

The table lives in the audit database (`audit.db`) alongside `webhook_state` and `webhook_history`. This keeps all deployment/webhook data in one database, separate from the user/auth database (`substrukt.db`).

`app_id` is nullable with no FK constraint (see architecture decision above). The UNIQUE index is on `slug` alone (not `(app_id, slug)`) since there is no multi-app yet. When multi-app arrives, the index changes to `UNIQUE(app_id, slug)`.

`updated_at` is added (not present in the original draft spec) to track when a deployment was last modified, useful for UI display and debugging.

Slug validation: lowercase alphanumeric + hyphens, no leading/trailing hyphens, 1-64 characters. Same rules as schema slugs.

### Modified Table: `webhook_state`

Currently:
```sql
CREATE TABLE webhook_state (
    environment TEXT PRIMARY KEY,
    last_fired_at TEXT
);
```

New:
```sql
CREATE TABLE deployment_state (
    deployment_id INTEGER PRIMARY KEY REFERENCES deployments(id) ON DELETE CASCADE,
    last_fired_at TEXT
);
```

The old `webhook_state` table is dropped. The new table uses `deployment_id` as the primary key. Rows are created lazily when a deployment is first fired (INSERT ... ON CONFLICT ... DO UPDATE in `mark_deployment_fired`).

Note: the current `mark_fired` uses `UPDATE ... WHERE environment = ?`, relying on rows pre-seeded by migration 002. The new `mark_deployment_fired` uses `INSERT ... ON CONFLICT ... DO UPDATE` instead, so no pre-seeding is needed.

### Modified Table: `webhook_history`

Currently:
```sql
CREATE TABLE webhook_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    environment TEXT NOT NULL,
    ...
);
```

New: `environment TEXT` column replaced with `deployment_id INTEGER NOT NULL REFERENCES deployments(id) ON DELETE CASCADE`. The index changes from `(environment, created_at DESC)` to `(deployment_id, created_at DESC)`.

### Migration: `audit_migrations/004_configurable_deployments.sql`

```sql
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
```

History data from the old `webhook_history` table is not migrated. The old rows referenced string environment names that have no corresponding deployment ID. Attempting to create "legacy" deployment records during a SQL migration is fragile (the migration has no access to CLI flag values). The history is informational — losing it on upgrade is acceptable.

### Rust Struct

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct Deployment {
    pub id: i64,
    pub app_id: Option<i64>,
    pub name: String,
    pub slug: String,
    pub webhook_url: String,
    pub webhook_auth_token: Option<String>,
    pub include_drafts: bool,
    pub auto_deploy: bool,
    pub debounce_seconds: i64,
    pub created_at: String,
    pub updated_at: String,
}
```

### Removed from Config

The following fields are removed from `Config` and the corresponding CLI flags from `Cli`:

- `staging_webhook_url: Option<String>`
- `staging_webhook_auth_token: Option<String>`
- `production_webhook_url: Option<String>`
- `production_webhook_auth_token: Option<String>`
- `webhook_check_interval: u64`

## Routing

### UI Routes

All deployment routes live under `/deployments` (no app prefix, since multi-app does not exist yet).

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/deployments` | editor+ | List all deployments with status and history |
| GET | `/deployments/new` | admin | Create deployment form |
| POST | `/deployments` | admin | Create deployment |
| GET | `/deployments/{slug}/edit` | admin | Edit deployment form |
| POST | `/deployments/{slug}` | admin | Update deployment |
| POST | `/deployments/{slug}/delete` | admin | Delete deployment |
| POST | `/deployments/{slug}/fire` | editor+ | Manually fire webhook |

These routes are registered in a new `src/routes/deployments.rs` module, nested under `/deployments` in `build_router` (same pattern as `/schemas`, `/content`, `/uploads`). They sit inside the CSRF + auth middleware layers.

Note: the current `/settings/webhooks` page is admin-only. The new `/deployments` list is editor+ because editors need to see deployment status and fire webhooks. Only CRUD operations (create/edit/delete) remain admin-restricted. This is an intentional broadening of visibility.

### API Routes

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/deployments` | any token | List deployments (names, slugs, config — auth token omitted) |
| POST | `/api/v1/deployments/{slug}/fire` | editor+ | Fire webhook |

API routes added to `src/routes/api.rs`.

### Removed Routes

| Removed | Replacement |
|---------|-------------|
| `POST /publish/{environment}` (UI) | `POST /deployments/{slug}/fire` |
| `POST /api/v1/publish/{environment}` (API) | `POST /api/v1/deployments/{slug}/fire` |
| `GET /settings/webhooks` (UI) | `GET /deployments` (history shown inline) |
| `POST /settings/webhooks/retry` (UI) | `POST /deployments/{slug}/fire` |

The `src/routes/publish.rs` module is deleted entirely.

### Forward Compatibility

When multi-app arrives, routes will move under `/apps/{app-slug}/deployments/...` and `/api/v1/apps/{app-slug}/deployments/...`. The handler logic stays the same; only the routing prefix and the app_id parameter change.

## Deployment CRUD — Database Methods

New methods on `AuditLogger` (since deployments live in the audit database):

```rust
pub async fn create_deployment(
    &self,
    name: &str,
    slug: &str,
    webhook_url: &str,
    webhook_auth_token: Option<&str>,
    include_drafts: bool,
    auto_deploy: bool,
    debounce_seconds: i64,
) -> eyre::Result<Deployment>

pub async fn get_deployment_by_slug(&self, slug: &str) -> eyre::Result<Option<Deployment>>

pub async fn get_deployment_by_id(&self, id: i64) -> eyre::Result<Option<Deployment>>

pub async fn list_deployments(&self) -> eyre::Result<Vec<Deployment>>

pub async fn update_deployment(
    &self,
    id: i64,
    name: &str,
    slug: &str,
    webhook_url: &str,
    webhook_auth_token: Option<&str>,
    include_drafts: bool,
    auto_deploy: bool,
    debounce_seconds: i64,
) -> eyre::Result<()>

pub async fn delete_deployment(&self, id: i64) -> eyre::Result<()>

pub async fn list_auto_deploy_deployments(&self) -> eyre::Result<Vec<Deployment>>
```

All methods use the same `self.pool` as existing audit methods. The `Deployment` struct is defined in `src/audit.rs` alongside `AuditLogEntry` and `WebhookHistoryGroup`.

## Dirty Detection

### Current Approach

`is_dirty(environment: &str)` compares `MAX(timestamp)` from `audit_log` (for mutation actions) against `last_fired_at` from `webhook_state` for the given environment string.

### New Approach

`is_dirty_for_deployment(deployment_id: i64)` does the same comparison but reads `last_fired_at` from `deployment_state` keyed by `deployment_id`. The audit_log query remains the same — it checks for any content/schema mutation, regardless of app scope (since there is only one app).

The mutation action filter includes `entry_published` and `entry_unpublished` in addition to the existing CRUD actions. These are audit events from the per-entry publish workflow that represent content state changes visible to consumers. A deployment should fire when entries change publish status, not just when content is created/updated/deleted.

```rust
pub async fn is_dirty_for_deployment(&self, deployment_id: i64) -> eyre::Result<bool> {
    let last_fired: Option<(Option<String>,)> =
        sqlx::query_as("SELECT last_fired_at FROM deployment_state WHERE deployment_id = ?")
            .bind(deployment_id)
            .fetch_optional(self.pool.as_ref())
            .await?;

    let last_fired_at = match last_fired {
        Some((Some(ts),)) => ts,
        _ => return Ok(true), // Never fired -> dirty
    };

    let latest_mutation: (Option<String>,) = sqlx::query_as(
        "SELECT MAX(timestamp) FROM audit_log WHERE action IN (\
            'content_create', 'content_update', 'content_delete', \
            'schema_create', 'schema_update', 'schema_delete', \
            'entry_published', 'entry_unpublished')",
    )
    .fetch_one(self.pool.as_ref())
    .await?;

    match latest_mutation {
        (Some(ts),) => Ok(ts > last_fired_at),
        _ => Ok(false),
    }
}

pub async fn mark_deployment_fired(&self, deployment_id: i64) -> eyre::Result<String> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO deployment_state (deployment_id, last_fired_at) VALUES (?, ?)
         ON CONFLICT(deployment_id) DO UPDATE SET last_fired_at = excluded.last_fired_at"
    )
    .bind(deployment_id)
    .bind(&now)
    .execute(self.pool.as_ref())
    .await?;
    Ok(now)
}
```

The old `is_dirty(environment)` and `mark_fired(environment)` methods are removed.

When multi-app arrives, the audit_log query gains a `WHERE app_id = ?` filter. The deployment's `app_id` is passed to `is_dirty_for_deployment`.

## Webhook Firing

### Modified `fire_webhook` Signature

Currently:
```rust
pub async fn fire_webhook(
    client: &reqwest::Client,
    audit: &AuditLogger,
    config: &Config,
    environment: &str,
    source: TriggerSource,
) -> Result<bool>
```

New:
```rust
pub async fn fire_webhook(
    client: &reqwest::Client,
    audit: &AuditLogger,
    deployment: &Deployment,
    source: TriggerSource,
) -> Result<bool>
```

The function no longer reads URL/token from `Config`. Instead, it reads from the `Deployment` struct directly. The `Ok(false)` return case (URL not configured) is removed — a deployment always has a URL. Returns `Ok(true)` on first-attempt success, `Err` on first-attempt failure (with background retries spawned).

### Webhook Payload

```json
{
    "event_type": "substrukt-publish",
    "deployment": "preview",
    "include_drafts": true,
    "triggered_at": "2026-03-31T10:00:00Z",
    "triggered_by": "manual"
}
```

Changes from current payload:
- `environment` field renamed to `deployment` (contains the deployment slug).
- `include_drafts` field added (boolean).
- `triggered_by` values: `"manual"`, `"auto"` (replaces `"cron"`), `"retry"`.

The `WebhookPayload` struct changes accordingly:

```rust
#[derive(Serialize)]
struct WebhookPayload {
    event_type: &'static str,
    deployment: String,       // was: environment
    include_drafts: bool,     // new
    triggered_at: String,
    triggered_by: &'static str,
}
```

### Webhook History

`record_webhook_attempt` changes its first parameter from `environment: &str` to `deployment_id: i64`. The `list_webhook_history` method changes its environment filter to a deployment filter:

```rust
pub async fn record_webhook_attempt(
    &self,
    deployment_id: i64,
    trigger_source: &str,
    status: &str,
    http_status: Option<u16>,
    error_message: Option<&str>,
    response_time_ms: Option<i64>,
    attempt: i32,
    group_id: &str,
) -> eyre::Result<i64>

/// List webhook history, optionally filtered by deployment and/or status.
/// Pass `deployment_id: None` to list history across all deployments (used by the
/// deployments list page). Pass `Some(id)` to filter to a single deployment.
pub async fn list_webhook_history_for_deployment(
    &self,
    deployment_id: Option<i64>,
    status_filter: Option<&str>,
) -> eyre::Result<Vec<WebhookHistoryGroup>>
```

The `WebhookHistoryGroup` struct replaces `environment: String` with `deployment_id: i64` and adds `deployment_name: String` and `deployment_slug: String` (populated via JOIN).

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct WebhookHistoryGroup {
    pub id: i64,
    pub deployment_id: i64,
    pub deployment_name: String,
    pub deployment_slug: String,
    pub trigger_source: String,
    pub status: String,
    pub http_status: Option<i32>,
    pub error_message: Option<String>,
    pub response_time_ms: Option<i64>,
    pub attempt_count: i32,
    pub group_id: String,
    pub created_at: String,
}
```

### Retry Logic

Unchanged in structure: first attempt synchronous, 2 background retries at 5s and 30s. The spawned retry task clones `Deployment` (it derives Clone) instead of reading from Config.

## Background Tasks & Auto-Deploy

### AppState Changes

```rust
pub struct AppStateInner {
    // ... existing fields ...
    pub deploy_tasks: DashMap<i64, tokio_util::sync::CancellationToken>,
}
```

The `deploy_tasks` DashMap tracks running auto-deploy tasks by deployment ID. Stored in `AppState` so route handlers can manage task lifecycle on deployment CRUD.

Note: `CancellationToken` is from `tokio_util::sync`, not `tokio::sync`. The `tokio-util` crate must be added as a direct dependency.

### Startup

In `run_server`, after creating the `AppState`:

```rust
// Spawn auto-deploy tasks for all enabled deployments
if let Ok(deployments) = state.audit.list_auto_deploy_deployments().await {
    for deployment in deployments {
        spawn_auto_deploy_task(&state, deployment);
    }
}
```

This replaces the current `webhooks::spawn_cron(...)` call.

### `spawn_auto_deploy_task`

New function in `src/webhooks.rs`:

```rust
use tokio_util::sync::CancellationToken;

pub fn spawn_auto_deploy_task(state: &AppState, deployment: Deployment) {
    let cancel_token = CancellationToken::new();
    let child_token = cancel_token.child_token();
    state.deploy_tasks.insert(deployment.id, cancel_token);

    let client = state.http_client.clone();
    let audit = state.audit.clone();
    let poll_interval = Duration::from_secs(30);
    let debounce = Duration::from_secs(deployment.debounce_seconds as u64);

    tokio::spawn(async move {
        loop {
            // Check dirty
            let dirty = match audit.is_dirty_for_deployment(deployment.id).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Dirty check failed for deployment {}: {e}", deployment.slug);
                    false
                }
            };

            if dirty {
                // Debounce: wait, then re-check
                tokio::select! {
                    _ = tokio::time::sleep(debounce) => {},
                    _ = child_token.cancelled() => return,
                }

                // Re-check after debounce
                let still_dirty = audit.is_dirty_for_deployment(deployment.id).await.unwrap_or(false);
                if still_dirty {
                    tracing::info!("Auto-deploying {}", deployment.slug);
                    if let Err(e) = fire_webhook(&client, &audit, &deployment, TriggerSource::Auto).await {
                        tracing::warn!("Auto-deploy webhook failed for {}: {e}", deployment.slug);
                    }
                }
            }

            // Sleep until next poll
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {},
                _ = child_token.cancelled() => return,
            }
        }
    });
}
```

### `cancel_auto_deploy_task`

```rust
pub fn cancel_auto_deploy_task(state: &AppState, deployment_id: i64) {
    if let Some((_, token)) = state.deploy_tasks.remove(&deployment_id) {
        token.cancel();
    }
}
```

### Task Lifecycle on CRUD

- **Create deployment** with `auto_deploy = true`: call `spawn_auto_deploy_task`.
- **Update deployment**: always cancel existing task (if any). If the updated deployment has `auto_deploy = true`, spawn a new task with the fresh `Deployment` struct. This is necessary because the spawned task captures the `Deployment` by value — any field change (URL, auth token, debounce, include_drafts) requires a respawn to pick up new values. Unconditionally cancelling and conditionally respawning is simpler and safer than diffing individual fields.
- **Delete deployment**: call `cancel_auto_deploy_task`.

### Manual Fire + Auto-Deploy Interaction

When a manual fire succeeds, it calls `mark_deployment_fired`, which updates `last_fired_at`. The next time the auto-deploy task checks `is_dirty_for_deployment`, it will see the deployment as clean (assuming no new mutations since the manual fire). This naturally prevents double-firing without needing explicit debounce-reset coordination.

If a manual fire happens during the auto-deploy debounce window:
1. The manual fire calls `mark_deployment_fired` immediately.
2. After the debounce sleep, the auto-deploy task re-checks `is_dirty_for_deployment`.
3. Since `last_fired_at` was just updated by the manual fire, and no new mutations happened, the check returns `false`.
4. The auto-deploy task skips firing and loops back to polling.

This works because the debounce window includes a mandatory re-check after sleeping. No explicit notification or reset mechanism is needed.

## UI

### Deployments List Page (`/deployments`)

Template: `templates/deployments/list.html`

Page layout:
- Header: "Deployments" with "Create Deployment" button (admin only)
- Table of deployments with columns:
  - Status dot (green = clean, amber = dirty)
  - Name
  - URL (truncated to ~40 chars with ellipsis)
  - Mode badge: "Auto" (with debounce seconds) or "Manual"
  - Include drafts badge (if enabled)
  - Last fired (relative time, e.g., "2 hours ago")
  - Actions: "Fire" button (editor+), "Edit" / "Delete" links (admin only)
- Empty state: "No deployments configured. Create one to start deploying."
- Below the table: webhook history section showing the last 50 webhook attempts across all deployments, with deployment name column. Same columns as current webhooks page: time, deployment, source, status, HTTP code, response time, attempts.

### Create/Edit Deployment Form

Template: `templates/deployments/form.html` (shared for create and edit)

Fields:
- **Name** -- text input, required. Label: "Deployment Name". Placeholder: "e.g., Production, Staging, Preview".
- **Slug** -- text input, auto-generated from name via JS (same pattern as schema slug generation). Editable. Label: "Slug". Help text: "Used in webhook payload and URLs. Lowercase letters, numbers, and hyphens."
- **Webhook URL** -- text input (type=url), required. Label: "Webhook URL". Placeholder: "https://api.example.com/deploy".
- **Auth Token** -- text input (type=password), optional. Label: "Auth Token". Help text: "Sent as Bearer token in webhook requests. Leave blank for no auth." On edit, shows placeholder dots if a token exists; submitting empty clears it.
- **Include Drafts** -- checkbox. Label: "Include draft content". Help text: "When enabled, the webhook payload includes `include_drafts: true`. Your build system can use this to fetch draft entries via `?status=all`."
- **Auto-deploy** -- checkbox. Label: "Auto-deploy on content changes".
- **Debounce** -- number input, visible only when auto-deploy is checked (toggled via JS). Label: "Debounce (seconds)". Default: 300. Min: 10. Help text: "Wait this long after the last change before auto-deploying. Prevents rapid re-deploys during editing sessions."

Submit button: "Create Deployment" or "Save Deployment".

### Nav Changes

The "Publish" section in `_nav.html` (lines 34-57, the `{% if user_role != "viewer" %}` block containing `get_publish_state()`, the staging/production form buttons, and their dirty-state dots) is removed entirely.

The "Webhooks" link in the admin section (line 30, `<a href="/settings/webhooks" ...>Webhooks</a>`) is also removed since its functionality moves to the deployments page.

Replaced with a "Deployments" link visible to editor+ users:

```html
{% if user_role != "viewer" %}
<a href="/deployments" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Deployments</a>
{% endif %}
```

Positioned after "Uploads" (line 24) and before "Users" (line 25) in the nav order.

The `get_publish_state()` template function in `src/templates.rs` is removed. The dirty-state checking moves to the deployments list page handler where it queries each deployment's dirty state from the database.

### Template Context

The deployments list handler passes:
- `deployments` -- list of deployment objects with all fields plus `is_dirty: bool` computed per deployment
- `history` -- recent webhook history entries (all deployments)
- Standard context: `csrf_token`, `user_role`, `base_template`

## Audit Events

All deployment-related audit events use `resource_type = "deployment"`:

| Action | Actor | Resource ID | Details | When |
|--------|-------|-------------|---------|------|
| `deployment_created` | user_id | deployment slug | `{"name": "..."}` | Admin creates deployment |
| `deployment_updated` | user_id | deployment slug | `{"changes": [...]}` | Admin updates deployment |
| `deployment_deleted` | user_id | deployment slug | `{"name": "..."}` | Admin deletes deployment |
| `deployment_fired` | user_id or `"api"` | deployment slug | `null` | Manual trigger (UI or API) |
| `deployment_auto_fired` | `"system"` | deployment slug | `null` | Auto-deploy trigger |

Note: `deployment_webhook_failed` from the original draft is removed. Webhook failures are already tracked in `webhook_history` with full status/error details. An additional audit log entry would be redundant.

`deployment_fired` and `deployment_auto_fired` are NOT counted as content mutations for dirty detection — they do not appear in the `is_dirty_for_deployment` action filter. This is intentional: deployment fires should not make other deployments "dirty."

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Create deployment with duplicate slug | 400, "A deployment with this slug already exists" |
| Create deployment with invalid slug (uppercase, spaces, etc.) | 400, "Invalid slug: ..." with validation message |
| Create deployment with empty name or URL | 400, form re-rendered with field-level errors |
| Fire deployment that does not exist | 404 |
| Fire deployment, webhook times out (10s reqwest timeout) | First attempt fails, background retries begin. UI flash: "Webhook failed -- retries in progress". API: 502 |
| Fire deployment, webhook returns non-2xx | Same as timeout |
| Fire deployment, webhook URL is unreachable | Same as timeout |
| Delete deployment while auto-deploy task is running | Task cancelled via CancellationToken. DB row cascade-deletes state and history. |
| Update deployment while auto-deploy task is running | Old task cancelled, new task spawned with updated Deployment values. The new task immediately re-evaluates dirty state. |
| Viewer tries to access /deployments | 403 (editor+ required) |
| Viewer tries to fire via API | 403 JSON error |
| Non-admin tries to create/edit/delete deployment (UI) | 403 |
| SQLite write failure on deployment create | 500, flash error or JSON error |
| Auto-deploy dirty check fails (SQLite error) | Logged via `tracing::warn`, task continues to next poll cycle |
| Auto-deploy task panics | Tokio task exits, logged. CancellationToken stays in DashMap but is inert. Next server restart re-spawns. No crash-recovery within a running instance -- acceptable for the expected deployment count. |

## Edge Cases and Failure Modes

1. **Server restart with auto-deploy deployments.** On startup, `list_auto_deploy_deployments()` is called and tasks are spawned. Any in-flight debounce windows from the previous process are lost — the task starts fresh. This may cause a deploy to fire slightly earlier or later than if the server had not restarted. Acceptable.

2. **Concurrent manual fires of the same deployment.** Both call `fire_webhook` and `mark_deployment_fired`. The webhook fires twice. This is harmless — webhook consumers should be idempotent (they typically trigger a build which is itself idempotent). No deduplication is attempted.

3. **Rapid CRUD on deployments.** Admin creates, then immediately updates, then deletes a deployment. Each operation manages the task lifecycle independently. The DashMap ensures cancel/spawn is safe even if the previous task hasn't fully exited yet (CancellationToken is async-safe).

4. **Deployment with auto_deploy but debounce_seconds < poll_interval.** The debounce sleep is shorter than the 30s poll interval. This means the deployment fires at most every `debounce_seconds + poll_interval` seconds. This is correct — the debounce is a minimum wait, not a maximum.

5. **No deployments configured.** The deployments list page shows an empty state. No background tasks run. The nav "Deployments" link is still visible (so users know the feature exists). The old publish buttons in the nav are gone regardless.

6. **Webhook auth token update without re-entering.** On the edit form, the auth token field shows placeholder dots but the actual value is not sent to the browser (security). If the admin submits the form without entering a new token, the existing token is preserved. If the admin clears the field and submits, the token is removed. Implementation: a hidden field `_token_action` with values `keep` (default) or `clear`/`update`.

7. **Slug collision across future apps.** Currently the UNIQUE constraint is on `slug` alone. When multi-app arrives, two apps could want a deployment with the same slug (e.g., both have "production"). The migration to multi-app changes the constraint to `UNIQUE(app_id, slug)`.

8. **Clock skew in dirty detection.** `is_dirty_for_deployment` compares RFC3339 timestamps. If the server clock jumps backward, `last_fired_at` could be in the "future" relative to new audit_log entries, making the deployment appear clean when it is dirty. This is the same limitation as the current `is_dirty` implementation and is accepted as a known constraint of timestamp-based comparison.

## Files Changed

### New Files

- **`audit_migrations/004_configurable_deployments.sql`** -- Migration creating `deployments`, `deployment_state`, and rebuilding `webhook_history`.
- **`src/routes/deployments.rs`** -- Route handlers for deployment CRUD, fire, and list.
- **`templates/deployments/list.html`** -- Deployments list with status, history, and actions.
- **`templates/deployments/form.html`** -- Create/edit deployment form.

### Modified Files

- **`src/audit.rs`** -- Add `Deployment` struct, deployment CRUD methods, `is_dirty_for_deployment`, `mark_deployment_fired`, updated `record_webhook_attempt` and `list_webhook_history` signatures (deployment_id instead of environment), updated `WebhookHistoryGroup` struct. Remove `is_dirty(environment)`, `mark_fired(environment)`.
- **`src/webhooks.rs`** -- Change `fire_webhook` signature to accept `&Deployment`. Add `TriggerSource::Auto` variant (replacing `Cron`). Add `spawn_auto_deploy_task` and `cancel_auto_deploy_task`. Remove `spawn_cron`. Update `WebhookPayload` to use `deployment` field and `include_drafts`. Add `use tokio_util::sync::CancellationToken`.
- **`src/config.rs`** -- Remove `staging_webhook_url`, `staging_webhook_auth_token`, `production_webhook_url`, `production_webhook_auth_token`, `webhook_check_interval` fields and their `Config::new` parameters.
- **`src/main.rs`** -- Remove CLI flags for webhook URLs, auth tokens, and check interval. Remove `spawn_cron` call. Add auto-deploy task startup loop. Pass `deploy_tasks` to AppState.
- **`src/state.rs`** -- Add `deploy_tasks: DashMap<i64, tokio_util::sync::CancellationToken>` to `AppStateInner`. Add `use tokio_util::sync::CancellationToken`.
- **`src/routes/mod.rs`** -- Add `pub mod deployments;`. Replace `.nest("/publish", publish_routes)` with `.nest("/deployments", deployments_routes)`. Remove `pub mod publish;`.
- **`src/templates.rs`** -- Remove `get_publish_state()` template function and its closure. Remove the `audit_logger` and `config` parameters from `create_reloader` since they were only used by `get_publish_state()`. The function signature becomes `create_reloader(schemas_dir: PathBuf) -> AutoReloader`.
- **`templates/_nav.html`** -- Remove publish section (lines 34-57: staging/production buttons and dirty dots). Remove "Webhooks" link (line 30). Add "Deployments" link (editor+ visible, between Uploads and Users).
- **`src/routes/api.rs`** -- Remove `publish` handler. Add `list_deployments` and `fire_deployment` API handlers. Remove `/publish/{environment}` route, add `/deployments` and `/deployments/{slug}/fire` routes.
- **`src/routes/settings.rs`** -- Remove `webhooks_page` handler, `retry_webhook` handler, `WebhookFilter` struct, `RetryForm` struct, and their route registrations (`.route("/webhooks", get(webhooks_page))` and `.route("/webhooks/retry", axum::routing::post(retry_webhook))`).

### Deleted Files

- **`src/routes/publish.rs`** -- Entire module removed. Functionality replaced by `/deployments/{slug}/fire`.
- **`templates/settings/webhooks.html`** -- Entire template removed. Webhook history now displayed on the deployments list page.

### Not Changed

- **`src/content/mod.rs`** -- Content CRUD unchanged.
- **`src/content/form.rs`** -- Form rendering unchanged.
- **`src/sync/mod.rs`** -- Export/import unchanged (deployments are not part of the bundle — they are instance-specific configuration).
- **`src/cache.rs`** -- Cache unchanged.
- **`src/history.rs`** -- Version history unchanged.
- **`src/db/`** -- Main database unchanged (deployments live in audit.db).
- **`src/auth/`** -- Auth unchanged.

## Dependencies

### New Crate

`tokio-util` -- for `CancellationToken`. Add to `Cargo.toml`:

```toml
tokio-util = { version = "0.7", features = ["rt"] }
```

Note: `tokio-util` is already present as a transitive dependency (via reqwest/hyper) in `Cargo.lock`, but it must be added as a direct dependency to use it in application code.

All other dependencies are already present (`dashmap`, `tokio`, `reqwest`, `sqlx`, etc.).

## Testing

### Unit Tests (in `src/audit.rs`)

- `create_deployment` returns a deployment with correct fields
- `get_deployment_by_slug` returns None for nonexistent slug
- `list_deployments` returns all deployments sorted by name
- `update_deployment` updates all fields including `updated_at`
- `delete_deployment` cascade-deletes `deployment_state` and `webhook_history` rows
- `is_dirty_for_deployment` returns true when never fired
- `is_dirty_for_deployment` returns false after `mark_deployment_fired` with no new mutations
- `is_dirty_for_deployment` returns true after `mark_deployment_fired` with subsequent mutation
- `is_dirty_for_deployment` returns true after `entry_published` event (verify publish status changes are detected)
- `mark_deployment_fired` creates row on first fire (INSERT ... ON CONFLICT)
- `mark_deployment_fired` updates row on subsequent fires
- `record_webhook_attempt` with deployment_id writes and returns row id
- `list_webhook_history_for_deployment` with `None` returns all deployments
- `list_webhook_history_for_deployment` with `Some(id)` filters to that deployment

### Integration Tests

- Create deployment via UI (admin session, POST with CSRF) -- verify redirect and DB row
- Create deployment with duplicate slug -- verify error message
- Create deployment with invalid slug -- verify validation error
- Edit deployment -- verify updated fields in DB
- Delete deployment -- verify row removed, history cascade-deleted
- Fire deployment via UI -- verify webhook attempt recorded in history
- Fire deployment via API -- verify 200 response with status
- Fire deployment with unreachable URL -- verify error response, retry spawned
- List deployments via API -- verify JSON response with deployment data, auth tokens omitted
- Viewer cannot access /deployments (403)
- Non-admin cannot create/edit/delete deployments (403)
- Auto-deploy: create deployment with auto_deploy=true, insert audit mutation, wait for poll+debounce, verify webhook fires (this test needs a mock HTTP server)
- Auto-deploy: manual fire during debounce prevents double-fire
- Slug validation: reject uppercase, spaces, leading hyphens
- Old routes return 404: `POST /publish/staging`, `GET /settings/webhooks`

## Implementation Order

1. **Migration + data model** -- Create `004_configurable_deployments.sql`, add `Deployment` struct and CRUD methods to `audit.rs`. Add `is_dirty_for_deployment` and `mark_deployment_fired`.
2. **Config cleanup** -- Remove webhook CLI flags from `Config`, `Cli`, and `Config::new`.
3. **Webhook refactor** -- Change `fire_webhook` to accept `Deployment`. Update `TriggerSource`. Remove `spawn_cron`. Add `spawn_auto_deploy_task` / `cancel_auto_deploy_task`.
4. **State changes** -- Add `deploy_tasks` to `AppStateInner`. Update `run_server` to spawn auto-deploy tasks.
5. **UI routes** -- Create `src/routes/deployments.rs` with all handlers. Create templates.
6. **API routes** -- Add deployment list and fire endpoints to `src/routes/api.rs`.
7. **Nav + template cleanup** -- Remove publish section from `_nav.html`, remove "Webhooks" link from admin section, remove `get_publish_state()` from `templates.rs`, simplify `create_reloader` signature, add Deployments link.
8. **Remove old routes** -- Delete `src/routes/publish.rs`, remove `/publish` routes from router, remove `/settings/webhooks` routes and handlers from `settings.rs`, delete `templates/settings/webhooks.html`.
9. **Integration tests.**

Steps 1-3 form a coherent unit (data + core logic). Steps 4-5 form a unit (state + UI). Steps 6-8 are cleanup. This ordering ensures the codebase compiles at each step (old routes can coexist with new routes temporarily during development).
