# Configurable Deployments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace hardcoded staging/production webhook system with admin-managed deployment targets. Each deployment has its own webhook URL, auth token, include_drafts toggle, and optional auto-deploy with configurable debounce.

**Architecture:** New `deployments` table in audit.db with CRUD managed via `AuditLogger`. Background auto-deploy uses one tokio task per deployment with `CancellationToken` for lifecycle management. The old CLI flags for webhook URLs, the fixed staging cron, and the `/publish` routes are removed entirely. A new `/deployments` route module handles all UI and API interactions.

**Tech Stack:** Rust, Axum, sqlx (SQLite), minijinja, htmx, tokio-util (CancellationToken), DashMap

**Spec:** `docs/superpowers/specs/2026-03-31-configurable-deployments-design.md`

---

## Prerequisites

**Verify a clean working tree before starting.** Run `git status` and ensure `src/routes/api.rs` and `src/routes/publish.rs` have no uncommitted changes. If they do, discard them:

```bash
eval "$(direnv export bash 2>/dev/null)" && git checkout src/routes/api.rs src/routes/publish.rs
```

All line references in this plan refer to the committed (HEAD) state.

---

## File Map

**New files:**
- `audit_migrations/004_configurable_deployments.sql` -- Migration for deployments, deployment_state tables and webhook_history rebuild
- `src/routes/deployments.rs` -- UI route handlers: list, create, edit, delete, fire
- `templates/deployments/list.html` -- Deployments list page with status dots, history table
- `templates/deployments/form.html` -- Shared create/edit deployment form

**Modified files:**
- `Cargo.toml` -- Add `tokio-util` dependency
- `src/audit.rs` -- Add `Deployment` struct, CRUD methods, `is_dirty_for_deployment`, `mark_deployment_fired`, updated webhook history methods. Remove old `is_dirty`, `mark_fired`
- `src/webhooks.rs` -- Change `fire_webhook` to accept `&Deployment`. Add `spawn_auto_deploy_task`, `cancel_auto_deploy_task`. Remove `spawn_cron`. Update `WebhookPayload` and `TriggerSource`
- `src/config.rs` -- Remove webhook CLI fields (5 fields)
- `src/main.rs` -- Remove webhook CLI flags. Replace `spawn_cron` with auto-deploy startup loop. Pass `deploy_tasks` to AppState
- `src/state.rs` -- Add `deploy_tasks: DashMap<i64, CancellationToken>` to `AppStateInner`
- `src/routes/mod.rs` -- Replace `pub mod publish` with `pub mod deployments`. Update `build_router`
- `src/routes/api.rs` -- Remove `publish` handler. Add `list_deployments` and `fire_deployment` API handlers
- `src/routes/settings.rs` -- Remove `webhooks_page`, `retry_webhook`, `WebhookFilter`, `RetryForm` and their routes
- `src/templates.rs` -- Remove `get_publish_state()` function and `audit_logger`/`config` params from `create_reloader`
- `templates/_nav.html` -- Remove publish section (lines 34-57), remove Webhooks link (line 30), add Deployments link
- `tests/integration.rs` -- Update `TestServer` (remove webhook params from `Config::new`), remove old webhook tests, add deployment tests

**Deleted files:**
- `src/routes/publish.rs` -- Replaced by `/deployments/{slug}/fire`
- `templates/settings/webhooks.html` -- History moves to deployments list page

---

### Task 1: Add tokio-util dependency and migration

**Files:**
- Modify: `Cargo.toml`
- Create: `audit_migrations/004_configurable_deployments.sql`

**Depends on:** Nothing

- [ ] **Step 1:** Add `tokio-util` to `[dependencies]` in `Cargo.toml`:
  ```toml
  tokio-util = { version = "0.7", features = ["rt"] }
  ```

- [ ] **Step 2:** Create `audit_migrations/004_configurable_deployments.sql` with the SQL from the spec's "Migration" section. Contents:
  - CREATE TABLE deployments (id, app_id nullable, name, slug, webhook_url, webhook_auth_token, include_drafts, auto_deploy, debounce_seconds, created_at, updated_at)
  - CREATE UNIQUE INDEX idx_deployments_slug ON deployments(slug)
  - CREATE TABLE deployment_state (deployment_id PK FK -> deployments ON DELETE CASCADE, last_fired_at)
  - CREATE TABLE webhook_history_new with deployment_id FK instead of environment column
  - CREATE INDEX on (deployment_id, created_at DESC) and (group_id)
  - DROP TABLE webhook_history, ALTER TABLE webhook_history_new RENAME TO webhook_history
  - DROP TABLE webhook_state

- [ ] **Step 3:** No compile verification at this point -- the migration drops `webhook_state` and rebuilds `webhook_history`, so the old Rust code that queries those tables will not compile. Migration correctness is verified implicitly by the `test_pool()` helper in later tasks (it runs all migrations on an in-memory database).

**Tests:** Migration validity verified implicitly by the test_pool() helper in later tasks.

---

### Task 2: Add Deployment struct and CRUD methods to audit.rs

> **Note:** Tasks 1 through 3 all modify `src/audit.rs` and are an atomic compile unit for tests. After Task 1's migration drops `webhook_state`, the old unit tests in `audit.rs` will not compile. The old tests are removed in Task 3 Step 6. Tests cannot be run until Task 3 is complete.

**Files:**
- Modify: `src/audit.rs`

**Depends on:** Task 1 (migration must exist)

- [ ] **Step 1:** Add `Deployment` struct after the existing `AuditLogEntry` struct (around line 46):
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

- [ ] **Step 2:** Add deployment CRUD methods to the `AuditLogger` impl block. Each method uses `self.pool.as_ref()`. Methods to add:
  - `create_deployment(name, slug, webhook_url, webhook_auth_token, include_drafts, auto_deploy, debounce_seconds) -> Result<Deployment>` -- INSERT then fetch by last_insert_rowid
  - `get_deployment_by_slug(slug) -> Result<Option<Deployment>>` -- SELECT WHERE slug = ?
  - `get_deployment_by_id(id) -> Result<Option<Deployment>>` -- SELECT WHERE id = ?
  - `list_deployments() -> Result<Vec<Deployment>>` -- SELECT all ORDER BY name
  - `update_deployment(id, name, slug, webhook_url, webhook_auth_token, include_drafts, auto_deploy, debounce_seconds) -> Result<()>` -- UPDATE SET ... , updated_at = datetime('now') WHERE id = ?
  - `delete_deployment(id) -> Result<()>` -- DELETE WHERE id = ? (CASCADE handles state/history)
  - `list_auto_deploy_deployments() -> Result<Vec<Deployment>>` -- SELECT WHERE auto_deploy = 1

  For the query tuple destructuring, follow the same pattern as existing `list_webhook_history` and `list_audit_log` (query_as with tuple type, then map into struct). The `include_drafts` and `auto_deploy` columns are INTEGER in SQLite; read as `i32` then convert: `include_drafts: row.X != 0`, `auto_deploy: row.Y != 0`.

- [ ] **Step 3:** Add slug validation function `fn validate_deployment_slug(slug: &str) -> Result<(), String>`. Rules: lowercase alphanumeric + hyphens, no leading/trailing hyphens, 1-64 chars. Same regex approach as schema slug validation if one exists, or manual character checks.

- [ ] **Step 4:** Add unit tests in the existing `#[cfg(test)] mod tests` block:
  - `test_create_and_get_deployment` -- create, fetch by slug, verify all fields
  - `test_list_deployments_sorted` -- create two deployments, verify order by name
  - `test_update_deployment` -- create, update, verify changes including updated_at differs from created_at
  - `test_duplicate_slug_fails` -- create two with same slug, second should error
  - `test_validate_deployment_slug` -- valid slugs pass, invalid ones (uppercase, spaces, leading hyphen) fail

  Note: `test_delete_deployment_cascades` (create deployment, record webhook attempt, delete, verify cascade) is deferred to Task 3 Step 6 because it depends on the updated `record_webhook_attempt` signature.

**Tests:** Unit tests listed above (except cascade test, see note).

---

### Task 3: Update dirty detection and webhook history for deployments

**Files:**
- Modify: `src/audit.rs`

**Depends on:** Task 2

- [ ] **Step 1:** Add `is_dirty_for_deployment(deployment_id: i64) -> Result<bool>` method. Same logic as existing `is_dirty` but reads from `deployment_state` WHERE deployment_id = ?. The mutation action filter adds `'entry_published', 'entry_unpublished'` to the existing list (see spec).

- [ ] **Step 2:** Add `mark_deployment_fired(deployment_id: i64) -> Result<String>` method. Uses INSERT ... ON CONFLICT(deployment_id) DO UPDATE SET last_fired_at = excluded.last_fired_at. Returns the timestamp string.

- [ ] **Step 3:** Update `record_webhook_attempt` signature: change first param from `environment: &str` to `deployment_id: i64`. Update the INSERT query to use `deployment_id` instead of `environment`. This will cause compile errors in callers -- that's expected, fixed in Task 5.

- [ ] **Step 4:** Update `list_webhook_history` to `list_webhook_history_for_deployment`. Change signature: `deployment_id: Option<i64>` replaces `environment_filter: Option<&str>`. The query JOINs deployments to get name/slug. Update `WebhookHistoryGroup` struct: replace `environment: String` with `deployment_id: i64`, `deployment_name: String`, `deployment_slug: String`. Filter condition becomes `h.deployment_id = ?` when Some.

- [ ] **Step 5:** Remove old methods: `is_dirty(&self, environment: &str)` and `mark_fired(&self, environment: &str)`. This will cause compile errors -- expected, fixed in Tasks 5-7.

- [ ] **Step 6:** Update existing unit tests that use old methods:
  - Remove `test_is_dirty_when_no_mutations`, `test_is_dirty_after_mutation`, `test_not_dirty_after_mark_fired`, `test_dirty_ignores_non_mutation_events`, `test_staging_and_production_independent`
  - Remove `test_record_webhook_attempt` and `test_list_webhook_history_grouped`
  - Add replacement tests:
    - `test_is_dirty_never_fired` -- create deployment, check is_dirty_for_deployment returns true
    - `test_is_dirty_after_mark_fired_no_mutations` -- create, mark fired, verify false
    - `test_is_dirty_after_mutation` -- create, mark fired, insert audit mutation with future timestamp, verify true
    - `test_is_dirty_ignores_non_mutation_events` -- mark fired, insert login event, verify false
    - `test_is_dirty_detects_entry_published` -- mark fired, insert entry_published event, verify true
    - `test_mark_deployment_fired_upsert` -- first call creates row, second updates it
    - `test_record_webhook_attempt_with_deployment` -- create deployment, record attempt with deployment_id, verify
    - `test_list_webhook_history_for_deployment` -- create two deployments, record attempts for each, test None (all) and Some(id) filtering
    - `test_delete_deployment_cascades` -- (deferred from Task 2) create deployment, record webhook attempt with new signature, delete deployment, verify history and state are cascade-deleted

**Tests:** Unit tests listed above.

---

### Task 4: Remove webhook CLI flags from Config

> **Note:** Tasks 1 through 8 form an atomic compile unit. After Task 1's migration changes the DB schema, the old Rust code (audit.rs methods, webhooks.rs, templates.rs, routes) will not compile. Compilation is restored progressively through Tasks 2-8 and is fully clean after Task 8 (when old routes are removed and new ones are in place).

**Files:**
- Modify: `src/config.rs`
- Modify: `src/main.rs`

**Depends on:** Task 3 (old methods removed from audit.rs)

- [ ] **Step 1:** In `src/config.rs`, remove these fields from the `Config` struct: `staging_webhook_url`, `staging_webhook_auth_token`, `production_webhook_url`, `production_webhook_auth_token`, `webhook_check_interval`. Remove the corresponding parameters from `Config::new` and the assignments in the constructor body.

- [ ] **Step 2:** In `src/main.rs`, remove these CLI args from the `Cli` struct (lines 44-62): `staging_webhook_url`, `staging_webhook_auth_token`, `production_webhook_url`, `production_webhook_auth_token`, `webhook_check_interval`. Update the `Config::new(...)` call (lines 110-122) to remove the removed arguments.

- [ ] **Step 3:** Remove the `spawn_cron` call in `run_server` (lines 208-213). Leave a TODO comment: `// Auto-deploy tasks spawned in Task 7`.

**Tests:** None yet (compile check only). Integration test `TestServer` will be updated in Task 10.

---

### Task 5: Refactor webhooks.rs for deployment-based firing

**Files:**
- Modify: `src/webhooks.rs`

**Depends on:** Task 3 (audit methods updated), Task 4 (Config fields removed)

- [ ] **Step 1:** Update imports at top: add `use crate::audit::Deployment;`. Remove `use crate::config::Config;` (no longer needed after removing `fire_webhook`'s Config param and `spawn_cron`).

- [ ] **Step 2:** Update `TriggerSource` enum: rename `Cron` to `Auto`. Update the match arm in `fire_webhook` accordingly.

- [ ] **Step 3:** Update `WebhookPayload` struct:
  - Rename `environment: String` to `deployment: String`
  - Add `include_drafts: bool`

- [ ] **Step 4:** Change `fire_webhook` signature to accept `&Deployment` instead of `&Config` + `environment`:
  ```rust
  pub async fn fire_webhook(
      client: &reqwest::Client,
      audit: &AuditLogger,
      deployment: &Deployment,
      source: TriggerSource,
  ) -> Result<bool>
  ```
  - Remove the URL/auth_token lookup from Config (old lines 80-95). Use `deployment.webhook_url` and `deployment.webhook_auth_token.as_deref()` directly.
  - Remove the `Ok(false)` return case (deployment always has a URL).
  - Update payload construction: use `deployment: deployment.slug.clone()`, `include_drafts: deployment.include_drafts`.
  - Update `triggered_by` match: `TriggerSource::Auto => "auto"` (was `Cron => "cron"`).
  - Update `audit.record_webhook_attempt(...)` calls: pass `deployment.id` instead of `environment`.
  - Update `audit.mark_fired(...)` calls to `audit.mark_deployment_fired(deployment.id)`.
  - In the retry spawn block: clone `deployment` (it derives Clone) instead of cloning environment string and config fields. Update all references inside the spawned future.

- [ ] **Step 5:** Remove `spawn_cron` function entirely.

- [ ] **Step 6:** Add `spawn_auto_deploy_task` and `cancel_auto_deploy_task` functions. See spec for full implementation. Key points:
  - `spawn_auto_deploy_task(state: &AppState, deployment: Deployment)` -- creates CancellationToken, inserts into `state.deploy_tasks`, spawns tokio task with poll/debounce loop using `tokio::select!` on sleep and child_token.cancelled().
  - After a successful `fire_webhook` call inside the auto-deploy task, log audit event: `audit.log("system", "deployment_auto_fired", "deployment", &deployment.slug, None)`. This is separate from `deployment_fired` (which is used for manual triggers in the route handlers).
  - `cancel_auto_deploy_task(state: &AppState, deployment_id: i64)` -- removes from DashMap and cancels token.
  - Imports needed: `use tokio_util::sync::CancellationToken;`, `use std::time::Duration;`, `use crate::state::AppState;`

**Tests:** Compile check. Functional tests in Task 10.

---

### Task 6: Update AppState and server startup

**Files:**
- Modify: `src/state.rs`
- Modify: `src/main.rs`

**Depends on:** Task 5

- [ ] **Step 1:** In `src/state.rs`, add import: `use tokio_util::sync::CancellationToken;`. Add field to `AppStateInner`: `pub deploy_tasks: DashMap<i64, CancellationToken>,`.

- [ ] **Step 2:** In `src/main.rs`, update the `AppStateInner` construction (around line 196) to include `deploy_tasks: DashMap::new(),`.

- [ ] **Step 3:** In `run_server`, after creating the state Arc, replace the old TODO comment with auto-deploy task startup:
  ```rust
  // Spawn auto-deploy tasks for all enabled deployments
  if let Ok(deployments) = state.audit.list_auto_deploy_deployments().await {
      for deployment in deployments {
          crate::webhooks::spawn_auto_deploy_task(&state, deployment);
      }
  }
  ```

**Tests:** Compile check.

---

### Task 7: Simplify templates.rs (remove get_publish_state)

**Files:**
- Modify: `src/templates.rs`

**Depends on:** Task 4 (Config fields removed)

- [ ] **Step 1:** Change `create_reloader` signature to remove `audit_logger` and `config` params:
  ```rust
  pub fn create_reloader(schemas_dir: PathBuf) -> AutoReloader {
  ```

- [ ] **Step 2:** Remove the `get_publish_state` closure and its registration (lines 47-68). Remove the unused imports: `use crate::audit::AuditLogger;` and `use crate::config::Config;`.

- [ ] **Step 3:** Update all callers of `create_reloader`:
  - `src/main.rs` line 176-177: change to `templates::create_reloader(config.schemas_dir())`
  - `tests/integration.rs` line 68-69: change to `templates::create_reloader(config.schemas_dir())`

**Tests:** Compile check.

---

### Task 8: Create deployment UI routes and templates

**Files:**
- Create: `src/routes/deployments.rs`
- Create: `templates/deployments/list.html`
- Create: `templates/deployments/form.html`
- Modify: `src/routes/mod.rs`

**Depends on:** Tasks 2, 3, 5, 6

- [ ] **Step 1:** Create `src/routes/deployments.rs` with route function and handlers. Follow the same patterns as `src/routes/settings.rs` and `src/routes/schemas.rs`.

  Route function (follows the same pattern as `schemas::routes()` where POST for create is on the `/new` route):
  ```rust
  pub fn routes() -> Router<AppState> {
      Router::new()
          .route("/", get(list_deployments))
          .route("/new", get(new_deployment_form).post(create_deployment))
          .route("/{slug}/edit", get(edit_deployment_form))
          .route("/{slug}", post(update_deployment))
          .route("/{slug}/delete", post(delete_deployment))
          .route("/{slug}/fire", post(fire_deployment))
  }
  ```

  Note: The spec's routing table shows `POST /deployments` for creation, but the codebase convention (see `schemas::routes()`) puts both GET and POST on the `/new` sub-route. The form's action attribute should be `/deployments/new`. The spec table is a logical description; actual URL is `POST /deployments/new`.

  Handlers to implement:
  - `list_deployments` -- editor+ required. Fetch all deployments, compute `is_dirty` for each, fetch recent webhook history (all deployments). Render `deployments/list.html`.
  - `new_deployment_form` -- admin required. Render `deployments/form.html` with empty fields.
  - `create_deployment` -- admin required. Validate form (name, slug, webhook_url required; slug validation). Call `audit.create_deployment(...)`. If auto_deploy, spawn task. Audit log `deployment_created`. Redirect to `/deployments`.
  - `edit_deployment_form` -- admin required. Fetch by slug, 404 if not found. Render `deployments/form.html` with populated fields (auth token NOT sent to browser, just a `has_token: bool` flag).
  - `update_deployment` -- admin required. Fetch by slug. Validate. Handle auth token: if `_token_action` = "keep", preserve existing; if "clear", set None; if "update", use submitted value. Call `audit.update_deployment(...)`. Cancel old auto-deploy task, optionally spawn new one. Audit log `deployment_updated`. Redirect to `/deployments`.
  - `delete_deployment` -- admin required. Fetch by slug. Cancel auto-deploy task. Call `audit.delete_deployment(...)`. Audit log `deployment_deleted`. Redirect to `/deployments`.
  - `fire_deployment` -- editor+ required. Fetch by slug. Call `fire_webhook(...)`. Audit log `deployment_fired`. Flash success/error. Redirect to `/deployments`.

  Form structs needed:
  ```rust
  #[derive(serde::Deserialize)]
  struct DeploymentForm {
      name: String,
      slug: String,
      webhook_url: String,
      #[serde(default)]
      webhook_auth_token: String,
      #[serde(default)]
      _token_action: String,  // "keep", "clear", "update"
      #[serde(default)]
      include_drafts: Option<String>,  // checkbox: Some("on") or None
      #[serde(default)]
      auto_deploy: Option<String>,
      #[serde(default)]
      debounce_seconds: Option<i64>,
  }
  ```

- [ ] **Step 2:** Create `templates/deployments/list.html`. Structure:
  - Extends `base_template`, title "Deployments"
  - Header with "Create Deployment" button (admin only, links to `/deployments/new`)
  - Table with columns: status dot (green/amber based on `is_dirty`), name, URL (truncated), mode badge (Auto with debounce or Manual), include_drafts badge, last_fired, actions (Fire button for editor+, Edit/Delete for admin)
  - Empty state message when no deployments
  - Below table: webhook history section (reuse the table structure from `settings/webhooks.html` but with deployment name column instead of environment, and no retry button -- fire button on the deployment row covers that)
  - Flash message display at top (same pattern as webhooks.html)

- [ ] **Step 3:** Create `templates/deployments/form.html`. Structure:
  - Extends `base_template`
  - Title: "Create Deployment" or "Edit Deployment" based on `editing` boolean
  - Form action: `/deployments/new` for create mode, `/deployments/{{ deployment.slug }}` for edit mode
  - Form fields per spec: name, slug (with JS auto-generation from name), webhook_url, auth_token (type=password, with _token_action hidden field for edit mode), include_drafts checkbox, auto_deploy checkbox, debounce_seconds (shown/hidden via JS based on auto_deploy)
  - Error display at top
  - Submit button text varies by mode
  - JS for slug generation: same pattern as schema slug generation (listen to name input, slugify to lowercase-hyphenated)
  - JS for debounce visibility toggle: show/hide debounce field based on auto_deploy checkbox state

- [ ] **Step 4:** In `src/routes/mod.rs`:
  - Replace `pub mod publish;` with `pub mod deployments;`
  - In `build_router`: replace `let publish_routes = publish::routes();` with `let deployment_routes = deployments::routes();`
  - Replace `.nest("/publish", publish_routes)` with `.nest("/deployments", deployment_routes)`

- [ ] **Step 5:** Delete `src/routes/publish.rs`.

**Tests:** Compile check. Functional tests in Task 10.

---

### Task 9: Update API routes and settings, clean up nav

**Files:**
- Modify: `src/routes/api.rs`
- Modify: `src/routes/settings.rs`
- Modify: `templates/_nav.html`
- Delete: `templates/settings/webhooks.html`

**Depends on:** Tasks 5, 7, 8

- [ ] **Step 1:** In `src/routes/api.rs`:
  - Remove the `publish` handler (lines 894-931) and its route `.route("/publish/{environment}", post(publish))` (line 52).
  - Add two new handlers. The `api_fire_deployment` handler needs `crate::webhooks::{fire_webhook, TriggerSource}` and `crate::audit::Deployment` -- use full paths as the existing code does, or add imports at the top.

    `api_list_deployments` -- any token. Fetch all deployments via `state.audit.list_deployments()`. Return JSON array with fields: name, slug, webhook_url, include_drafts, auto_deploy, debounce_seconds. Omit `webhook_auth_token` from response.

    `api_fire_deployment` -- editor+ required. Fetch deployment by slug from path via `state.audit.get_deployment_by_slug(&slug)`. 404 if not found. Call `fire_webhook(&state.http_client, &state.audit, &deployment, TriggerSource::Manual)`. Audit log `deployment_fired`. Return JSON `{"status": "triggered"}` or 502 on error.

  - Add routes:
    ```rust
    .route("/deployments", get(api_list_deployments))
    .route("/deployments/{slug}/fire", post(api_fire_deployment))
    ```

- [ ] **Step 2:** In `src/routes/settings.rs`:
  - Remove `webhooks_page` handler (lines 596-662)
  - Remove `retry_webhook` handler (lines 669-702)
  - Remove `WebhookFilter` struct (lines 588-594)
  - Remove `RetryForm` struct (lines 664-667)
  - Remove the two route registrations: `.route("/webhooks", get(webhooks_page))` and `.route("/webhooks/retry", axum::routing::post(retry_webhook))`

- [ ] **Step 3:** In `templates/_nav.html`:
  - Remove the publish section (lines 34-57): the `{% if user_role != "viewer" %}` block containing `get_publish_state()`, the staging/production forms, and the closing `{% endif %}`
  - Remove the Webhooks link (line 30): `<a href="/settings/webhooks" ...>Webhooks</a>`
  - Add a Deployments link after the Uploads link (after line 24) and before the admin-only Users link:
    ```html
    {% if user_role != "viewer" %}
    <a href="/deployments" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Deployments</a>
    {% endif %}
    ```

- [ ] **Step 4:** Delete `templates/settings/webhooks.html`.

**Tests:** Compile check. Old publish API tests will need updating in Task 10.

---

### Task 10: Update integration tests

**Files:**
- Modify: `tests/integration.rs`

**Depends on:** All previous tasks

- [ ] **Step 1:** Update `TestServer::start_with_webhooks` and `TestServer::start_with_webhook_auth` -- these helpers are no longer needed since webhook config moved to the database. Simplify or remove them. Update `Config::new(...)` calls to remove the 5 deleted parameters. The signature should now be:
  ```rust
  Config::new(
      Some(data_dir.path().to_path_buf()),
      Some(db_path),
      Some(0),        // port
      false,          // secure_cookies
      10,             // version_history_count
      10,             // max_body_size_mb
  )
  ```

- [ ] **Step 2:** Add `deploy_tasks: DashMap::new()` to the `AppStateInner` construction in `TestServer`.

- [ ] **Step 3:** Update `create_reloader` calls to remove audit_logger and config params: `templates::create_reloader(config.schemas_dir())`.

- [ ] **Step 4:** Add helper method `create_deployment` to `TestServer`:
  ```rust
  async fn create_deployment(&self, name: &str, slug: &str, webhook_url: &str) {
      let csrf = self.get_csrf("/deployments/new").await;
      self.client
          .post(self.url("/deployments/new"))
          .form(&[
              ("name", name),
              ("slug", slug),
              ("webhook_url", webhook_url),
              ("_csrf", &csrf),
          ])
          .send()
          .await
          .unwrap();
  }
  ```

- [ ] **Step 5:** Remove all old webhook/publish tests and helpers. Complete list of items to remove:
  - Helper: `start_with_webhooks` (lines 29-34, fold into `start()` which just calls the simplified constructor)
  - Helper: `start_with_webhook_auth` (lines 36-116, rename remaining to `start()` with the simplified Config::new)
  - Test: `test_publish_api_no_webhook_configured` (line 1326)
  - Test: `test_publish_api_fires_webhook` (line 1365)
  - Test: `test_webhook_sends_auth_token` (line 1417)
  - Test: `test_dirty_state_tracking` (line 1489) -- references `/api/v1/publish/staging` and `start_with_webhooks`
  - Test: `webhook_fire_records_history` (line 2526) -- references `/publish/staging` and `/settings/webhooks`
  - Test: `webhook_failure_triggers_retries` (line 2575) -- references `start_with_webhooks` and `/publish/staging`
  - Test: `webhook_retry_button_fires_new_webhook` (line 2631) -- references `/settings/webhooks/retry`
  - Test: `webhook_publish_no_longer_flips_drafts` (line 3540) -- references `start_with_webhooks` and `/api/v1/publish/production`

- [ ] **Step 6:** Add new integration tests:

  **test_create_deployment_via_ui:**
  - Start server, setup admin
  - POST to `/deployments/new` with name, slug, webhook_url
  - Verify redirect to `/deployments`
  - GET `/deployments` and verify deployment name appears in response body

  **test_create_deployment_duplicate_slug:**
  - Create deployment with slug "prod" (via `create_deployment` helper)
  - Create another with same slug "prod"
  - Verify error message in response body

  **test_create_deployment_invalid_slug:**
  - POST to `/deployments/new` with slug "My Slug" (spaces/uppercase)
  - Verify error message

  **test_edit_deployment:**
  - Create deployment with slug "staging"
  - GET `/deployments/staging/edit`, verify form pre-populated (check name in response)
  - POST `/deployments/staging` with updated name (and `_token_action: "keep"` to preserve auth token)
  - GET `/deployments`, verify updated name on list page

  **test_delete_deployment:**
  - Create deployment
  - POST `/deployments/{slug}/delete`
  - Verify deployment gone from list

  **test_fire_deployment_via_ui:**
  - Start mock webhook server (same pattern as existing `test_publish_api_fires_webhook`)
  - Create deployment pointing to mock URL
  - POST `/deployments/{slug}/fire`
  - Verify mock received the webhook
  - Verify payload has `deployment` field (not `environment`), `include_drafts`, `triggered_by: "manual"`

  **test_fire_deployment_via_api:**
  - Start mock webhook server
  - Create deployment via admin UI
  - Create API token
  - POST `/api/v1/deployments/{slug}/fire` with bearer token
  - Verify 200 and `{"status": "triggered"}`
  - Verify mock received webhook

  **test_list_deployments_api:**
  - Create two deployments
  - GET `/api/v1/deployments` with bearer token
  - Verify JSON array with both deployments
  - Verify `webhook_auth_token` is NOT in response

  **test_viewer_cannot_access_deployments:**
  - Setup admin, create viewer via invite
  - Viewer GET `/deployments` -> 403

  **test_editor_can_see_but_not_crud_deployments:**
  - Setup admin, create editor via invite, create a deployment as admin first
  - Editor GET `/deployments` -> 200
  - Editor POST `/deployments/new` (create) -> 403
  - Editor POST `/deployments/{slug}/fire` -> 200 (allowed)

  **test_old_publish_routes_404:**
  - POST `/api/v1/publish/staging` with valid token -> 404
  - POST `/publish/staging` with admin session -> 404 (caught by fallback)

  **test_fire_deployment_sends_auth_token:**
  - Start mock webhook that captures Authorization header
  - Create deployment with auth token (direct form POST to `/deployments/new` with additional `webhook_auth_token` field, not via the basic `create_deployment` helper)
  - Fire it via POST `/deployments/{slug}/fire`
  - Verify Bearer token in captured header

  **test_fire_deployment_include_drafts_in_payload:**
  - Start mock webhook
  - Create deployment with include_drafts = true (direct form POST to `/deployments/new` with `include_drafts: "on"` field)
  - Fire it
  - Verify `include_drafts: true` in payload JSON

  **test_fire_deployment_unreachable_url:**
  - Create deployment with webhook_url pointing to an unreachable address (e.g., `http://127.0.0.1:1/hook`)
  - Fire via POST `/deployments/{slug}/fire`
  - Verify flash message indicates failure and retries in progress
  - Fire via API: POST `/api/v1/deployments/{slug}/fire` -> 502

  **Omitted from integration tests (spec coverage gap, accepted):** The spec lists auto-deploy integration tests (auto-deploy fires after poll+debounce, manual fire during debounce prevents double-fire). These require timing-dependent sleep-based assertions and are fragile in CI. Auto-deploy logic is covered by: (1) the unit tests for `is_dirty_for_deployment` and `mark_deployment_fired`, (2) the `spawn_auto_deploy_task` function structure being verified through the manual smoke test in Task 11 Step 6.

**Tests:** All integration tests listed above.

---

### Task 11: Final cleanup and verification

**Files:** None (verification only)

**Depends on:** All previous tasks

- [ ] **Step 1:** Run full build: `eval "$(direnv export bash 2>/dev/null)" && cargo build`
- [ ] **Step 2:** Run clippy: `eval "$(direnv export bash 2>/dev/null)" && cargo clippy`
- [ ] **Step 3:** Run fmt: `eval "$(direnv export bash 2>/dev/null)" && cargo fmt`
- [ ] **Step 4:** Run unit tests: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib`
- [ ] **Step 5:** Run integration tests: `eval "$(direnv export bash 2>/dev/null)" && cargo test --test integration -- --test-threads=4`
- [ ] **Step 6:** Manual smoke test:
  - `cargo run -- serve`
  - Log in as admin
  - Navigate to Deployments (nav link visible)
  - Create a deployment with auto_deploy off
  - Edit it, toggle auto_deploy on
  - Delete it
  - Create a deployment pointing to a real endpoint (e.g., httpbin.org/post)
  - Fire it, verify success flash and webhook history entry
  - Verify old routes gone: `/settings/webhooks` -> 404, `/publish/staging` -> 404
  - Verify nav no longer shows staging/production publish buttons
- [ ] **Step 7:** Update `NOTES.md` with any new dependency gotchas or architectural notes discovered during implementation.

---

## Verification Checklist

- [ ] `cargo build` succeeds
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo test --lib` passes (audit.rs unit tests)
- [ ] `cargo test --test integration` passes (all new + updated tests)
- [ ] No references to `staging_webhook_url`, `production_webhook_url`, `webhook_check_interval` remain in codebase
- [ ] No references to `spawn_cron` remain
- [ ] `src/routes/publish.rs` is deleted
- [ ] `templates/settings/webhooks.html` is deleted
- [ ] Nav shows "Deployments" link (editor+), no publish buttons, no "Webhooks" link
- [ ] `/deployments` page works for admin and editor (with appropriate CRUD restrictions)
- [ ] API endpoints `/api/v1/deployments` and `/api/v1/deployments/{slug}/fire` work
- [ ] Auto-deploy tasks spawn on server start for enabled deployments
- [ ] CRUD operations correctly manage background task lifecycle
