# Import/Export UI for Substrukt Admin

## Problem

Importing data into a running substrukt instance requires CLI access (SSH + docker exec), knowledge of correct `--data-dir` flags, and manual generation of `uploads-manifest.json`. This is error-prone and inaccessible to non-technical users.

## Solution

Add a browser-based import/export page to the existing admin UI at `/settings/data`. Uses the same form-submit + flash-message pattern as existing admin pages. No new dependencies.

## Architecture

### Routes

Add to `src/routes/settings.rs`:

| Method | Path | Handler | Purpose |
|--------|------|---------|---------|
| GET | `/settings/data` | `data_page()` | Render import/export page |
| POST | `/settings/data/import` | `import_data()` | Accept tar.gz, import, redirect with flash |
| POST | `/settings/data/export` | `export_data()` | Generate tar.gz, return as file download |

These are UI routes — session + CSRF auth, not bearer token. API endpoints remain unchanged.

### Template

`templates/settings/data.html` — extends `base.html` or `_partial.html` via `base_for_htmx`.

Two sections:

**Export:**
- `<form method="POST" action="/settings/data/export" hx-disable>` with CSRF hidden field
- Single "Download Bundle" button
- Description: "Export all schemas, content, and uploads as a .tar.gz bundle"
- `hx-disable` prevents HTMX boosting so the browser handles the file download natively.

**Import:**
- `<form method="POST" action="/settings/data/import" enctype="multipart/form-data" hx-disable>` with CSRF hidden field
- File input: `accept=".tar.gz,.tgz,application/gzip"`
- "Import Bundle" submit button with `onclick="return confirm('This will overwrite existing data. Continue?')"` confirmation dialog
- Description: "Import a .tar.gz bundle. This will overwrite existing schemas, content, and uploads."
- `hx-disable` so the multipart form posts normally (HTMX does not handle file downloads or multipart well).

**Flash messages — handled in this template, not `base.html`:**

The `data_page` handler passes `import_warnings` (a list of strings) and `import_status` ("success" | "warning" | "error") as separate template variables, extracted from the flash message. The template renders these directly:

- Success: green banner — "Bundle imported successfully"
- Warning: amber banner — "Bundle imported with N warnings" + `<details><summary>Show warnings</summary><ul>` listing each warning as `<li>`
- Error: red banner with error message

This avoids modifying `base.html`'s flash rendering. The import handler stores warnings as JSON in the flash value (e.g. `{"status": "warning", "message": "Bundle imported with 6 warnings", "warnings": [...]}`). The `data_page` handler deserializes this and passes structured data to the template.

### Navigation

Add "Data" link to `templates/_nav.html` next to the existing "API Tokens" link.

### Data Page Handler (`data_page`)

1. Call `auth::take_flash(&session)` to consume any pending flash
2. If flash kind is "import_result", deserialize the JSON value into `import_status`, `import_message`, and `import_warnings`
3. Pass `base_template`, `csrf_token`, `import_status`, `import_message`, `import_warnings` to template context
4. Render `settings/data.html`

### Import Handler (`import_data`)

1. Parse multipart form fields: extract `_csrf` token and `bundle` file bytes
2. Verify CSRF token manually against session (multipart forms bypass the CSRF middleware — this is a new pattern, not matching existing upload handlers which skip CSRF verification)
3. Validate file is present — if not, flash error and redirect
4. Call `sync::import_bundle_from_bytes(&state.config.data_dir, &state.pool, &bytes)`
5. On success: rebuild content cache via `cache::rebuild(&state.cache, &state.config.schemas_dir(), &state.config.content_dir())`
6. Audit log: `state.audit.log(&user_id.to_string(), "import", "bundle", "", None)`
7. Set flash:
   - No warnings: `set_flash("import_result", r#"{"status":"success","message":"Bundle imported successfully","warnings":[]}"#)`
   - With warnings: `set_flash("import_result", serde_json::to_string(&json!({"status":"warning","message":"Bundle imported with N warnings","warnings":[...]})))`
8. On error: `set_flash("import_result", r#"{"status":"error","message":"<error message>","warnings":[]}"#)`
9. Redirect to `/settings/data`

### Export Handler (`export_data`)

1. Extract and verify CSRF token from urlencoded form body
2. Create temp file via `tempfile::NamedTempFile`
3. Call `sync::export_bundle(&state.config.data_dir, &state.pool, &temp_path)`
4. Read exported bytes
5. Build `Content-Disposition` header with dynamic date using `HeaderValue::from_str(&format!("attachment; filename=\"substrukt-export-{}.tar.gz\"", chrono::Utc::now().format("%Y-%m-%d")))` (not `from_static`)
6. Audit log: `state.audit.log(&user_id.to_string(), "export", "bundle", "", None)`
7. Return response with `Content-Type: application/gzip` and the disposition header
8. Temp file cleaned up automatically on drop
9. On error: flash error and redirect to `/settings/data` (consistent with import error handling)

### Error Handling

- Missing file in import: flash error, redirect
- Corrupt/invalid tar.gz: `import_bundle_from_bytes` returns `Err` — flash the error message, redirect
- File too large: handled by existing 50MB body limit layer (returns 413)
- Export failure: flash error, redirect to `/settings/data`

## Files Changed

| File | Change |
|------|--------|
| `src/routes/settings.rs` | Add `data_page`, `import_data`, `export_data` handlers; register routes |
| `templates/settings/data.html` | New template for import/export page |
| `templates/_nav.html` | Add "Data" nav link |

## Out of Scope

- Progress indication / streaming (import is fast enough for typical bundle sizes)
- Import history / persistent warning log (warnings shown once via flash)
- Dual-auth on API endpoints (UI and API auth stay cleanly separated)
- CLI improvements (separate concern)
