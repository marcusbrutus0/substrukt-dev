# Multi-App Support Design

## Overview

Substrukt becomes multi-app: a single instance hosts multiple independent apps, each with its own schemas, content, uploads, and history. Users create all apps explicitly — there is no implicit default. Admins have full access to all apps; non-admin users (editors, viewers) are granted access to specific apps.

## Data Model & Storage

### New SQLite Tables

```sql
CREATE TABLE apps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slug TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE app_access (
    app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (app_id, user_id)
);
```

### Modified Tables

- `api_tokens`: add `app_id INTEGER NOT NULL REFERENCES apps(id)`
- `uploads`: add `app_id INTEGER NOT NULL REFERENCES apps(id)`
- `upload_references`: add `app_id INTEGER NOT NULL REFERENCES apps(id)`
- `audit_log`: add `app_id INTEGER` (nullable — global events like user creation have no app)

### Disk Layout

```
data/
├── {app-slug}/
│   ├── schemas/
│   ├── content/
│   ├── uploads/
│   └── _history/
├── substrukt.db
└── audit.db
```

Databases stay at the `data/` root, shared across all apps.

### Slug Validation

Lowercase alphanumeric + hyphens, no leading/trailing hyphens, max 64 characters.

## Routing

### New Routes

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/apps` | any user | App list (dashboard) |
| GET | `/apps/new` | admin | Create app form |
| POST | `/apps` | admin | Create app |
| GET | `/apps/{app-slug}/settings` | admin | App settings (access, tokens, danger zone) |
| POST | `/apps/{app-slug}/delete` | admin | Delete app |

### App-Scoped Routes

All existing content routes move under `/apps/{app-slug}/`:

- `/apps/{app-slug}/schemas/*`
- `/apps/{app-slug}/content/*`
- `/apps/{app-slug}/uploads/*`
- `/apps/{app-slug}/publish/*`

### Global Routes (unchanged paths)

- `/settings/users` — user management
- `/settings/invitations` — invitations
- `/login`, `/logout`, `/signup`
- `/healthz`, `/metrics`

### API Routes

- `/api/v1/apps/{app-slug}/schemas/*`
- `/api/v1/apps/{app-slug}/content/*`
- `/api/v1/apps/{app-slug}/uploads/*`
- `/api/v1/apps/{app-slug}/export`
- `/api/v1/apps/{app-slug}/import`
- `/api/v1/apps/{app-slug}/publish/*`

### Root Redirect

`/` redirects to `/apps`.

## App Extractor

Custom Axum extractor (`AppContext`) applied to all app-scoped routes:

1. Reads `{app-slug}` from the path
2. Looks up the app in the DB by slug (404 if not found)
3. For UI: checks current user has access (admin = always, others = check `app_access`) — 403 if denied
4. For API: checks token's `app_id` matches the requested app — 403 if mismatch
5. Returns the app record (id, slug, name) to the handler

## Cache & File Watcher

### Cache Keying

Single `DashMap` shared across all apps. Keys prefixed with app slug: `"{app-slug}/{schema-slug}/{entry-id}"`.

### Cache Operations

- `populate(cache, data_dir)` — startup: iterate all app directories under `data/`, load each app's schemas and content
- `rebuild_app(cache, data_dir, app_slug)` — clear keys with app prefix, repopulate from disk
- `remove_app(cache, app_slug)` — clear all keys with app prefix (on app deletion)

### File Watcher

Single watcher on `data/` recursively. On change, extract the app slug from the path (first directory component under `data/`) and rebuild that app's cache only.

### App Lifecycle

- **Create:** `mkdir -p data/{app-slug}/{schemas,content,uploads,_history}`, insert into `apps` table. No cache action (empty app).
- **Delete:** remove `data/{app-slug}/` from disk, `remove_app(cache, app_slug)`, cascade-delete DB records via foreign keys (`app_access`, `api_tokens`, `uploads`, `upload_references` rows for this app are all deleted).

## Auth & Access Control

### UI Requests

1. `require_auth` middleware checks session (unchanged)
2. `AppContext` extractor on app-scoped routes:
   - Admin → access granted to all apps
   - Editor/viewer → check `app_access` table, 403 if no entry

### API Requests

1. Bearer token validated (unchanged)
2. Token's `app_id` compared to requested app's id (403 if mismatch)
3. Token's inherited role governs read/write permissions within the app

### App Management

- Only admins can create, delete, and configure apps
- Only admins can manage app access (assign/remove users)

### Dashboard Behavior

- Admin sees all apps
- Editor/viewer sees only apps in their `app_access` entries
- Zero-app users see an empty state message

## UI & Navigation

### App List Page (`/apps`)

Grid of app cards showing name, slug, and schema count. "Create App" button visible to admins only. Clicking an app navigates to `/apps/{app-slug}/content` (or per-app landing if no schemas exist yet).

### Per-App Navigation

- Top of nav: app name with "Back to apps" link
- Body: same as current nav (schemas list, uploads, etc.) scoped to current app
- Admin-only within app: app settings link

### App Settings Page (`/apps/{app-slug}/settings`)

- App name (editable)
- User access: list of non-admin users with toggle to grant/revoke
- API tokens for this app
- Webhooks config for this app
- Danger zone: delete app (with confirmation)

### Global Settings (`/settings`)

- Users management (unchanged)
- Invitations (unchanged)
- Audit log with app filter dropdown

### Template Changes

- All templates receive `app` context variable (slug, name) when inside an app-scoped route
- URL generation uses `app.slug` prefix
- `get_nav_schemas()` becomes app-scoped (reads from `data/{app-slug}/schemas/`)

## Export/Import

### Export

Per-app. `POST /api/v1/apps/{app-slug}/export` produces a tar.gz:

```
├── app.json              # app metadata (name, slug)
├── uploads-manifest.json # upload metadata for this app
├── schemas/
├── content/
└── uploads/
```

### Import

`POST /api/v1/apps/{app-slug}/import` accepts a tar.gz. Overwrites the target app's schemas, content, and uploads. `app.json` in the bundle is informational — target app's slug/name is not changed.

No cross-app import. To clone an app: create a new app, then import the source app's bundle.

## Audit Log & Metrics

### Audit Log

- App-scoped actions (schema CRUD, content CRUD, uploads, publish) include `app_id`
- Global actions (user create, login, app create/delete) have `app_id = NULL`
- Viewer gets app filter dropdown
- Single `audit.db` — no per-app split

### Metrics

- Add `app` label to content-related Prometheus metrics (e.g. `substrukt_content_entries{app="blog"}`)
- Global metrics (login attempts, request counts) remain unlabeled by app
