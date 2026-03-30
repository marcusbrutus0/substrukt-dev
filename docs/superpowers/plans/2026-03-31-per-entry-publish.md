# Per-Entry Draft/Publish Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple publish/unpublish from deployments so each content entry can be individually published or unpublished via dedicated UI buttons and API endpoints.

**Architecture:** Add a `set_entry_status` function that writes `_status` directly to disk without going through `save_entry`. Add dedicated `POST .../publish` and `POST .../unpublish` routes for both UI and API. Modify `save_entry` to respect an explicit `_status` in incoming data (for API clients). Remove `publish_all_drafts` — the publish/webhook routes become thin wrappers that only fire webhooks. The UI gets an htmx-powered inline status control on the edit page.

**Tech Stack:** Rust, Axum, serde_json, minijinja templates, htmx, axum-htmx

**Spec:** `docs/superpowers/specs/2026-03-31-per-entry-publish-design.md`

---

## Prerequisites

**Discard uncommitted changes before starting.** The working tree has uncommitted modifications to `src/routes/api.rs` and `src/routes/publish.rs` (WIP draft-flip changes). These must be discarded to start from a clean committed state:

```bash
eval "$(direnv export bash 2>/dev/null)" && git checkout src/routes/api.rs src/routes/publish.rs
```

**Current committed state:**
- `src/routes/publish.rs` (UI): Does NOT call `publish_all_drafts` — already clean.
- `src/routes/api.rs` (API): Calls `publish_all_drafts` only for `"production"` environment (line 785, guarded by `if environment == "production"`).

All line references in this plan refer to the committed (HEAD) state.

---

## File Map

- **Create:** `templates/content/_status_control.html` — Partial template for inline publish/unpublish control (used both on page load and as htmx fragment response)
- **Modify:** `src/content/mod.rs` — Add `set_entry_status()`, modify `save_entry()` to respect explicit `_status`, remove `publish_all_drafts()` and its test
- **Modify:** `src/routes/content.rs` — Add `publish_entry` and `unpublish_entry` handlers, register two new routes
- **Modify:** `src/routes/api.rs` — Add `api_publish_entry` and `api_unpublish_entry` handlers, register two new routes, remove `publish_all_drafts` call from `publish` handler
- **Modify:** `templates/content/edit.html` — Replace static draft badge with `{% include "content/_status_control.html" %}`
- **Modify:** `tests/integration.rs` — Add integration tests for per-entry publish/unpublish (UI and API)

---

### Task 1: Add `set_entry_status` to content module

**Files:**
- Modify: `src/content/mod.rs` (add function after `delete_entry` around line 223, add tests in `mod tests` block)

**Depends on:** Nothing (standalone core function)

- [ ] **Step 1: Write failing unit tests for `set_entry_status`**

Add these tests inside the existing `#[cfg(test)] mod tests` block at the end of `src/content/mod.rs` (after the `get_entry_status_returns_correct_status` test around line 636):

```rust
    #[test]
    fn set_entry_status_directory_mode() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let data = json!({"title": "Hello"});
        let id = save_entry(tmp.path(), &schema, None, data).unwrap();

        // Starts as draft
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "draft");

        // Publish it
        set_entry_status(tmp.path(), &schema, &id, "published").unwrap();
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "published");
        // Content untouched
        assert_eq!(entry.data.get("title").and_then(|v| v.as_str()), Some("Hello"));

        // Unpublish it
        set_entry_status(tmp.path(), &schema, &id, "draft").unwrap();
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "draft");
    }

    #[test]
    fn set_entry_status_single_file_single() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Single, StorageMode::SingleFile);
        save_entry(tmp.path(), &schema, Some("_single"), json!({"title": "Settings"})).unwrap();

        set_entry_status(tmp.path(), &schema, "_single", "published").unwrap();
        let entry = get_entry(tmp.path(), &schema, "_single").unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "published");
    }

    #[test]
    fn set_entry_status_single_file_collection() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::SingleFile);
        let id_a = save_entry(tmp.path(), &schema, None, json!({"title": "A"})).unwrap();
        let id_b = save_entry(tmp.path(), &schema, None, json!({"title": "B"})).unwrap();

        // Publish only entry A
        set_entry_status(tmp.path(), &schema, &id_a, "published").unwrap();

        let entry_a = get_entry(tmp.path(), &schema, &id_a).unwrap().unwrap();
        let entry_b = get_entry(tmp.path(), &schema, &id_b).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry_a.data), "published");
        assert_eq!(get_entry_status(&entry_b.data), "draft", "other entry should be untouched");
    }

    #[test]
    fn set_entry_status_nonexistent_directory() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let result = set_entry_status(tmp.path(), &schema, "nonexistent", "published");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn set_entry_status_nonexistent_single_file_collection() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::SingleFile);
        // Create one entry so the file exists
        save_entry(tmp.path(), &schema, None, json!({"title": "A"})).unwrap();

        let result = set_entry_status(tmp.path(), &schema, "nonexistent", "published");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn set_entry_status_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Single, StorageMode::SingleFile);
        // File does not exist at all
        let result = set_entry_status(tmp.path(), &schema, "_single", "published");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn set_entry_status_invalid_status() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        save_entry(tmp.path(), &schema, None, json!({"title": "Hello"})).unwrap();

        let result = set_entry_status(tmp.path(), &schema, "hello", "archived");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid status"));
    }

    #[test]
    fn set_entry_status_idempotent() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let id = save_entry(tmp.path(), &schema, None, json!({"title": "Hello"})).unwrap();

        // Entry starts as draft — unpublishing again should succeed (idempotent)
        set_entry_status(tmp.path(), &schema, &id, "draft").unwrap();
        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "draft");
    }

    #[test]
    fn set_entry_status_adds_field_to_legacy_entry() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        // Write a legacy entry with no _status field
        let dir = tmp.path().join("test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("legacy.json"), r#"{"title": "Old"}"#).unwrap();

        set_entry_status(tmp.path(), &schema, "legacy", "published").unwrap();
        let entry = get_entry(tmp.path(), &schema, "legacy").unwrap().unwrap();
        assert_eq!(get_entry_status(&entry.data), "published");
        assert_eq!(entry.data.get("title").and_then(|v| v.as_str()), Some("Old"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test set_entry_status -- --nocapture`

Expected: compilation error — `set_entry_status` function does not exist.

- [ ] **Step 3: Implement `set_entry_status`**

Add the function in `src/content/mod.rs` after the `delete_entry` function (after line 223, before `matches_query`):

```rust
/// Set the _status of an entry without modifying its content.
/// Does not create a history snapshot (metadata-only change).
pub fn set_entry_status(
    content_dir: &Path,
    schema: &SchemaFile,
    entry_id: &str,
    status: &str,
) -> eyre::Result<()> {
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
                    let matches = e
                        .get("_id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s == entry_id);
                    if matches {
                        if let Some(obj) = e.as_object_mut() {
                            obj.insert(
                                "_status".to_string(),
                                Value::String(status.to_string()),
                            );
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

- [ ] **Step 4: Run the tests to verify they pass**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test set_entry_status -- --nocapture`

Expected: all 8 new tests pass.

- [ ] **Step 5: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/content/mod.rs && git commit -m "feat: add set_entry_status for per-entry publish/unpublish"
```

---

### Task 2: Modify `save_entry` to respect explicit `_status` in incoming data

**Files:**
- Modify: `src/content/mod.rs:114-129` (the status determination block in `save_entry`), add tests in `mod tests`

**Depends on:** Task 1 (tests reference `set_entry_status` for setup)

- [ ] **Step 1: Write failing unit tests for the new `save_entry` behavior**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `src/content/mod.rs`:

```rust
    #[test]
    fn save_entry_explicit_status_published_on_create() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let data = json!({"title": "Hello", "_status": "published"});
        let id = save_entry(tmp.path(), &schema, None, data).unwrap();

        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(
            entry.data.get("_status").and_then(|v| v.as_str()),
            Some("published"),
            "explicit _status in data should be respected on create"
        );
    }

    #[test]
    fn save_entry_explicit_status_draft_on_update() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let id = save_entry(tmp.path(), &schema, None, json!({"title": "Hello"})).unwrap();

        // Publish the entry via set_entry_status
        set_entry_status(tmp.path(), &schema, &id, "published").unwrap();

        // Update with explicit _status: "draft" — should override existing published status
        let data = json!({"title": "Updated", "_status": "draft"});
        save_entry(tmp.path(), &schema, Some(&id), data).unwrap();

        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(
            entry.data.get("_status").and_then(|v| v.as_str()),
            Some("draft"),
            "explicit _status: draft should override existing published"
        );
    }

    #[test]
    fn save_entry_explicit_invalid_status_falls_back_to_draft() {
        let tmp = TempDir::new().unwrap();
        let schema = test_schema(Kind::Collection, StorageMode::Directory);
        let data = json!({"title": "Hello", "_status": "archived"});
        let id = save_entry(tmp.path(), &schema, None, data).unwrap();

        let entry = get_entry(tmp.path(), &schema, &id).unwrap().unwrap();
        assert_eq!(
            entry.data.get("_status").and_then(|v| v.as_str()),
            Some("draft"),
            "invalid _status value should normalize to draft"
        );
    }
```

- [ ] **Step 2: Run the tests to verify the new tests fail**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test save_entry_explicit -- --nocapture`

Expected: `save_entry_explicit_status_published_on_create` fails (status will be "draft" because current code ignores `_status` in incoming data). The other two may or may not fail depending on order of checks.

- [ ] **Step 3: Modify `save_entry` to check for explicit `_status`**

In `src/content/mod.rs`, replace lines 114-129 (the status determination block):

```rust
    // Determine _status: draft for new entries, preserve existing for updates
    let status = if let Some(eid) = entry_id {
        // Update path: try to read existing _status
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

With:

```rust
    // Determine _status: respect explicit value, else preserve existing, else draft
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

- [ ] **Step 4: Run all content tests to verify everything passes**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib content::tests -- --nocapture`

Expected: all tests pass, including the 3 new tests and all existing tests (existing tests for `save_entry_create_injects_draft_status`, `save_entry_update_preserves_status`, `save_entry_update_no_existing_falls_back_to_draft` should still pass since those tests don't include `_status` in the input data).

- [ ] **Step 5: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/content/mod.rs && git commit -m "feat: save_entry respects explicit _status in incoming data"
```

**Note on `_status` and schema validation:** The API route handlers (`create_entry`, `update_entry`, `upsert_single` in `src/routes/api.rs`) call `content::validate_content(&schema_file, &data)` BEFORE passing data to `save_entry`. If an API client sends `_status` in the JSON body, and the schema has `"additionalProperties": false`, validation will reject the request before `save_entry` ever sees the `_status` field. This is acceptable behavior: schemas that disallow additional properties should use the dedicated `POST .../publish` endpoint instead of inline `_status`. Most schemas do NOT set `additionalProperties: false` (JSON Schema defaults to allowing additional properties), so inline `_status` works in the common case. The import path (`src/sync/mod.rs`) already strips `_`-prefixed keys before validation for the same reason.

---

### Task 3: Create status control partial template and update edit page

**Files:**
- Create: `templates/content/_status_control.html`
- Modify: `templates/content/edit.html:7` (replace static draft badge)

**Depends on:** Nothing (template-only, no Rust changes)

- [ ] **Step 1: Create the status control partial template**

Create the file `templates/content/_status_control.html`:

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

- [ ] **Step 2: Update the edit template to use the partial**

In `templates/content/edit.html`, replace line 7:

```html
    {% if entry_status == "draft" %}<span class="ml-2 px-2 py-0.5 rounded text-xs font-medium bg-accent-soft text-accent align-middle">Draft</span>{% endif %}
```

With:

```html
    {% if not is_new %}{% include "content/_status_control.html" %}{% endif %}
```

- [ ] **Step 3: Verify the build compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo build`

Expected: compiles successfully. (The template is only checked at runtime in debug mode, but this confirms no Rust compilation issues.)

- [ ] **Step 4: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add templates/content/_status_control.html templates/content/edit.html && git commit -m "feat: add inline publish/unpublish status control on edit page"
```

---

### Task 4: Add UI route handlers for publish/unpublish

**Files:**
- Modify: `src/routes/content.rs` — Add `publish_entry` and `unpublish_entry` handlers, register routes

**Depends on:** Task 1 (`set_entry_status`), Task 3 (template partial)

- [ ] **Step 1: Add the two route registrations in `content::routes()`**

In `src/routes/content.rs`, modify the `routes()` function (lines 25-39). Add the publish/unpublish routes **before** the existing `/{schema_slug}/{entry_id}` route. Replace the entire `routes()` function:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{schema_slug}", get(list_entries))
        .route("/{schema_slug}/new", get(new_entry_page).post(create_entry))
        .route("/{schema_slug}/{entry_id}/edit", get(edit_entry_page))
        .route(
            "/{schema_slug}/{entry_id}/publish",
            axum::routing::post(publish_entry),
        )
        .route(
            "/{schema_slug}/{entry_id}/unpublish",
            axum::routing::post(unpublish_entry),
        )
        .route(
            "/{schema_slug}/{entry_id}",
            axum::routing::post(update_entry).delete(delete_entry),
        )
        .route("/{schema_slug}/{entry_id}/history", get(entry_history))
        .route(
            "/{schema_slug}/{entry_id}/revert/{timestamp}",
            axum::routing::post(revert_entry),
        )
}
```

- [ ] **Step 2: Add the `publish_entry` handler**

Add this function in `src/routes/content.rs` after the `delete_entry` handler (after line 597, before the `UploadField` struct):

```rust
async fn publish_entry(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if auth::require_role(&session, "editor").await.is_err() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Insufficient permissions",
        )
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
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
        "published",
    ) {
        tracing::error!("Publish failed: {e}");
        auth::set_flash(&session, "error", "Failed to publish entry").await;
        return Redirect::to(&format!("/content/{schema_slug}/{entry_id}/edit")).into_response();
    }

    crate::cache::reload_entry(
        &state.cache,
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
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
        let tmpl = state
            .templates
            .acquire_env()
            .map_err(|e| format!("Template env error: {e}"))
            .unwrap();
        let template = tmpl
            .get_template("content/_status_control.html")
            .map_err(|e| format!("Template error: {e}"))
            .unwrap();
        let html = template
            .render(minijinja::context! {
                csrf_token => csrf_token,
                user_role => user_role,
                schema_slug => schema_slug,
                entry_id => entry_id,
                entry_status => "published",
            })
            .map_err(|e| format!("Render error: {e}"))
            .unwrap();
        return Html(html).into_response();
    }

    auth::set_flash(&session, "success", "Entry published").await;
    Redirect::to(&format!("/content/{schema_slug}/{entry_id}/edit")).into_response()
}
```

- [ ] **Step 3: Add the `unpublish_entry` handler**

Add this function directly after `publish_entry`:

```rust
async fn unpublish_entry(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if auth::require_role(&session, "editor").await.is_err() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Insufficient permissions",
        )
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
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
        "draft",
    ) {
        tracing::error!("Unpublish failed: {e}");
        auth::set_flash(&session, "error", "Failed to unpublish entry").await;
        return Redirect::to(&format!("/content/{schema_slug}/{entry_id}/edit")).into_response();
    }

    crate::cache::reload_entry(
        &state.cache,
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
    );

    let user_id = auth::current_user_id(&session).await.unwrap_or(0);
    state.audit.log(
        &user_id.to_string(),
        "entry_unpublished",
        "content",
        &format!("{schema_slug}/{entry_id}"),
        None,
    );

    if is_htmx {
        let csrf_token = auth::ensure_csrf_token(&session).await;
        let user_role = auth::current_user_role(&session).await.unwrap_or_default();
        let tmpl = state
            .templates
            .acquire_env()
            .map_err(|e| format!("Template env error: {e}"))
            .unwrap();
        let template = tmpl
            .get_template("content/_status_control.html")
            .map_err(|e| format!("Template error: {e}"))
            .unwrap();
        let html = template
            .render(minijinja::context! {
                csrf_token => csrf_token,
                user_role => user_role,
                schema_slug => schema_slug,
                entry_id => entry_id,
                entry_status => "draft",
            })
            .map_err(|e| format!("Render error: {e}"))
            .unwrap();
        return Html(html).into_response();
    }

    auth::set_flash(&session, "success", "Entry unpublished").await;
    Redirect::to(&format!("/content/{schema_slug}/{entry_id}/edit")).into_response()
}
```

- [ ] **Step 4: Verify the build compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo build`

Expected: compiles successfully.

- [ ] **Step 5: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/routes/content.rs && git commit -m "feat: add UI publish/unpublish route handlers for content entries"
```

---

### Task 5: Add API route handlers for publish/unpublish

**Files:**
- Modify: `src/routes/api.rs` — Add `api_publish_entry` and `api_unpublish_entry` handlers, register routes

**Depends on:** Task 1 (`set_entry_status`)

- [ ] **Step 1: Add the two route registrations in `api::routes()`**

In `src/routes/api.rs`, modify the `routes()` function (lines 24-46). Add the publish/unpublish routes **before** the existing `/content/{schema_slug}/{entry_id}` route. Replace the entire `routes()` function:

```rust
pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/schemas", get(list_schemas))
        .route("/schemas/{slug}", get(get_schema))
        .route(
            "/content/{schema_slug}",
            get(list_entries).post(create_entry),
        )
        .route(
            "/content/{schema_slug}/single",
            get(get_single).put(upsert_single).delete(delete_single),
        )
        .route(
            "/content/{schema_slug}/{entry_id}/publish",
            post(api_publish_entry),
        )
        .route(
            "/content/{schema_slug}/{entry_id}/unpublish",
            post(api_unpublish_entry),
        )
        .route(
            "/content/{schema_slug}/{entry_id}",
            get(get_entry).put(update_entry).delete(delete_entry),
        )
        .route("/uploads", post(upload_file))
        .route("/uploads/{hash}", get(get_upload))
        .route("/export", post(export_bundle))
        .route("/import", post(import_bundle))
        .route("/publish/{environment}", post(publish))
        .layer(middleware::from_fn_with_state(state, api_rate_limit))
}
```

- [ ] **Step 2: Add the `api_publish_entry` handler**

Add this function in `src/routes/api.rs` after the `delete_single` function (after line 597, before `upload_file`):

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
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if let Err(e) = content::set_entry_status(
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
        "published",
    ) {
        let msg = e.to_string();
        if msg.contains("not found") {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response();
    }

    crate::cache::reload_entry(
        &state.cache,
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
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

- [ ] **Step 3: Add the `api_unpublish_entry` handler**

Add this function directly after `api_publish_entry`:

```rust
async fn api_unpublish_entry(
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
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if let Err(e) = content::set_entry_status(
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
        "draft",
    ) {
        let msg = e.to_string();
        if msg.contains("not found") {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response();
    }

    crate::cache::reload_entry(
        &state.cache,
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
    );

    state.audit.log(
        "api",
        "entry_unpublished",
        "content",
        &format!("{schema_slug}/{entry_id}"),
        None,
    );

    Json(serde_json::json!({"status": "draft", "entry_id": entry_id})).into_response()
}
```

- [ ] **Step 4: Verify the build compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo build`

Expected: compiles successfully.

- [ ] **Step 5: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/routes/api.rs && git commit -m "feat: add API publish/unpublish endpoints for content entries"
```

---

### Task 6: Remove `publish_all_drafts` and decouple API publish route from status mutation

**Files:**
- Modify: `src/content/mod.rs` — Remove `publish_all_drafts()` function and its test
- Modify: `src/routes/api.rs` — Remove `publish_all_drafts` call and cache rebuild from `publish` handler

Note: `src/routes/publish.rs` (UI publish route) does NOT call `publish_all_drafts` in the committed codebase — no change needed there.

**Depends on:** Tasks 4, 5 (new publish routes must exist before removing the old bulk mechanism)

- [ ] **Step 1: Remove `publish_all_drafts` function from `src/content/mod.rs`**

Delete the entire `publish_all_drafts` function (lines 272-339 in `src/content/mod.rs`):

```rust
/// Flip all draft entries to published across all schemas. Returns count of entries published.
/// Bypasses save_entry to avoid validation/snapshot overhead (metadata-only change).
pub fn publish_all_drafts(schemas_dir: &Path, content_dir: &Path) -> eyre::Result<usize> {
    ...
}
```

Remove this entire block (from the `/// Flip all draft` doc comment through the closing `}`).

- [ ] **Step 2: Remove the `publish_all_drafts_flips_status` test**

In the `#[cfg(test)] mod tests` block of `src/content/mod.rs`, delete the entire `publish_all_drafts_flips_status` test (lines 581-625):

```rust
    #[test]
    fn publish_all_drafts_flips_status() {
        ...
    }
```

Remove this entire block (from `#[test]` through the closing `}`).

- [ ] **Step 3: Update `src/routes/api.rs` — remove `publish_all_drafts` call from `publish` handler**

In `src/routes/api.rs`, replace the `publish` function (the handler for `POST /api/v1/publish/{environment}`) with this version that only fires the webhook:

```rust
async fn publish(
    State(state): State<AppState>,
    token: BearerToken,
    Path(environment): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    if !matches!(environment.as_str(), "staging" | "production") {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Unknown environment"})),
        )
            .into_response();
    }

    match crate::webhooks::fire_webhook(
        &state.http_client,
        &state.audit,
        &state.config,
        &environment,
        crate::webhooks::TriggerSource::Manual,
    )
    .await
    {
        Ok(true) => Json(serde_json::json!({"status": "triggered"})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Webhook URL not configured"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
```

- [ ] **Step 4: Verify the build compiles and all existing tests pass**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo build && cargo test`

Expected: compiles and all tests pass. The compiler will catch any remaining references to `publish_all_drafts`.

- [ ] **Step 5: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/content/mod.rs src/routes/api.rs && git commit -m "feat: remove publish_all_drafts, decouple publish routes from status mutation"
```

---

### Task 7: Integration tests for per-entry publish/unpublish

**Files:**
- Modify: `tests/integration.rs` — Add integration tests

**Depends on:** Tasks 1-6 (all implementation complete)

- [ ] **Step 1: Add integration tests for API publish/unpublish**

Add these tests at the end of `tests/integration.rs` (after the last test, around line 3047). Add a section comment first:

```rust
// ── Per-Entry Publish/Unpublish tests ──────────────────────────

#[tokio::test]
async fn api_publish_entry() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    let token = s.create_api_token("publish-test").await;

    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    // Create schema
    let schema_json = r#"{
        "x-substrukt": {"title": "Pub Test", "slug": "pub-test", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    // Create entry via API (starts as draft)
    let resp = api
        .post(s.url("/api/v1/content/pub-test"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Draft Post"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let entry_id = body["id"].as_str().unwrap().to_string();

    // Entry is draft — default list (published only) should be empty
    let resp = api
        .get(s.url("/api/v1/content/pub-test"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 0, "draft entry should not appear in published-only list");

    // Publish the entry
    let resp = api
        .post(s.url(&format!("/api/v1/content/pub-test/{entry_id}/publish")))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "published");
    assert_eq!(body["entry_id"], entry_id);

    // Entry now visible in default list
    let resp = api
        .get(s.url("/api/v1/content/pub-test"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 1, "published entry should appear in default list");

    // Unpublish the entry
    let resp = api
        .post(s.url(&format!("/api/v1/content/pub-test/{entry_id}/unpublish")))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "draft");

    // Entry no longer in default list
    let resp = api
        .get(s.url("/api/v1/content/pub-test"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 0, "unpublished entry should not appear in default list");
}

#[tokio::test]
async fn api_publish_idempotent() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    let token = s.create_api_token("idempotent-test").await;

    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    let schema_json = r#"{
        "x-substrukt": {"title": "Idemp Test", "slug": "idemp-test", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    let resp = api
        .post(s.url("/api/v1/content/idemp-test"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Hello"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let entry_id = body["id"].as_str().unwrap().to_string();

    // Unpublish a draft (idempotent) — should succeed
    let resp = api
        .post(s.url(&format!("/api/v1/content/idemp-test/{entry_id}/unpublish")))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Publish twice (idempotent) — should succeed
    api.post(s.url(&format!("/api/v1/content/idemp-test/{entry_id}/publish")))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let resp = api
        .post(s.url(&format!("/api/v1/content/idemp-test/{entry_id}/publish")))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "published");
}

#[tokio::test]
async fn api_publish_unauthenticated() {
    let s = TestServer::start().await;
    s.setup_admin().await;

    let schema_json = r#"{
        "x-substrukt": {"title": "Auth Test", "slug": "auth-test", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    let admin_token = s.create_api_token("admin-token").await;
    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    // Create entry as admin
    let resp = api
        .post(s.url("/api/v1/content/auth-test"))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({"title": "Test"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let entry_id = body["id"].as_str().unwrap().to_string();

    // Unauthenticated request (no bearer token) — should fail with 401
    let resp = api
        .post(s.url(&format!("/api/v1/content/auth-test/{entry_id}/publish")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Request with invalid bearer token — should also fail with 401
    let resp = api
        .post(s.url(&format!("/api/v1/content/auth-test/{entry_id}/publish")))
        .bearer_auth("invalid-token-value")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_publish_nonexistent_entry() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    let token = s.create_api_token("notfound-test").await;

    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    let schema_json = r#"{
        "x-substrukt": {"title": "NF Test", "slug": "nf-test", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    let resp = api
        .post(s.url("/api/v1/content/nf-test/nonexistent/publish"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_publish_nonexistent_schema() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    let token = s.create_api_token("noschema-test").await;

    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    let resp = api
        .post(s.url("/api/v1/content/nonexistent-schema/some-id/publish"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_put_with_explicit_status() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    let token = s.create_api_token("put-status-test").await;

    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    let schema_json = r#"{
        "x-substrukt": {"title": "Put Status", "slug": "put-status", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    // Create with explicit _status: "published"
    let resp = api
        .post(s.url("/api/v1/content/put-status"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Published", "_status": "published"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let entry_id = body["id"].as_str().unwrap().to_string();

    // Default list should include it (it's published)
    let resp = api
        .get(s.url("/api/v1/content/put-status"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 1, "entry created as published should appear in default list");

    // Update without _status — should preserve published
    let resp = api
        .put(s.url(&format!("/api/v1/content/put-status/{entry_id}")))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Updated"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = api
        .get(s.url("/api/v1/content/put-status"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 1, "published status preserved after update without explicit _status");

    // Update with explicit _status: "draft" — should change to draft
    let resp = api
        .put(s.url(&format!("/api/v1/content/put-status/{entry_id}")))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Now Draft", "_status": "draft"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = api
        .get(s.url("/api/v1/content/put-status"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 0, "entry with _status: draft should not appear in default list");
}

#[tokio::test]
async fn api_publish_single_entry() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    let token = s.create_api_token("single-pub-test").await;

    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    let schema_json = r#"{
        "x-substrukt": {"title": "Single Pub", "slug": "single-pub", "storage": "single-file", "kind": "single"},
        "type": "object",
        "properties": {"site_name": {"type": "string"}},
        "required": ["site_name"]
    }"#;
    s.create_schema(schema_json).await;

    // Upsert single (starts as draft)
    let resp = api
        .put(s.url("/api/v1/content/single-pub/single"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"site_name": "My Site"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Default GET returns 404 (draft)
    let resp = api
        .get(s.url("/api/v1/content/single-pub/single"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Publish via dedicated endpoint
    let resp = api
        .post(s.url("/api/v1/content/single-pub/_single/publish"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "published");

    // Default GET now returns 200
    let resp = api
        .get(s.url("/api/v1/content/single-pub/single"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Unpublish
    let resp = api
        .post(s.url("/api/v1/content/single-pub/_single/unpublish"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "draft");
}

#[tokio::test]
async fn webhook_publish_no_longer_flips_drafts() {
    // Start with a mock webhook server
    let webhook_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let webhook_addr = webhook_listener.local_addr().unwrap();
    let webhook_url = format!("http://{webhook_addr}/hook");

    // Spawn a simple handler that accepts POST and returns 200
    // Note: use "ok" string response, not StatusCode::OK — the imported StatusCode
    // is reqwest::StatusCode which doesn't implement axum's IntoResponse.
    tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(|| async { "ok" }),
        );
        axum::serve(webhook_listener, app).await.unwrap();
    });

    let s = TestServer::start_with_webhooks(None, Some(webhook_url)).await;
    s.setup_admin().await;
    let token = s.create_api_token("webhook-test").await;

    let api = Client::builder()
        .cookie_store(false)
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    let schema_json = r#"{
        "x-substrukt": {"title": "WH Test", "slug": "wh-test", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    // Create a draft entry
    let resp = api
        .post(s.url("/api/v1/content/wh-test"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Draft Post"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Fire production publish webhook
    let resp = api
        .post(s.url("/api/v1/publish/production"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Wait briefly for any async effects
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Entry should STILL be draft (publish no longer flips drafts)
    let resp = api
        .get(s.url("/api/v1/content/wh-test?status=all"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 1);

    let resp = api
        .get(s.url("/api/v1/content/wh-test"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(entries.len(), 0, "draft entry should NOT be flipped to published by webhook");
}

#[tokio::test]
async fn ui_publish_entry_via_form() {
    let s = TestServer::start().await;
    s.setup_admin().await;

    let schema_json = r#"{
        "x-substrukt": {"title": "UI Pub Test", "slug": "ui-pub-test", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    // Create an entry via the UI
    let csrf = s.get_csrf("/content/ui-pub-test/new").await;
    let resp = s
        .client
        .post(s.url("/content/ui-pub-test/new"))
        .multipart(
            reqwest::multipart::Form::new()
                .text("title", "Test Post")
                .text("_csrf", csrf.clone()),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // Find the entry ID from the list page
    let resp = s
        .client
        .get(s.url("/content/ui-pub-test"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("Draft"), "entry should show Draft badge on list");

    // Load the edit page to get entry_id and CSRF
    // The entry ID is "test-post" (slugified from "Test Post")
    let resp = s
        .client
        .get(s.url("/content/ui-pub-test/test-post/edit"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("Publish"), "edit page should show Publish button for draft entry");
    let csrf = extract_csrf_token(&body).unwrap();

    // Publish via UI POST (non-htmx — should redirect)
    let resp = s
        .client
        .post(s.url("/content/ui-pub-test/test-post/publish"))
        .form(&[("_csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER, "non-htmx publish should redirect");

    // Edit page should now show Published + Unpublish button
    let resp = s
        .client
        .get(s.url("/content/ui-pub-test/test-post/edit"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("Published"), "edit page should show Published badge");
    assert!(body.contains("Unpublish"), "edit page should show Unpublish button");

    // Unpublish via UI POST
    let csrf = extract_csrf_token(&body).unwrap();
    let resp = s
        .client
        .post(s.url("/content/ui-pub-test/test-post/unpublish"))
        .form(&[("_csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // Edit page should show Draft again
    let resp = s
        .client
        .get(s.url("/content/ui-pub-test/test-post/edit"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("Draft"), "edit page should show Draft badge after unpublish");
}

#[tokio::test]
async fn ui_htmx_publish_returns_fragment() {
    let s = TestServer::start().await;
    s.setup_admin().await;

    let schema_json = r#"{
        "x-substrukt": {"title": "HTMX Test", "slug": "htmx-test", "storage": "directory"},
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"]
    }"#;
    s.create_schema(schema_json).await;

    // Create entry
    let csrf = s.get_csrf("/content/htmx-test/new").await;
    s.client
        .post(s.url("/content/htmx-test/new"))
        .multipart(
            reqwest::multipart::Form::new()
                .text("title", "HTMX Post")
                .text("_csrf", csrf),
        )
        .send()
        .await
        .unwrap();

    // Get CSRF from edit page
    let resp = s
        .client
        .get(s.url("/content/htmx-test/htmx-post/edit"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    let csrf = extract_csrf_token(&body).unwrap();

    // Publish with HX-Request header — should get HTML fragment, not redirect
    let resp = s
        .client
        .post(s.url("/content/htmx-test/htmx-post/publish"))
        .header("HX-Request", "true")
        .form(&[("_csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "htmx request should return 200");
    let body = resp.text().await.unwrap();
    assert!(body.contains("Published"), "htmx response should contain Published badge");
    assert!(body.contains("Unpublish"), "htmx response should contain Unpublish button");
    assert!(body.contains("entry-status"), "htmx response should contain entry-status span");
}
```

**Test coverage notes:**

- **Viewer API test replaced with unauthenticated test:** The spec calls for testing "viewer token attempts API publish: 403". However, viewers cannot create API tokens (the `create_token` handler requires editor role). Since viewer API tokens are impossible to create through the application, this scenario cannot occur in practice. The `api_publish_unauthenticated` test instead verifies the more practical boundary: requests without a valid bearer token are rejected (401). The `require_api_role` function's role-level check is already exercised by the existing RBAC test infrastructure.
- **Single PUT with explicit `_status` not separately tested:** The spec mentions "API PUT single (upsert) with `_status: published`". This uses the same `save_entry` code path as the collection PUT test (`api_put_with_explicit_status`). A separate test would add coverage of the upsert_single handler specifically, but the behavior is fully determined by `save_entry`, which is already tested in both unit and integration tests.
- **Single publish via UI route not separately tested:** The `api_publish_single_entry` test covers singles via the API. A UI test for publishing `_single` would follow the same pattern as `ui_publish_entry_via_form` but for a single schema. The status control template works identically for singles (same `entry_id` = `_single` pattern).

- [ ] **Step 2: Run the integration tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test api_publish_entry api_publish_idempotent api_publish_unauthenticated api_publish_nonexistent_entry api_publish_nonexistent_schema api_put_with_explicit_status api_publish_single_entry webhook_publish_no_longer_flips_drafts ui_publish_entry_via_form ui_htmx_publish_returns_fragment -- --nocapture`

Expected: all 10 new tests pass.

- [ ] **Step 3: Run the full test suite**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`

Expected: all tests pass (existing + new). Check specifically that these existing tests that relied on `publish_all_drafts` behavior no longer exist or have been adjusted:
- The `publish_all_drafts_flips_status` unit test was removed in Task 6.
- The existing integration test `single_schema_draft_published` (around line 2892) tests that `POST /api/v1/publish/production` flips drafts — this test will now **fail** since we changed that behavior. See Step 4.
- The existing integration test `production_publish_flips_drafts` (around line 2789) directly tests the old bulk-flip behavior — this test will now **fail**. See Step 5.

- [ ] **Step 4: Fix the existing `single_schema_draft_published` integration test**

The existing test in `tests/integration.rs` (the `single_schema_draft_published` test, around line 2892) expects that `POST /api/v1/publish/production` flips drafts to published. This behavior was removed. The test must be updated to use the new per-entry publish endpoint instead.

Find the section of that test that does:

```rust
    // Publish production — flips draft to published
    let _ = api
        .post(s.url("/api/v1/publish/production"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
```

Replace it with:

```rust
    // Publish the single entry via dedicated endpoint
    let resp = api
        .post(s.url("/api/v1/content/site-settings/_single/publish"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
```

- [ ] **Step 5: Fix the existing `production_publish_flips_drafts` integration test**

The existing test in `tests/integration.rs` (the `production_publish_flips_drafts` test, around line 2789) directly tests the old behavior where `POST /api/v1/publish/production` flips all drafts to published. This behavior no longer exists. Replace the entire test to use per-entry publish instead.

Find the test:

```rust
#[tokio::test]
async fn production_publish_flips_drafts() {
    ...
}
```

Replace the entire test body with a version that publishes entries individually via the new dedicated endpoints:

```rust
#[tokio::test]
async fn production_publish_does_not_flip_drafts() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    s.create_schema(DRAFT_TEST_SCHEMA).await;
    let token = s.create_api_token("publish-test").await;
    let api = Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    // Create two entries
    let resp = api
        .post(s.url("/api/v1/content/draft-posts"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Article 1"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let id1 = body["id"].as_str().unwrap().to_string();

    api.post(s.url("/api/v1/content/draft-posts"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Article 2"}))
        .send()
        .await
        .unwrap();

    // Verify both are draft
    let resp = api
        .get(s.url("/api/v1/content/draft-posts?status=draft"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 2);

    // Fire production publish — should NOT flip drafts
    let _ = api
        .post(s.url("/api/v1/publish/production"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    // Drafts should still be draft
    let resp = api
        .get(s.url("/api/v1/content/draft-posts?status=draft"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 2, "production publish should NOT flip drafts");

    // Publish one entry individually
    let resp = api
        .post(s.url(&format!("/api/v1/content/draft-posts/{id1}/publish")))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Now one published, one draft
    let resp = api
        .get(s.url("/api/v1/content/draft-posts"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1, "only the individually published entry should appear");

    let resp = api
        .get(s.url("/api/v1/content/draft-posts?status=draft"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let entries: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1, "one entry should still be draft");
}
```

- [ ] **Step 6: Run the full test suite again after the fixes**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add tests/integration.rs && git commit -m "test: add integration tests for per-entry publish/unpublish workflow"
```

---

## Final Verification

After all tasks are complete, run the following checks:

- [ ] **Full build:** `eval "$(direnv export bash 2>/dev/null)" && cargo build`
- [ ] **All tests pass:** `eval "$(direnv export bash 2>/dev/null)" && cargo test`
- [ ] **Clippy clean:** `eval "$(direnv export bash 2>/dev/null)" && cargo clippy`
- [ ] **Format clean:** `eval "$(direnv export bash 2>/dev/null)" && cargo fmt -- --check`

Verify key behaviors manually if possible:
1. Create a new entry via the UI — it starts as draft with a "Publish" button visible on the edit page.
2. Click "Publish" — the status changes inline to "Published" with an "Unpublish" button (via htmx).
3. Click "Unpublish" — the status reverts to "Draft".
4. Trigger a deployment via the "Publish Production" button — it only fires the webhook; it does not change any entry statuses.
5. Use the API to create an entry with `"_status": "published"` — verify it appears in the default published-only list.
6. Singles work the same way as collection entries.
