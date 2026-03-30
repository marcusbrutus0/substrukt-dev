# Per-Entry Draft/Publish Workflow Design

## Motivation

The current draft/publish system is bulk-only: `publish_all_drafts()` flips every draft entry across every schema to `"published"` as a side effect of firing a deployment webhook. This creates several problems:

1. **No granular control.** An editor cannot publish a single finished entry while leaving other in-progress entries as drafts. Publishing one entry forces publishing all of them.
2. **Publish is coupled to deployments.** The only way to mark content as published is to trigger a webhook. If you want to publish an entry without deploying, you cannot.
3. **No unpublish.** Once published, there is no way to revert an entry to draft status through the UI or API.
4. **Confusing mental model.** The "Publish Production" button both changes content status AND fires a webhook -- two unrelated concerns bundled together.

This spec decouples publish/unpublish from deployments entirely. Each entry gets its own publish/unpublish action. Deployments become purely about notifying external consumers -- they no longer mutate content status.

## Goals

- Editors and admins can publish or unpublish individual entries.
- New entries default to draft (unchanged from current behavior).
- Publish/unpublish is a distinct action from saving content and from triggering deployments.
- The API supports per-entry status changes.
- The existing `_status` field, filtering, and backwards compatibility are preserved.

## Non-Goals (Out of Scope)

- **Scheduled publishing.** No "publish at time X" feature.
- **Bulk publish from UI.** No "select all and publish" checkbox UI. The API can be scripted for bulk operations.
- **Editorial workflows.** No review/approve/reject states. Status remains binary: draft or published.
- **Draft previews with share links.** No anonymous preview URLs for draft content.
- **Per-schema publish settings.** No "all entries in this schema default to published."
- **Configurable deployments.** That is a separate spec (`2026-03-31-configurable-deployments-design.md`). This spec assumes the current webhook system but is designed to be forward-compatible with configurable deployments.

## Architecture Decision: Dedicated Endpoints vs. Inline Status Field

**Option A: Dedicated publish/unpublish endpoints.** Separate `POST` routes that change `_status` only. The edit form's save action never touches `_status`.

**Option B: Inline `_status` in the update payload.** Clients include `_status` in the JSON body of a PUT/POST update. No new routes.

**Decision: Option A (dedicated endpoints) for UI, Option B allowed for API.**

Rationale:
- For the UI, a dedicated endpoint is semantically correct. Publishing is not an edit -- it is a workflow action. A dedicated button that calls a dedicated endpoint makes the user's intent unambiguous and allows htmx to handle it without a full form submission.
- For the API, allowing `_status` in the update payload is the pragmatic choice. API clients (CI scripts, external tools) should be able to set status in the same call that updates content, avoiding a two-request workflow. The `save_entry` function already preserves `_status` from the existing entry; the new behavior is that if the incoming data explicitly contains `_status`, that value takes precedence.
- This hybrid approach serves both audiences without compromise.

## Data Model

No changes to the data model. The `_status` field already exists in content entries:

```json
{
  "_id": "my-post",
  "_status": "draft",
  "title": "My Post",
  "body": "..."
}
```

Values: `"draft"` or `"published"`. Missing `_status` treated as `"published"` (backwards compatibility, unchanged).

## Routing

### New UI Routes

Added to the existing content router in `src/routes/content.rs` via `content::routes()`. These routes are nested under `/content` by `build_router` in `src/routes/mod.rs`, which means they inherit the `verify_csrf` and `require_auth` middleware layers.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/content/{schema_slug}/{entry_id}/publish` | editor+ | Set entry status to published |
| POST | `/content/{schema_slug}/{entry_id}/unpublish` | editor+ | Set entry status to draft |

CSRF protection: These routes accept `application/x-www-form-urlencoded` POST bodies containing a `_csrf` hidden field. The existing `verify_csrf` middleware in `src/auth/mod.rs` handles validation automatically for urlencoded forms -- no manual CSRF check needed in the handler. For htmx requests (which also send urlencoded form data), the same middleware applies transparently.

Both routes work for singles (`entry_id` = `_single`).

Route registration in `content::routes()`:

```rust
.route(
    "/{schema_slug}/{entry_id}/publish",
    axum::routing::post(publish_entry),
)
.route(
    "/{schema_slug}/{entry_id}/unpublish",
    axum::routing::post(unpublish_entry),
)
```

**Route ordering note:** These routes must be registered before the existing `/{schema_slug}/{entry_id}` catch-all route. Axum's router matches by specificity, so `/publish` and `/unpublish` as literal trailing segments will match before the `{entry_id}` parameter. However, to be safe and explicit, register the publish/unpublish routes first in the chain.

### New API Routes

Added to the existing API router in `src/routes/api.rs` via `api::routes()`. These routes are nested under `/api/v1` by `build_router`, outside the auth/CSRF middleware (API routes use bearer token auth instead).

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/v1/content/{schema_slug}/{entry_id}/publish` | bearer token, editor+ | Set entry status to published |
| POST | `/api/v1/content/{schema_slug}/{entry_id}/unpublish` | bearer token, editor+ | Set entry status to draft |

Return `200 {"status": "published"}` or `200 {"status": "draft"}` on success. Return `404` if entry not found.

Route registration in `api::routes()`:

```rust
.route(
    "/content/{schema_slug}/{entry_id}/publish",
    post(api_publish_entry),
)
.route(
    "/content/{schema_slug}/{entry_id}/unpublish",
    post(api_unpublish_entry),
)
```

**Route ordering note:** Same concern as UI routes. Register these before the existing `/content/{schema_slug}/{entry_id}` route in the API router. The existing API router already has `/content/{schema_slug}/single` for singles, so there is no collision with `publish`/`unpublish` as those are nested under `{entry_id}`.

Additionally, the existing `PUT /api/v1/content/{schema_slug}/{entry_id}` and `PUT /api/v1/content/{schema_slug}/single` endpoints gain the ability to set `_status` if included in the request body (see API Changes section).

### Modified Routes

The existing `POST /publish/{environment}` UI route (`src/routes/publish.rs`) and `POST /api/v1/publish/{environment}` API route (`src/routes/api.rs`) no longer call `publish_all_drafts()`. They only fire the webhook.

### Removed

No routes are removed in this spec. The `publish/{environment}` routes remain for webhook firing. (The configurable deployments spec will remove them later.)

## Content Module Changes

### `set_entry_status(content_dir, schema, entry_id, status) -> Result<()>`

New public function in `src/content/mod.rs`. Reads the entry from disk, sets `_status` to the given value, writes back. Does NOT create a version history snapshot (status change is metadata, not content). Does NOT go through `save_entry` (avoids re-validation, re-generation of ID, and other save-time side effects).

```rust
/// Set the _status of an entry without modifying its content.
/// Does not create a history snapshot (metadata-only change).
pub fn set_entry_status(
    content_dir: &Path,
    schema: &SchemaFile,
    entry_id: &str,
    status: &str,
) -> eyre::Result<()> {
    // Validate status value
    if !matches!(status, "draft" | "published") {
        eyre::bail!("Invalid status: {status}. Must be \"draft\" or \"published\".");
    }

    let slug = &schema.meta.slug;
    match schema.meta.storage {
        StorageMode::Directory => {
            let path = content_dir.join(slug).join(format!("{entry_id}.json"));
            if !path.exists() {
                eyre::bail!("Entry not found: {slug}/{entry_id}");
            }
            let content = std::fs::read_to_string(&path)?;
            let mut data: Value = serde_json::from_str(&content)?;
            if let Some(obj) = data.as_object_mut() {
                obj.insert("_status".to_string(), Value::String(status.to_string()));
            }
            std::fs::write(&path, serde_json::to_string_pretty(&data)?)?;
        }
        StorageMode::SingleFile => {
            let path = content_dir.join(format!("{slug}.json"));
            if !path.exists() {
                eyre::bail!("Entry not found: {slug}/{entry_id}");
            }
            if schema.meta.kind == Kind::Single {
                let content = std::fs::read_to_string(&path)?;
                let mut data: Value = serde_json::from_str(&content)?;
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("_status".to_string(), Value::String(status.to_string()));
                }
                std::fs::write(&path, serde_json::to_string_pretty(&data)?)?;
            } else {
                // Collection in single file
                let content = std::fs::read_to_string(&path)?;
                let mut entries: Vec<Value> = serde_json::from_str(&content)?;
                let found = entries.iter_mut().any(|e| {
                    let matches = e.get("_id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s == entry_id);
                    if matches {
                        if let Some(obj) = e.as_object_mut() {
                            obj.insert("_status".to_string(), Value::String(status.to_string()));
                        }
                    }
                    matches
                });
                if !found {
                    eyre::bail!("Entry not found: {slug}/{entry_id}");
                }
                std::fs::write(&path, serde_json::to_string_pretty(&entries)?)?;
            }
        }
    }
    Ok(())
}
```

Implementation details:
- For `StorageMode::Directory`: read `data/content/{slug}/{entry_id}.json`, set `_status`, write back.
- For `StorageMode::SingleFile` with `Kind::Single`: read `data/content/{slug}.json` as a single JSON object, set `_status`, write back.
- For `StorageMode::SingleFile` with `Kind::Collection`: read `data/content/{slug}.json` as a JSON array, find the entry by `_id`, set `_status`, write back the entire array.
- Returns `Err` if the entry does not exist on disk.
- Validates `status` at the top of the function. Only `"draft"` and `"published"` are accepted.

### `save_entry` Changes

Currently, `save_entry` (lines 106-192 of `src/content/mod.rs`) always determines `_status` from the existing entry (update path) or defaults to `"draft"` (create path). The current code unconditionally overwrites whatever `_status` may be present in the incoming `data`:

```rust
// Current code (lines 114-129):
let status = if let Some(eid) = entry_id {
    get_entry(content_dir, schema, eid)
        .ok()
        .flatten()
        .and_then(|e| {
            e.data
                .get("_status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "draft".to_string())
} else {
    "draft".to_string()
};
```

New behavior: check whether the incoming `data` already contains a valid `_status` field. If so, respect it. If not, fall back to the current logic. Replace the above block with:

```rust
let status = if let Some(explicit) = data.get("_status").and_then(|v| v.as_str()) {
    // Caller explicitly set _status (API use case) — respect it
    match explicit {
        "draft" | "published" => explicit.to_string(),
        _ => "draft".to_string(), // invalid values fall back to draft
    }
} else if let Some(eid) = entry_id {
    // Update path: preserve existing status from disk
    get_entry(content_dir, schema, eid)
        .ok()
        .flatten()
        .and_then(|e| {
            e.data
                .get("_status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "draft".to_string())
} else {
    // Create path: default to draft
    "draft".to_string()
};
```

The subsequent code that injects `_status` into the data object (lines 132-134) remains unchanged:

```rust
if let Some(obj) = data.as_object_mut() {
    obj.insert("_status".to_string(), Value::String(status));
}
```

This is backwards-compatible: the UI form never includes `_status` in form fields (it is not a schema property and `form_data_to_json` does not generate it), so UI saves continue to preserve existing status via the disk-read fallback. API clients that previously sent `_status` in PUT/POST bodies had that value silently overwritten; now it is respected. This is a deliberate, intentional behavior change for the API.

### `publish_all_drafts` Removal

The `publish_all_drafts` function (lines 272-339 of `src/content/mod.rs`) is removed. The associated test `publish_all_drafts_flips_status` (lines 582-625) is also removed. All call sites are updated:

1. **`src/routes/publish.rs` lines 30-46** (`publish` handler): Remove the `publish_all_drafts` call and the conditional cache rebuild. The handler becomes:

```rust
async fn publish(
    State(state): State<AppState>,
    session: Session,
    Path(environment): Path<String>,
) -> impl IntoResponse {
    if auth::require_role(&session, "editor").await.is_err() {
        return (axum::http::StatusCode::FORBIDDEN, "Insufficient permissions")
            .into_response();
    }
    if !matches!(environment.as_str(), "staging" | "production") {
        return Redirect::to("/").into_response();
    }

    let label = if environment == "staging" {
        "Staging build"
    } else {
        "Production publish"
    };

    match crate::webhooks::fire_webhook(
        &state.http_client,
        &state.audit,
        &state.config,
        &environment,
        crate::webhooks::TriggerSource::Manual,
    )
    .await
    {
        Ok(true) => {
            auth::set_flash(&session, "success", &format!("{label} triggered")).await;
        }
        Ok(false) => {
            auth::set_flash(&session, "error", "Webhook URL not configured").await;
        }
        Err(e) => {
            tracing::warn!("Webhook failed for {environment}: {e}");
            auth::set_flash(&session, "error", "Webhook failed \u{2014} check configuration")
                .await;
        }
    }

    Redirect::to("/").into_response()
}
```

2. **`src/routes/api.rs` lines 784-807** (`publish` API handler): Remove the `publish_all_drafts` call and the conditional cache rebuild block. The handler fires the webhook directly.

The publish routes become thin wrappers that just fire the webhook.

### `strip_internal_status` Behavior

No change. `_status` continues to be stripped from API responses for list/get endpoints. The publish/unpublish endpoints return the status explicitly in their own response body (not from the entry data).

## UI Changes

### Edit Page (`templates/content/edit.html`)

The current edit page shows a static "Draft" badge in the `<h1>` header (line 7):

```html
{% if entry_status == "draft" %}<span class="ml-2 px-2 py-0.5 rounded text-xs font-medium bg-accent-soft text-accent align-middle">Draft</span>{% endif %}
```

Changes:

1. **Replace the static badge with an interactive publish/unpublish control.** When the entry is draft, show a "Draft" badge plus a "Publish" button. When published, show a "Published" badge plus an "Unpublish" button.

2. **The publish/unpublish buttons use htmx.** `hx-post` to the publish/unpublish endpoint. The form sends `application/x-www-form-urlencoded` data (the default for HTML forms) which the CSRF middleware handles. `hx-target="#entry-status"` with `hx-swap="outerHTML"` replaces the status control area inline.

3. **Placement:** In the `<h1>` header, replacing the current static badge. The status control is a `<span>` inline element (not a block `<div>`) so it flows naturally within the heading text.

4. **Viewers see the badge but not the button.** Gated on `user_role != "viewer"`.

5. **Hidden when `is_new == true`.** No status control on the new entry form -- the entry must be saved first.

Replace line 7 of `templates/content/edit.html`:

```html
{% if entry_status == "draft" %}<span class="ml-2 px-2 py-0.5 rounded text-xs font-medium bg-accent-soft text-accent align-middle">Draft</span>{% endif %}
```

With the status control (only shown when `not is_new`):

```html
{% if not is_new %}{% include "content/_status_control.html" %}{% endif %}
```

### Status Control Partial (`templates/content/_status_control.html`)

A dedicated partial template for the status control. This template is rendered both inline (when the edit page loads) and as an htmx fragment (when the publish/unpublish handler responds to an htmx request).

```html
<span id="entry-status" class="ml-2 align-middle inline-flex items-center gap-2">
  {% if entry_status == "draft" %}
    <span class="px-2 py-0.5 rounded text-xs font-medium bg-accent-soft text-accent">Draft</span>
    {% if user_role != "viewer" %}
    <form method="post" action="/content/{{ schema_slug }}/{{ entry_id }}/publish"
          hx-post="/content/{{ schema_slug }}/{{ entry_id }}/publish"
          hx-target="#entry-status" hx-swap="outerHTML"
          class="inline">
      <input type="hidden" name="_csrf" value="{{ csrf_token }}">
      <button type="submit"
              class="px-3 py-1 rounded text-xs font-medium bg-success text-white hover:bg-success-hover">
        Publish
      </button>
    </form>
    {% endif %}
  {% else %}
    <span class="px-2 py-0.5 rounded text-xs font-medium bg-success-soft text-success">Published</span>
    {% if user_role != "viewer" %}
    <form method="post" action="/content/{{ schema_slug }}/{{ entry_id }}/unpublish"
          hx-post="/content/{{ schema_slug }}/{{ entry_id }}/unpublish"
          hx-target="#entry-status" hx-swap="outerHTML"
          class="inline">
      <input type="hidden" name="_csrf" value="{{ csrf_token }}">
      <button type="submit"
              class="px-3 py-1 rounded text-xs font-medium border border-border text-secondary hover:bg-card-alt">
        Unpublish
      </button>
    </form>
    {% endif %}
  {% endif %}
</span>
```

Key design choices:
- Uses `<span>` (not `<div>`) as the outer element with `id="entry-status"` so it stays inline in the `<h1>`.
- `hx-swap="outerHTML"` replaces the entire `<span id="entry-status">` with the new fragment returned by the handler.
- Progressive enhancement: if JavaScript/htmx is disabled, the form submits normally (urlencoded POST), the server redirects back to the edit page, and the status is updated on reload.

### htmx Fragment Response

The publish/unpublish UI route handlers use the `HxRequest` extractor (from `axum-htmx`, already a dependency) to detect htmx requests:

- **htmx request (`HxRequest(true)`):** Render only `templates/content/_status_control.html` with the updated `entry_status`, `user_role`, `schema_slug`, `entry_id`, and `csrf_token` context. Return as `Html(fragment)`.
- **Non-htmx request:** Set a flash message and redirect to the edit page URL (`/content/{schema_slug}/{entry_id}/edit`).

This matches the existing pattern in the codebase where htmx requests get partial HTML and non-htmx requests get redirects. The existing `base_for_htmx` function is not used here because we are returning a sub-page fragment (a single `<span>`), not a full page wrapped in a base template.

### New Entry Page

No publish/unpublish button on the "new entry" form (`is_new == true`). The `{% include %}` is gated on `not is_new`. After creation, the user lands on the list page (current behavior); they can open the entry to publish it.

### Content List Page

No changes needed. The draft badge already shows on the list (line 38-40 of `templates/content/list.html`). No publish/unpublish buttons on the list page -- that action happens on the edit page only. This keeps the list page clean and avoids accidental publishes.

### Singles Edit Page

Works identically to collection entries. The `entry_id` for singles is `_single`, and the publish/unpublish routes accept that value. The status control renders the same way. The `edit_entry_page` handler already passes `entry_id` and `entry_status` to the template context for singles.

## Route Handler Implementation

### UI Handlers (`src/routes/content.rs`)

```rust
async fn publish_entry(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if auth::require_role(&session, "editor").await.is_err() {
        return (axum::http::StatusCode::FORBIDDEN, "Insufficient permissions")
            .into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        _ => {
            auth::set_flash(&session, "error", "Schema not found").await;
            return Redirect::to("/").into_response();
        }
    };

    if let Err(e) = content::set_entry_status(
        &state.config.content_dir(), &schema_file, &entry_id, "published",
    ) {
        tracing::error!("Publish failed: {e}");
        auth::set_flash(&session, "error", "Failed to publish entry").await;
        return Redirect::to(&format!("/content/{schema_slug}/{entry_id}/edit")).into_response();
    }

    crate::cache::reload_entry(
        &state.cache, &state.config.content_dir(), &schema_file, &entry_id,
    );

    let user_id = auth::current_user_id(&session).await.unwrap_or(0);
    state.audit.log(
        &user_id.to_string(),
        "entry_published",
        "content",
        &format!("{schema_slug}/{entry_id}"),
        None,
    );

    if is_htmx {
        let csrf_token = auth::ensure_csrf_token(&session).await;
        let user_role = auth::current_user_role(&session).await.unwrap_or_default();
        let tmpl = state.templates.acquire_env()
            .map_err(|e| format!("Template env error: {e}")).unwrap();
        let template = tmpl.get_template("content/_status_control.html")
            .map_err(|e| format!("Template error: {e}")).unwrap();
        let html = template.render(minijinja::context! {
            csrf_token => csrf_token,
            user_role => user_role,
            schema_slug => schema_slug,
            entry_id => entry_id,
            entry_status => "published",
        }).map_err(|e| format!("Render error: {e}")).unwrap();
        return Html(html).into_response();
    }

    auth::set_flash(&session, "success", "Entry published").await;
    Redirect::to(&format!("/content/{schema_slug}/{entry_id}/edit")).into_response()
}
```

The `unpublish_entry` handler is identical except it calls `set_entry_status(..., "draft")`, logs `"entry_unpublished"`, passes `entry_status => "draft"`, and flashes `"Entry unpublished"`.

### API Handlers (`src/routes/api.rs`)

```rust
async fn api_publish_entry(
    State(state): State<AppState>,
    token: BearerToken,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
    };

    if let Err(e) = content::set_entry_status(
        &state.config.content_dir(), &schema_file, &entry_id, "published",
    ) {
        let msg = e.to_string();
        if msg.contains("not found") {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": msg}))).into_response();
        }
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": msg}))).into_response();
    }

    crate::cache::reload_entry(
        &state.cache, &state.config.content_dir(), &schema_file, &entry_id,
    );

    state.audit.log(
        "api",
        "entry_published",
        "content",
        &format!("{schema_slug}/{entry_id}"),
        None,
    );

    Json(serde_json::json!({"status": "published", "entry_id": entry_id})).into_response()
}
```

The `api_unpublish_entry` handler is identical except it calls `set_entry_status(..., "draft")`, logs `"entry_unpublished"`, and returns `{"status": "draft", "entry_id": ...}`.

## API Changes

### New Endpoints

**`POST /api/v1/content/{schema_slug}/{entry_id}/publish`**

- Auth: Bearer token with editor+ role
- Request body: empty (or ignored)
- Response: `200 {"status": "published", "entry_id": "<id>"}`
- Error: `404` if entry not found, `403` if insufficient role, `500` on disk error

**`POST /api/v1/content/{schema_slug}/{entry_id}/unpublish`**

- Auth: Bearer token with editor+ role
- Request body: empty (or ignored)
- Response: `200 {"status": "draft", "entry_id": "<id>"}`
- Error: `404` if entry not found, `403` if insufficient role, `500` on disk error

Both endpoints are idempotent: publishing an already-published entry returns `200` with `{"status": "published"}`, and unpublishing an already-draft entry returns `200` with `{"status": "draft"}`. No error is returned when the entry is already in the requested state. This simplifies API client logic and avoids unnecessary error handling.

**Singles via API:** The current API uses `/content/{schema_slug}/single` for single-schema operations (GET, PUT, DELETE), not `/content/{schema_slug}/_single`. For publish/unpublish, singles use the `{entry_id}` pattern with `_single` as the entry ID: `POST /api/v1/content/{schema_slug}/_single/publish`. This is consistent with how the UI routes work and avoids adding a third route pattern for singles.

### Modified Existing Endpoints

**`PUT /api/v1/content/{schema_slug}/{entry_id}`** (existing, modified)

- If request body includes `"_status": "draft"` or `"_status": "published"`, that value is used.
- If `_status` is absent from the body, existing status is preserved (current behavior).
- Invalid `_status` values (anything other than `"draft"` or `"published"`) are silently normalized to `"draft"`.

**`POST /api/v1/content/{schema_slug}`** (create, existing, modified)

- If request body includes `"_status": "published"`, the entry is created as published.
- If `_status` is absent, the entry is created as draft (current behavior).

**`PUT /api/v1/content/{schema_slug}/single`** (upsert single, existing, modified)

- Same behavior as the collection PUT: if `_status` is present in the body, respect it; otherwise, preserve existing or default to draft.

No code changes are needed in the API route handlers for these three endpoints. The behavior change comes entirely from the `save_entry` modification in the content module -- `save_entry` now checks for an explicit `_status` in the incoming data before falling back to the disk-read logic.

These modifications allow API-first workflows to manage status inline without needing the dedicated publish/unpublish endpoints.

### API Response: `_status` in List/Get Responses

Currently `_status` is stripped from all API responses by `strip_internal_status`. This is maintained. The publish/unpublish endpoints return status in their own response format (not from the entry data).

No change to the stripping behavior. API consumers who need to know entry status use the `?status=` filter parameter or call the dedicated publish/unpublish endpoints.

**Alternative considered:** Include `_status` in GET responses. Rejected because it changes the API contract for existing consumers who may not expect the field. If needed in the future, a `?include_status=true` parameter can be added.

## Interaction with Publish/Webhook Routes

### Current Behavior (being changed)

```
POST /publish/production    (UI)
POST /api/v1/publish/production    (API)
  1. publish_all_drafts()     <- bulk status flip
  2. cache::rebuild()         <- if any drafts were flipped
  3. fire_webhook()           <- notify external consumers
```

### New Behavior

```
POST /publish/production    (UI)
POST /api/v1/publish/production    (API)
  1. fire_webhook()           <- notify external consumers only
```

The publish route no longer mutates content. It is purely a deployment trigger. This means:

- An editor must publish individual entries before triggering a production deployment.
- Staging deployments with `include_drafts = true` (from the configurable deployments spec) see drafts regardless.
- The "Build Staging" and "Publish Production" buttons in the nav fire webhooks only.

This is the core decoupling this spec achieves.

## Interaction with Deployments (Forward Compatibility)

The configurable deployments spec introduces per-deployment `include_drafts` settings. This spec is compatible:

- `include_drafts = false`: API returns only entries where `_status == "published"` (current default).
- `include_drafts = true`: API returns all entries regardless of status (using `?status=all`).
- Publishing/unpublishing an entry changes what `include_drafts = false` deployments see on next API call.
- No webhook is fired by the publish/unpublish action. Deployments are notified through their own mechanisms (manual fire, auto-deploy cron).

## Interaction with Version History

Publishing and unpublishing do NOT create version history snapshots. Rationale: the entry content is unchanged; only metadata (`_status`) changes. Creating a snapshot for every publish/unpublish would pollute the history with identical content snapshots.

Reverting an entry preserves its current `_status` (existing behavior, unchanged). The `revert_entry` handler in `src/routes/content.rs` calls `save_entry` with `Some(&entry_id)` (update path). With the new `save_entry` logic, if the historical snapshot contains a `_status` field, that value would be respected. This is a subtle behavior change: reverting to an old snapshot that had `_status: "draft"` would set the entry back to draft. This is actually the correct behavior -- if the user wants to revert to an exact historical state, the status should be part of that state. However, old snapshots taken before this feature existed will not have `_status` at all, in which case `save_entry` falls back to the disk-read logic and preserves the current status. Net effect: reverting to old pre-feature snapshots preserves status; reverting to new snapshots restores the status from the snapshot.

## Interaction with Export/Import

No changes. Exported entries retain their `_status`. Imported entries keep whatever `_status` they had in the bundle. The import validation already strips `_`-prefixed keys before schema validation.

## Interaction with Cache

After `set_entry_status` writes to disk, the caller must update the cache via `crate::cache::reload_entry`. The route handlers (both UI and API) are responsible for this (same pattern as existing content CRUD routes in `src/routes/content.rs` and `src/routes/api.rs`).

The file watcher (`src/cache.rs`) will also eventually pick up the disk change, but the explicit `reload_entry` call ensures immediate cache consistency without waiting for the 200ms debounce window.

## Audit Events

Two new audit log actions:

- **`entry_published`** -- actor: user_id (UI) or `"api"` (API), resource_type: `"content"`, resource_id: `"{schema_slug}/{entry_id}"`, details: `null`
- **`entry_unpublished`** -- actor: user_id (UI) or `"api"` (API), resource_type: `"content"`, resource_id: `"{schema_slug}/{entry_id}"`, details: `null`

These events are NOT counted as content mutations for dirty detection. The `is_dirty` method in `src/audit.rs` (line 73) checks for actions `IN ('content_create', 'content_update', 'content_delete', 'schema_create', 'schema_update', 'schema_delete')`. Since `entry_published` and `entry_unpublished` are not in that list, they do not trigger the dirty flag. This is intentional: the deploy webhook should fire because content changed, not because someone toggled a status flag.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Publish/unpublish a nonexistent entry | 404 (UI: redirect with flash error; API: JSON `{"error": "..."}`) |
| Publish an already-published entry | 200 (idempotent, `set_entry_status` writes the same value) |
| Unpublish an already-draft entry | 200 (idempotent, `set_entry_status` writes the same value) |
| Viewer attempts publish/unpublish (UI) | 403 "Insufficient permissions" (from `auth::require_role`) |
| Viewer attempts publish/unpublish (API) | 403 JSON `{"error": "Insufficient permissions"}` (from `require_api_role`) |
| Invalid CSRF token on UI route | 403 "Invalid CSRF token" (from `verify_csrf` middleware, not from handler) |
| Disk write failure in `set_entry_status` | 500 (UI: redirect with flash error; API: JSON `{"error": "..."}`) |
| `set_entry_status` called with invalid status value | Returns `Err` with descriptive message. Route handlers only pass `"draft"` or `"published"` literals, so this is a defense-in-depth check. |
| Schema not found for given slug | 404 (UI: redirect to dashboard; API: 404 status) |
| Entry in single-file collection, but JSON file is corrupt | 500 with serde error details |
| Non-authenticated user hits publish route | Redirect to `/login` (from `require_auth` middleware) |

## Edge Cases

1. **Concurrent publish and save.** User A clicks "Publish" while User B is saving the same entry. Since `set_entry_status` reads the full entry, modifies `_status`, and writes back, a concurrent save could overwrite the status change or vice versa. This is the same file-level race condition that already exists for concurrent saves in `save_entry` and `delete_entry`. Mitigation: none (file-based storage does not support locking). The file watcher's debounce will eventually make the cache consistent with whatever was last written to disk.

2. **Publishing a single-file collection entry.** `set_entry_status` must read the entire JSON array, find the correct entry by `_id`, modify its `_status`, and rewrite the whole file. This is the same read-modify-write pattern used by `save_entry` and `delete_entry` for single-file collections.

3. **Publishing via API while `_status` is in PUT body.** If a client sends `PUT` with `"_status": "published"` and simultaneously calls `POST .../publish`, both paths converge to the same result. No conflict beyond the general concurrent-write race (edge case 1).

4. **Legacy entries without `_status`.** Publishing a legacy entry (no `_status` field) works: `set_entry_status` adds the `_status` field. Unpublishing a legacy entry also works: it adds `"_status": "draft"`. The `get_entry_status` helper treats missing `_status` as `"published"` for display purposes, but `set_entry_status` always writes the field explicitly.

5. **`publish_all_drafts` callers after removal.** Compile-time verification: removing the function from `content/mod.rs` will cause compile errors at all call sites (two: `routes/publish.rs` and `routes/api.rs`), ensuring none are missed. The test `publish_all_drafts_flips_status` also references the function and must be removed.

6. **htmx request with expired CSRF token.** The `verify_csrf` middleware rejects the request with a 403 before the handler runs. Since htmx receives a non-2xx response, it will not swap the DOM. The user sees no change. They can reload the page (which generates a new CSRF token) and try again. This is acceptable behavior and matches how other htmx-powered forms in the app handle CSRF expiry.

7. **Reverting entry with `_status` in snapshot.** As described in the Version History section, reverting to a snapshot that contains `_status` will restore that status value. This is a new behavior but is correct -- the snapshot is a complete record of the entry at that point in time.

## Files Changed

### New

- **`templates/content/_status_control.html`** -- Partial template for the publish/unpublish status control (used both inline and as htmx fragment response)

### Modified

- **`src/content/mod.rs`** -- Add `set_entry_status()`, modify `save_entry()` to respect explicit `_status` in data, remove `publish_all_drafts()` and its test
- **`src/routes/content.rs`** -- Add `publish_entry` and `unpublish_entry` handlers, add two new routes in `routes()` function
- **`src/routes/api.rs`** -- Add `api_publish_entry` and `api_unpublish_entry` handlers, add two new routes in `routes()` function, remove `publish_all_drafts` call from `publish` handler
- **`src/routes/publish.rs`** -- Remove `publish_all_drafts()` call and cache rebuild from the `publish` handler
- **`templates/content/edit.html`** -- Replace static draft badge with `{% include "content/_status_control.html" %}`

### Not Changed

- **`src/content/form.rs`** -- Form generation does not need to know about `_status`
- **`src/sync/mod.rs`** -- Export/import behavior unchanged
- **`src/config.rs`** -- No new configuration
- **`src/state.rs`** -- No state changes
- **`src/webhooks.rs`** -- Webhook firing logic unchanged
- **`src/audit.rs`** -- Audit logging interface unchanged (uses existing `log()` method with new action strings)
- **`src/cache.rs`** -- Cache update functions unchanged (handlers call existing `reload_entry`)
- **`src/history.rs`** -- Version history unchanged (publish/unpublish does not snapshot)
- **`templates/content/list.html`** -- Draft badge already works, no changes
- **`templates/content/history.html`** -- No changes

## Implementation Order

1. **Add `set_entry_status` to `src/content/mod.rs`** with unit tests. This is the core function with no dependencies on other changes.
2. **Modify `save_entry` in `src/content/mod.rs`** to respect explicit `_status`. Add unit tests for the new behavior.
3. **Create `templates/content/_status_control.html`** partial template.
4. **Update `templates/content/edit.html`** to use the partial instead of the static badge.
5. **Add UI route handlers** (`publish_entry`, `unpublish_entry`) to `src/routes/content.rs` and register routes.
6. **Add API route handlers** (`api_publish_entry`, `api_unpublish_entry`) to `src/routes/api.rs` and register routes.
7. **Remove `publish_all_drafts`** from `src/content/mod.rs`, update `src/routes/publish.rs` and `src/routes/api.rs` publish handlers.
8. **Add integration tests.**

Steps 1-2 can be committed independently. Steps 3-4 together. Steps 5-6 independently. Step 7 last (once all new routes are confirmed working). This ordering ensures the codebase compiles at every step.

## Testing

### Unit Tests (in `src/content/mod.rs`)

- `set_entry_status` on directory-mode entry: verify status changes on disk
- `set_entry_status` on single-file collection entry: verify only the target entry's status changes, other entries untouched
- `set_entry_status` on single-file single entry: verify status changes
- `set_entry_status` on nonexistent entry (directory mode): returns Err
- `set_entry_status` on nonexistent entry (single-file collection, entry not in array): returns Err
- `set_entry_status` on nonexistent file (single-file mode, file missing): returns Err
- `set_entry_status` with invalid status value (`"archived"`): returns Err
- `set_entry_status` on legacy entry without `_status`: adds the field
- `set_entry_status` idempotent: publishing a published entry succeeds (no error)
- `save_entry` with explicit `_status: "published"` in data: creates entry as published
- `save_entry` with explicit `_status: "draft"` in data on update: overrides existing published status
- `save_entry` with explicit invalid `_status: "archived"` in data: normalizes to draft
- `save_entry` without `_status` in data (create path): defaults to draft (unchanged behavior)
- `save_entry` without `_status` in data (update path): preserves existing status from disk (unchanged behavior)
- Removal of `publish_all_drafts`: compile-time verification (function and test no longer exist)

### Integration Tests

- Create entry (draft) -> publish via UI route (POST with CSRF) -> verify entry status is published on disk
- Create entry (draft) -> publish via API (POST with bearer token) -> verify `200 {"status": "published"}`
- Publish entry -> unpublish via UI -> verify entry status is draft on disk
- Publish entry -> unpublish via API -> verify `200 {"status": "draft"}`
- Publish already-published entry via API: 200 idempotent
- Unpublish already-draft entry via API: 200 idempotent
- Viewer token attempts API publish: 403
- API PUT with `_status: "published"`: entry saved as published, verify on disk
- API POST create with `_status: "published"`: entry created as published, verify on disk
- API PUT without `_status`: existing published status preserved
- API PUT single (upsert) with `_status: "published"`: single entry saved as published
- Webhook publish route (`POST /api/v1/publish/production`) no longer flips drafts: create draft, fire webhook, verify entry still draft on disk
- Single schema: publish `_single` entry via UI route works
- Single schema: publish `_single` entry via API route works
- htmx publish request (with `HX-Request: true` header) returns HTML fragment containing "Published" badge (not a redirect)
- Nonexistent entry publish via API: 404
- Nonexistent schema publish via API: 404
