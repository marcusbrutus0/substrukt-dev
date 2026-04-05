# Template & Display Fixes Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three template-level UX gaps: (1) Content form fields render in alphabetical order instead of schema-defined order, because `serde_json` defaults to `BTreeMap` for JSON objects. (2) Raw ISO timestamps displayed throughout the UI (audit log, invitations, tokens, deployments, uploads) need human-readable formatting. (3) The 404 error page renders without sidebar nav items because `render_error` does not pass `user_role`, `current_username`, or `csrf_token` to the template context.

**Architecture:** Fix (1) is a single dependency feature flag change -- enabling `serde_json/preserve_order` switches the internal `Map` from `BTreeMap` to `IndexMap`, which preserves JSON key insertion order. This is a zero-code-change fix that propagates everywhere `as_object()` is called. Fix (2) adds a custom minijinja filter `datefmt` registered in `src/templates.rs` that parses ISO 8601 strings and formats them as `"Jan 5, 2026 3:04 PM"`. Templates then pipe timestamps through `{{ val|datefmt }}`. Fix (3) updates `render_error` to accept optional pre-extracted nav context (user_role, current_username, csrf_token) so the sidebar nav renders correctly for authenticated users, while keeping the function synchronous to avoid restructuring async closures in `app_context.rs`.

**Tech Stack:** Rust, serde_json (preserve_order feature), minijinja (custom filter), Axum, chrono

---

## File Map

**Modified files:**
- `Cargo.toml` -- Enable `preserve_order` feature on `serde_json`
- `src/templates.rs` -- Register `datefmt` custom filter
- `templates/settings/audit_log.html` -- Apply `datefmt` filter to timestamps
- `templates/settings/users.html` -- Apply `datefmt` filter to invitation dates
- `templates/apps/settings.html` -- Apply `datefmt` filter to token `created_at`
- `templates/deployments/list.html` -- Apply `datefmt` filter to `last_fired` and history `created_at`
- `templates/uploads/list.html` -- Apply `datefmt` filter to upload `created_at`
- `src/routes/mod.rs` -- Update `render_error` signature to accept optional nav context; update `not_found` handler to extract and pass session data
- `src/app_context.rs` -- Update `render_error` calls to pass `None` for the new nav context parameters
- `NOTES.md` -- Record the `preserve_order` decision

---

### Task 1: Preserve schema property order in form fields

**Files:**
- Modify: `Cargo.toml`
- Modify: `NOTES.md`

**Depends on:** Nothing

- [ ] **Step 1: Enable `preserve_order` feature on `serde_json`**

  In `Cargo.toml`, change line 19 from:

  ```toml
  serde_json = "1"
  ```

  to:

  ```toml
  serde_json = { version = "1", features = ["preserve_order"] }
  ```

  This switches `serde_json::Map` from `BTreeMap` to `IndexMap`, preserving the key order from the original JSON file. Since schemas are authored as JSON files with intentional property ordering, this ensures form fields, API responses, and `generate_entry_id` all respect the author's intended order.

  **Impact analysis:** This is a global change affecting all `serde_json::Map` usage. Positive effects:
  - `render_form_fields` iterates `properties` in schema-defined order (the primary fix)
  - `form_data_from_fields` reconstructs data in schema-defined order
  - `generate_entry_id` picks the first string field in schema order, not alphabetical order
  - API JSON responses preserve property order

  No negative effects expected -- `IndexMap` supports all the same operations as `BTreeMap`. The `indexmap` crate is already a direct dependency (line 38 of `Cargo.toml`).

- [ ] **Step 2: Update NOTES.md**

  Add under "Architectural Decisions":

  ```
  - **serde_json preserve_order**: Enabled `preserve_order` feature on serde_json so JSON object keys use IndexMap instead of BTreeMap. This preserves schema property ordering in form field rendering, API output, and `generate_entry_id`. The `indexmap` crate was already a direct dependency.
  ```

- [ ] **Step 3: Verify**

  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo build && cargo test
  ```

  All existing tests must pass. Form fields should now render in schema-defined order instead of alphabetically.

**Commit message:** `fix: preserve schema property order in form fields via serde_json preserve_order`

---

### Task 2: Add `datefmt` template filter for human-readable timestamps

**Files:**
- Modify: `src/templates.rs`

**Depends on:** Nothing

- [ ] **Step 1: Add `datefmt` filter function**

  In `src/templates.rs`, add a function above the `create_reloader` function (before line 4):

  ```rust
  /// Minijinja filter: format ISO 8601 timestamp as human-readable date.
  /// Input: "2026-01-05T15:04:30.123456+00:00" or "2026-01-05T15:04:30Z"
  /// Output: "Jan 5, 2026 3:04 PM"
  /// Falls back to the original string if parsing fails.
  fn datefmt(value: &str) -> String {
      // Try RFC 3339 first (most common format from chrono::Utc::now().to_rfc3339())
      if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
          return dt.format("%b %-d, %Y %-I:%M %p").to_string();
      }
      // Try ISO 8601 without timezone (SQLite datetime() format: "2026-01-05 15:04:30")
      if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
          return dt.format("%b %-d, %Y %-I:%M %p").to_string();
      }
      // Try just the date-time portion if it has a T separator but no timezone
      if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S") {
          return dt.format("%b %-d, %Y %-I:%M %p").to_string();
      }
      // Fall back to original string
      value.to_string()
  }
  ```

  No `use chrono;` statement is needed -- Rust 2024 edition has implicit extern crate declarations, and the function uses fully-qualified paths (`chrono::DateTime`, `chrono::NaiveDateTime`). The `chrono` crate is already in `Cargo.toml` (line 10).

- [ ] **Step 2: Register the filter in `create_reloader`**

  Inside the `create_reloader` function's closure, add the filter registration just before the `Ok(env)` return (before line 27 in the current file). Place it after the `env.add_global(...)` line:

  ```rust
  env.add_filter("datefmt", datefmt);
  ```

- [ ] **Step 3: Verify**

  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo build
  ```

  Must compile without errors. The filter is now available in all templates as `{{ value|datefmt }}`.

**Commit message:** `feat: add datefmt template filter for human-readable timestamps`

---

### Task 3: Apply `datefmt` filter to all timestamp displays

**Files:**
- Modify: `templates/settings/audit_log.html`
- Modify: `templates/settings/users.html`
- Modify: `templates/apps/settings.html`
- Modify: `templates/deployments/list.html`
- Modify: `templates/uploads/list.html`

**Depends on:** Task 2

- [ ] **Step 1: Audit log timestamps**

  In `templates/settings/audit_log.html`, line 54, change:

  ```html
  <td class="px-4 py-2.5 text-sm text-muted font-mono">{{ entry.timestamp[:19] }}</td>
  ```

  to:

  ```html
  <td class="px-4 py-2.5 text-sm text-muted">{{ entry.timestamp|datefmt }}</td>
  ```

  Remove `font-mono` since the formatted date is no longer a raw ISO string.

- [ ] **Step 2: Invitation timestamps**

  In `templates/settings/users.html`, lines 54-55, change:

  ```html
  <td class="px-4 py-2.5 text-muted text-sm">{{ inv.created_at }}</td>
  <td class="px-4 py-2.5 text-muted text-sm">{{ inv.expires_at }}</td>
  ```

  to:

  ```html
  <td class="px-4 py-2.5 text-muted text-sm">{{ inv.created_at|datefmt }}</td>
  <td class="px-4 py-2.5 text-muted text-sm">{{ inv.expires_at|datefmt }}</td>
  ```

- [ ] **Step 3: API token timestamps**

  In `templates/apps/settings.html`, line 81, change:

  ```html
  <td class="py-2.5 text-muted">{{ token.created_at }}</td>
  ```

  to:

  ```html
  <td class="py-2.5 text-muted">{{ token.created_at|datefmt }}</td>
  ```

- [ ] **Step 4: Deployment timestamps**

  In `templates/deployments/list.html`:

  Line 60 -- deployment `last_fired`, change:

  ```html
  {% if dep.last_fired %}{{ dep.last_fired[:19] }}{% else %}Never{% endif %}
  ```

  to:

  ```html
  {% if dep.last_fired %}{{ dep.last_fired|datefmt }}{% else %}Never{% endif %}
  ```

  Line 107 -- webhook history `created_at`, change:

  ```html
  <td class="px-4 py-2.5 text-sm text-muted font-mono">{{ entry.created_at[:19] }}</td>
  ```

  to:

  ```html
  <td class="px-4 py-2.5 text-sm text-muted">{{ entry.created_at|datefmt }}</td>
  ```

  Remove `font-mono` from the webhook history timestamp cell as well.

- [ ] **Step 5: Upload timestamps**

  In `templates/uploads/list.html`, line 40, change:

  ```html
  <td class="py-2.5 text-muted text-xs">{{ upload.created_at }}</td>
  ```

  to:

  ```html
  <td class="py-2.5 text-muted text-xs">{{ upload.created_at|datefmt }}</td>
  ```

- [ ] **Step 6: Verify**

  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo build && cargo test
  ```

  All tests must pass. Visually confirm by running the app that timestamps now display as e.g. "Jan 5, 2026 3:04 PM" instead of raw ISO strings.

**Commit message:** `fix: format timestamps as human-readable dates across all UI pages`

---

### Task 4: Fix 404 error page missing sidebar nav items

**Files:**
- Modify: `src/routes/mod.rs`
- Modify: `src/app_context.rs`

**Depends on:** Nothing

**Design note:** `render_error` must stay synchronous because it is called inside `.map_err(|_| {...})` and `.ok_or_else(|| {...})` closures in `src/app_context.rs`, which cannot be async. Instead of making `render_error` async and accepting a `Session` reference, we accept pre-extracted string values. The `not_found` handler (which is already async) extracts user_role/username/csrf_token from the session before calling `render_error`. All `app_context.rs` call sites pass `None` since they are in error paths where the sidebar nav is not critical.

- [ ] **Step 1: Update `render_error` to accept optional nav context**

  In `src/routes/mod.rs`, change the `render_error` function (lines 112-126) from:

  ```rust
  pub fn render_error(state: &AppState, status: u16, message: &str, is_htmx: bool) -> String {
      let Ok(tmpl) = state.templates.acquire_env() else {
          return format!("<h1>{status}</h1><p>{message}</p>");
      };
      if let Ok(template) = tmpl.get_template("error.html")
          && let Ok(html) = template.render(minijinja::context! {
              base_template => base_for_htmx(is_htmx),
              status => status,
              message => message,
          })
      {
          return html;
      }
      format!("<h1>{status}</h1><p>{message}</p>")
  }
  ```

  to:

  ```rust
  /// Render an error page. Accepts optional nav context for the sidebar.
  /// When `nav` is `None`, the sidebar renders without user-specific items (no admin links, no username, no logout).
  /// When `nav` is `Some((role, username, csrf))`, the sidebar renders fully.
  pub fn render_error(
      state: &AppState,
      status: u16,
      message: &str,
      is_htmx: bool,
      nav: Option<(&str, &str, &str)>,
  ) -> String {
      let (user_role, current_username, csrf_token) = nav.unwrap_or(("", "", ""));
      let Ok(tmpl) = state.templates.acquire_env() else {
          return format!("<h1>{status}</h1><p>{message}</p>");
      };
      if let Ok(template) = tmpl.get_template("error.html")
          && let Ok(html) = template.render(minijinja::context! {
              base_template => base_for_htmx(is_htmx),
              status => status,
              message => message,
              user_role => user_role,
              current_username => current_username,
              csrf_token => csrf_token,
          })
      {
          return html;
      }
      format!("<h1>{status}</h1><p>{message}</p>")
  }
  ```

- [ ] **Step 2: Update `not_found` handler to extract and pass session data**

  In `src/routes/mod.rs`, update the `not_found` function (lines 91-110). Change the render_error call from:

  ```rust
      let html = render_error(&state, 404, "Page not found", is_htmx);
      (axum::http::StatusCode::NOT_FOUND, Html(html)).into_response()
  ```

  to:

  ```rust
      let user_role = crate::auth::current_user_role(&session)
          .await
          .unwrap_or_default();
      let username = crate::auth::current_username(&session)
          .await
          .unwrap_or_default();
      let csrf = crate::auth::ensure_csrf_token(&session).await;
      let html = render_error(&state, 404, "Page not found", is_htmx, Some((&user_role, &username, &csrf)));
      (axum::http::StatusCode::NOT_FOUND, Html(html)).into_response()
  ```

- [ ] **Step 3: Update all `render_error` call sites in `app_context.rs`**

  In `src/app_context.rs`, add `None` as the fifth argument to every `render_error` call. There are 8 call sites -- all pass `None` since they are in error-handling paths (closures or match arms) where session context is either unavailable or the error itself indicates a broken state:

  Line 67: `render_error(state, 404, "Not found", is_htmx, None)`
  Line 73: `render_error(state, 404, "Not found", is_htmx, None)`
  Line 81: `render_error(state, 500, "Internal error", is_htmx, None)`
  Line 85: `render_error(state, 404, "App not found", is_htmx, None)`
  Line 91: `render_error(state, 500, "Session not available", is_htmx, None)`
  Line 98: `render_error(state, 403, "Not authenticated", is_htmx, None)`
  Line 111: `render_error(state, 500, "Internal error", is_htmx, None)`
  Line 115: `render_error(state, 403, "You do not have access to this app", is_htmx, None)`

- [ ] **Step 4: Verify**

  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo build && cargo test
  ```

  All tests must pass. To visually verify: navigate to a non-existent URL while logged in (e.g. `/nonexistent`) and confirm the sidebar nav appears with appropriate links (Apps, Users, Audit Log, etc. for admin users).

**Commit message:** `fix: show sidebar nav on 404 error page for authenticated users`

---

## Final Verification

After all four tasks are complete:

- [ ] **Full test suite passes:**
  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo test
  ```

- [ ] **Build succeeds in release mode:**
  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo build --release
  ```

- [ ] **Clippy clean:**
  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo clippy -- -D warnings
  ```

- [ ] **Format check:**
  ```bash
  eval "$(direnv export bash 2>/dev/null)" && cargo fmt -- --check
  ```

- [ ] **Manual verification (run the app):**
  1. Create a schema with properties in a specific order (e.g. title, body, author). Verify the content form renders fields in that order, not alphabetical.
  2. Check the audit log page -- timestamps should read like "Jan 5, 2026 3:04 PM".
  3. Check the users page -- invitation dates should be formatted.
  4. Check app settings -- token `Created` column should be formatted.
  5. Navigate to `/nonexistent` while logged in as admin -- sidebar should show full nav (Apps, Users, Audit Log, Backups).
  6. Navigate to `/nonexistent` while logged in as editor -- sidebar should show Apps link but not admin-only links.
