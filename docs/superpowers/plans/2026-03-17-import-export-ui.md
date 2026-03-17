# Import/Export UI Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a browser-based import/export page to the substrukt admin UI at `/settings/data`.

**Architecture:** Three new handlers in `src/routes/settings.rs` (GET page, POST import, POST export) using the existing form-submit + flash-message + redirect pattern. One new template. One nav link addition. No new dependencies.

**Tech Stack:** Rust/Axum, Minijinja templates, HTMX, Twind CSS, SQLite

**Spec:** `docs/superpowers/specs/2026-03-17-import-export-ui-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/routes/settings.rs` | Modify | Add `data_page`, `import_data`, `export_data` handlers; update `routes()` |
| `templates/settings/data.html` | Create | Import/export page template with flash message rendering |
| `templates/_nav.html` | Modify | Add "Data" nav link |

---

## Chunk 1: Template and Navigation

### Task 1: Add Data nav link

**Files:**
- Modify: `templates/_nav.html:24-25`

- [ ] **Step 1: Add the nav link**

In `templates/_nav.html`, after the "API Tokens" link (line 25), add the "Data" link:

```html
  <a href="/uploads" class="block px-3 py-2 rounded hover:bg-sidebar-hover mt-3">Uploads</a>
  <a href="/settings/tokens" class="block px-3 py-2 rounded hover:bg-sidebar-hover">API Tokens</a>
  <a href="/settings/data" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Data</a>
```

- [ ] **Step 2: Commit**

```bash
git add templates/_nav.html
git commit -m "feat: add Data nav link to sidebar"
```

### Task 2: Create data.html template

**Files:**
- Create: `templates/settings/data.html`

- [ ] **Step 1: Create the template**

Create `templates/settings/data.html`:

```html
{% extends base_template %}
{% block title %}Data — Substrukt{% endblock %}
{% block content %}
<h1 class="text-2xl font-bold tracking-tight mb-6">Data</h1>

{% if import_status == "success" %}
<div class="bg-success-soft border border-success text-success p-4 rounded mb-6">
  {{ import_message }}
</div>
{% elif import_status == "warning" %}
<div class="bg-accent-soft border border-accent text-accent p-4 rounded mb-6">
  <div class="font-medium">{{ import_message }}</div>
  {% if import_warnings %}
  <details class="mt-2">
    <summary class="text-sm cursor-pointer hover:underline">Show warnings</summary>
    <ul class="mt-2 text-sm list-disc list-inside space-y-1">
      {% for w in import_warnings %}
      <li>{{ w }}</li>
      {% endfor %}
    </ul>
  </details>
  {% endif %}
</div>
{% elif import_status == "error" %}
<div class="bg-danger-soft border border-danger text-danger p-4 rounded mb-6">
  {{ import_message }}
</div>
{% endif %}

<div class="bg-card border border-border-light rounded-lg p-6 mb-6">
  <h2 class="text-lg font-medium mb-2">Export</h2>
  <p class="text-muted text-sm mb-4">Export all schemas, content, and uploads as a .tar.gz bundle.</p>
  <form method="post" action="/settings/data/export" hx-disable>
    <input type="hidden" name="_csrf" value="{{ csrf_token }}">
    <button type="submit" class="bg-accent text-black px-4 py-2 rounded-md hover:bg-accent-hover text-sm font-medium">
      Download Bundle
    </button>
  </form>
</div>

<div class="bg-card border border-border-light rounded-lg p-6">
  <h2 class="text-lg font-medium mb-2">Import</h2>
  <p class="text-muted text-sm mb-4">Import a .tar.gz bundle. This will overwrite existing schemas, content, and uploads.</p>
  <form method="post" action="/settings/data/import" enctype="multipart/form-data" hx-disable>
    <input type="hidden" name="_csrf" value="{{ csrf_token }}">
    <div class="flex gap-3 items-end">
      <input type="file" name="bundle" accept=".tar.gz,.tgz,application/gzip" required
        class="flex-1 text-sm file:mr-3 file:py-2 file:px-4 file:rounded-md file:border-0 file:text-sm file:font-medium file:bg-card-alt file:text-primary hover:file:bg-sidebar-hover">
      <button type="submit" class="bg-accent text-black px-4 py-2 rounded-md hover:bg-accent-hover text-sm font-medium"
        onclick="return confirm('This will overwrite existing data. Continue?')">
        Import Bundle
      </button>
    </div>
  </form>
</div>
{% endblock %}
```

- [ ] **Step 2: Commit**

```bash
git add templates/settings/data.html
git commit -m "feat: add import/export data page template"
```

---

## Chunk 2: Route Handlers

### Task 3: Add all three handlers and update routes

All three handlers (`data_page`, `import_data`, `export_data`) must be added together since the route registration references all of them. This task adds all handlers and updates the route table in one step.

**Files:**
- Modify: `src/routes/settings.rs`

- [ ] **Step 1: Update imports**

Replace the existing imports at the top of `src/routes/settings.rs`:

```rust
use axum::{
    Form, Router,
    body::Body,
    extract::{Multipart, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use axum_htmx::HxRequest;
use tower_sessions::Session;

use crate::auth;
use crate::auth::token;
use crate::db::models;
use crate::state::AppState;
use crate::templates::base_for_htmx;
```

- [ ] **Step 2: Add the data routes to `routes()`**

Update the `routes()` function:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/tokens", get(tokens_page).post(create_token))
        .route(
            "/tokens/{token_id}/delete",
            axum::routing::post(delete_token),
        )
        .route("/data", get(data_page))
        .route("/data/import", axum::routing::post(import_data))
        .route("/data/export", axum::routing::post(export_data))
}
```

- [ ] **Step 3: Add the data_page handler**

Add the `data_page` handler after the existing `delete_token` handler. It consumes the flash message and passes structured data to the template:

```rust
async fn data_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<Html<String>> {
    let csrf_token = auth::ensure_csrf_token(&session).await;

    // Consume flash message if present
    let mut import_status = String::new();
    let mut import_message = String::new();
    let mut import_warnings: Vec<String> = Vec::new();

    if let Some((kind, value)) = auth::take_flash(&session).await {
        if kind == "data_result" {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value) {
                import_status = parsed["status"].as_str().unwrap_or("").to_string();
                import_message = parsed["message"].as_str().unwrap_or("").to_string();
                if let Some(warnings) = parsed["warnings"].as_array() {
                    import_warnings = warnings
                        .iter()
                        .filter_map(|w| w.as_str().map(String::from))
                        .collect();
                }
            }
        }
    }

    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/data.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            import_status => import_status,
            import_message => import_message,
            import_warnings => import_warnings,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}
```

- [ ] **Step 4: Add the import_data handler**

```rust
async fn import_data(
    State(state): State<AppState>,
    session: Session,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let user_id = match auth::current_user_id(&session).await {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    // Extract CSRF token and bundle from multipart fields
    let mut csrf_token = None;
    let mut bundle_data = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name().unwrap_or("") {
            "_csrf" => {
                if let Ok(text) = field.text().await {
                    csrf_token = Some(text);
                }
            }
            "bundle" => {
                if let Ok(bytes) = field.bytes().await {
                    if !bytes.is_empty() {
                        bundle_data = Some(bytes);
                    }
                }
            }
            _ => {}
        }
    }

    // Verify CSRF
    let csrf_valid = match &csrf_token {
        Some(token) => auth::verify_csrf_token(&session, token).await,
        None => false,
    };
    if !csrf_valid {
        auth::set_flash(
            &session,
            "data_result",
            &serde_json::json!({"status": "error", "message": "Invalid CSRF token", "warnings": []}).to_string(),
        ).await;
        return Redirect::to("/settings/data").into_response();
    }

    // Validate bundle present
    let data = match bundle_data {
        Some(d) => d,
        None => {
            auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({"status": "error", "message": "No file provided", "warnings": []}).to_string(),
            ).await;
            return Redirect::to("/settings/data").into_response();
        }
    };

    // Import
    match crate::sync::import_bundle_from_bytes(&state.config.data_dir, &state.pool, &data).await {
        Ok(warnings) => {
            crate::cache::rebuild(
                &state.cache,
                &state.config.schemas_dir(),
                &state.config.content_dir(),
            );
            state.audit.log(
                &user_id.to_string(),
                "import",
                "bundle",
                "",
                None,
            );

            let (status, message) = if warnings.is_empty() {
                ("success".to_string(), "Bundle imported successfully".to_string())
            } else {
                (
                    "warning".to_string(),
                    format!("Bundle imported with {} warnings", warnings.len()),
                )
            };

            auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({
                    "status": status,
                    "message": message,
                    "warnings": warnings,
                }).to_string(),
            ).await;
        }
        Err(e) => {
            auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({"status": "error", "message": e.to_string(), "warnings": []}).to_string(),
            ).await;
        }
    }

    Redirect::to("/settings/data").into_response()
}
```

- [ ] **Step 5: Add the export_data handler**

```rust
async fn export_data(
    State(state): State<AppState>,
    session: Session,
    Form(_form): Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let user_id = match auth::current_user_id(&session).await {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let tmp = std::env::temp_dir().join(format!(
        "substrukt-export-{}.tar.gz",
        uuid::Uuid::new_v4()
    ));

    match crate::sync::export_bundle(&state.config.data_dir, &state.pool, &tmp).await {
        Ok(()) => match std::fs::read(&tmp) {
            Ok(data) => {
                let _ = std::fs::remove_file(&tmp);
                state.audit.log(
                    &user_id.to_string(),
                    "export",
                    "bundle",
                    "",
                    None,
                );

                let date = chrono::Utc::now().format("%Y-%m-%d");
                let filename = format!("substrukt-export-{date}.tar.gz");
                let disposition = format!("attachment; filename=\"{filename}\"");

                let mut response = Body::from(data).into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/gzip"),
                );
                if let Ok(val) = HeaderValue::from_str(&disposition) {
                    response
                        .headers_mut()
                        .insert(header::CONTENT_DISPOSITION, val);
                }
                response
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                auth::set_flash(
                    &session,
                    "data_result",
                    &serde_json::json!({"status": "error", "message": format!("Export failed: {e}"), "warnings": []}).to_string(),
                ).await;
                Redirect::to("/settings/data").into_response()
            }
        },
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({"status": "error", "message": format!("Export failed: {e}"), "warnings": []}).to_string(),
            ).await;
            Redirect::to("/settings/data").into_response()
        }
    }
}
```

Note: The export form is `application/x-www-form-urlencoded` (no file upload), so the CSRF middleware handles `_csrf` verification automatically. The handler uses `Form<HashMap<String, String>>` to consume the body (the `_csrf` field is already verified by middleware). This is the same pattern as the publish handler.

- [ ] **Step 6: Verify everything compiles**

```bash
cargo check
```

Expected: clean compilation. All three handlers and updated routes are now in place.

- [ ] **Step 7: Commit**

```bash
git add src/routes/settings.rs
git commit -m "feat: add import/export data handlers with CSRF, audit logging, and flash messages"
```

---

## Chunk 3: Integration Verification

### Task 4: Manual smoke test

- [ ] **Step 1: Start the dev server**

```bash
cargo run -- serve --data-dir data
```

- [ ] **Step 2: Verify the page renders**

Navigate to `http://localhost:3000/settings/data`. Verify:
- Both Export and Import sections visible
- "Data" link in sidebar nav is present and active
- CSRF hidden fields are in both forms

- [ ] **Step 3: Test export**

Click "Download Bundle". Verify:
- Browser downloads a `.tar.gz` file named `substrukt-export-YYYY-MM-DD.tar.gz`
- File contains `schemas/`, `content/`, `uploads/`, `uploads-manifest.json`

- [ ] **Step 4: Test import**

Select the just-exported bundle and click "Import Bundle". Verify:
- Confirmation dialog appears ("This will overwrite existing data. Continue?")
- After confirming, redirects to `/settings/data` with success flash
- If the exported bundle had validation issues, warnings appear in a collapsible `<details>` block

- [ ] **Step 5: Test error case**

Upload a non-tar.gz file (e.g. a .txt file). Verify:
- Error flash message appears on redirect

- [ ] **Step 6: Final commit**

If any fixes were needed during smoke testing:

```bash
git add -A
git commit -m "fix: address issues found during import/export smoke test"
```
