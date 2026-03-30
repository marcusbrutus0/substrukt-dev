# Configurable Deployments Design

## Overview

Replace the hardcoded staging/production webhook system with a "Deployments" section where users create named deployment endpoints per app. Each deployment has its own webhook URL, auth token, and configuration for whether drafts are included and whether it auto-fires on content changes (with configurable debounce). The current CLI flags for webhook URLs and the fixed cron are removed entirely.

## Data Model

### New Table

```sql
CREATE TABLE deployments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    webhook_url TEXT NOT NULL,
    webhook_auth_token TEXT,
    include_drafts INTEGER NOT NULL DEFAULT 0,
    auto_deploy INTEGER NOT NULL DEFAULT 0,
    debounce_seconds INTEGER NOT NULL DEFAULT 300,
    created_at TEXT NOT NULL,
    UNIQUE(app_id, slug)
);
```

### Modified Tables

- `webhook_state`: replace `environment TEXT` primary key with `deployment_id INTEGER REFERENCES deployments(id) ON DELETE CASCADE`
- `webhook_history`: replace `environment TEXT` with `deployment_id INTEGER REFERENCES deployments(id) ON DELETE CASCADE`

### Removed from Config

- `staging_webhook_url`
- `staging_webhook_auth_token`
- `production_webhook_url`
- `production_webhook_auth_token`
- `webhook_check_interval`

And all corresponding CLI flags.

## Routing

### UI Routes (under app scope)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/apps/{app-slug}/deployments` | editor+ | List deployments |
| GET | `/apps/{app-slug}/deployments/new` | admin | Create form |
| POST | `/apps/{app-slug}/deployments` | admin | Create deployment |
| GET | `/apps/{app-slug}/deployments/{deploy-slug}/edit` | admin | Edit form |
| POST | `/apps/{app-slug}/deployments/{deploy-slug}` | admin | Update deployment |
| POST | `/apps/{app-slug}/deployments/{deploy-slug}/delete` | admin | Delete deployment |
| POST | `/apps/{app-slug}/deployments/{deploy-slug}/fire` | editor+ | Manually fire webhook |

### API Routes

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/apps/{app-slug}/deployments` | any | List deployments |
| POST | `/api/v1/apps/{app-slug}/deployments/{deploy-slug}/fire` | editor+ | Fire webhook |

### Removed Routes

- `POST /publish/{environment}` (UI and API)

## Background Tasks & Auto-Deploy

### Task Lifecycle

- **Startup:** query all deployments where `auto_deploy = 1`, spawn a tokio task per deployment
- **Create** (with auto_deploy): spawn task
- **Update:** auto_deploy toggled on → spawn; toggled off → cancel; debounce changed → cancel and respawn
- **Delete:** cancel task if running

### Task Registry

`DashMap<i64, tokio::sync::CancellationToken>` keyed by deployment ID, stored in `AppState`. Used to cancel tasks on update/delete.

### Auto-Deploy Task Loop

1. Check if this app has dirty content since `last_fired_at` for this deployment (audit log query filtered by `app_id`)
2. If dirty: sleep for `debounce_seconds`, then re-check (content might still be changing)
3. If still dirty after debounce: fire webhook
4. If clean: sleep for poll interval (30s) and check again
5. Listen for cancellation token throughout

### Dirty Detection

`is_dirty` becomes app-scoped — audit log queries filter by `app_id` when checking for mutations after `last_fired_at` for a given deployment.

### Manual Fire

Bypasses debounce, fires immediately. If an auto-deploy task is mid-debounce for the same deployment, the manual fire resets the debounce timer to prevent double-firing.

## Webhook Payload & Firing

### Payload

Same shape as current, deployment slug replaces environment:

```json
{
    "event_type": "substrukt-publish",
    "environment": "preview",
    "triggered_at": "2026-03-31T10:00:00Z",
    "triggered_by": "manual"
}
```

`environment` field uses the deployment's slug. `triggered_by` values: `"manual"`, `"cron"` (auto-deploy), `"retry"`.

### Include Drafts

Controls whether draft entries are included when the webhook consumer fetches content via the API:

- `include_drafts = false` (default): drafts filtered out of API responses (current behavior)
- `include_drafts = true`: drafts included alongside published entries

API content endpoints gain deployment context (via token's deployment scope or query param) to determine draft filtering.

### Retry Logic

Unchanged: first attempt inline, 2 background retries at 5s and 30s delays, grouped by UUID.

### History

`webhook_history` rows reference `deployment_id` instead of environment string. Same grouping and filtering logic.

## UI

### Deployments List Page (`/apps/{app-slug}/deployments`)

Table of deployments showing:
- Name, URL (truncated), auto/manual badge, dirty/clean status dot, last fired time
- "Fire" button per deployment (editor+)
- "Create Deployment" button (admin only)
- Edit/delete links per deployment (admin only)

Webhook history shown below the table or as expandable section per deployment, filtered by deployment. Same columns as current: time, source, status, HTTP code, response time, attempts, retry button.

### Create/Edit Deployment Form

- Name (text input)
- Slug (auto-generated from name, editable)
- Webhook URL
- Auth token (password field, optional)
- Include drafts (toggle)
- Auto-deploy (toggle)
- Debounce seconds (number input, shown when auto-deploy is on)

### Nav Changes

"Publish" section in sidebar replaced with "Deployments" link (visible to editor+). No more inline staging/production buttons in the nav. Users go to the deployments page to fire webhooks.

Dirty indicator moves from nav dots to the deployments list page (per-deployment status dot).

## Audit

Deployment-related audit events (all with `app_id` set):
- `deployment_created`
- `deployment_updated`
- `deployment_deleted`
- `deployment_fired` — manual trigger
- `deployment_auto_fired` — auto-deploy trigger
- `deployment_webhook_failed`
