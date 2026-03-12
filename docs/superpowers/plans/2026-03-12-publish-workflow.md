# Publish Workflow Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add webhook-based staging/production publish triggers with dirty detection via the audit log.

**Architecture:** Background tokio task checks audit log for content mutations and auto-fires staging webhook. Manual publish buttons in the sidebar fire staging/production webhooks via UI and API routes. Dirty state tracked by comparing audit timestamps against `webhook_state.last_fired_at` in `audit.db`.

**Tech Stack:** Rust, Axum, SQLite (sqlx), reqwest, minijinja, htmx, tokio

**Spec:** `docs/superpowers/specs/2026-03-12-publish-workflow-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `audit_migrations/002_create_webhook_state.sql` | Create | Migration for `webhook_state` table |
| `src/audit.rs` | Modify | Add `seed_webhook_state()`, `is_dirty()`, `mark_fired()` methods |
| `src/config.rs` | Modify | Add webhook URL and interval CLI args |
| `src/webhooks.rs` | Create | Webhook firing, cron task, publish handler logic |
| `src/state.rs` | Modify | Add `reqwest::Client` to `AppStateInner` |
| `src/main.rs` | Modify | Construct reqwest client, spawn cron task |
| `src/routes/mod.rs` | Modify | Mount publish UI routes |
| `src/routes/api.rs` | Modify | Add publish API endpoints |
| `src/routes/publish.rs` | Create | UI publish route handlers |
| `src/templates.rs` | Modify | Register `get_publish_state()` global function |
| `templates/_nav.html` | Modify | Add publish buttons with dirty indicators |
| `Cargo.toml` | Modify | Add `reqwest` to `[dependencies]` |
| `tests/integration.rs` | Modify | Add publish workflow tests |

---

## Chunk 1: Data Layer

### Task 1: Add `webhook_state` migration

**Files:**
- Create: `audit_migrations/002_create_webhook_state.sql`

- [ ] **Step 1: Write migration**

```sql
CREATE TABLE IF NOT EXISTS webhook_state (
    environment TEXT PRIMARY KEY,
    last_fired_at TEXT
);

INSERT OR IGNORE INTO webhook_state (environment, last_fired_at) VALUES ('staging', NULL);
INSERT OR IGNORE INTO webhook_state (environment, last_fired_at) VALUES ('production', NULL);
```

- [ ] **Step 2: Verify migration compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: compiles (sqlx runs migrations at pool init in `audit::init_pool`)

- [ ] **Step 3: Commit**

```bash
git add audit_migrations/002_create_webhook_state.sql
git commit -m "feat: add webhook_state migration for publish workflow"
```

### Task 2: Add `is_dirty()` and `mark_fired()` to AuditLogger

**Files:**
- Modify: `src/audit.rs`

- [ ] **Step 1: Write failing test**

Create a test in `src/audit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./audit_migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn test_is_dirty_when_no_mutations() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        // No mutations exist, but last_fired_at is NULL → dirty (never built)
        assert!(logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_is_dirty_after_mutation() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        logger.mark_fired("staging").await.unwrap();
        // Insert a mutation after mark_fired
        logger.execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES (datetime('now', '+1 second'), 'test', 'content_create', 'content', 'test/1')")
            .await
            .unwrap();
        assert!(logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_not_dirty_after_mark_fired() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        // Insert a mutation
        logger.execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES (datetime('now'), 'test', 'content_create', 'content', 'test/1')")
            .await
            .unwrap();
        // Mark as fired
        logger.mark_fired("staging").await.unwrap();
        assert!(!logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_dirty_ignores_non_mutation_events() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        logger.mark_fired("staging").await.unwrap();
        // Insert a non-mutation event (login)
        logger.execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES (datetime('now', '+1 second'), 'test', 'login', 'session', '')")
            .await
            .unwrap();
        assert!(!logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_staging_and_production_independent() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        // Insert a mutation
        logger.execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES (datetime('now'), 'test', 'content_create', 'content', 'test/1')")
            .await
            .unwrap();
        // Mark only staging as fired
        logger.mark_fired("staging").await.unwrap();
        assert!(!logger.is_dirty("staging").await.unwrap());
        // Production should still be dirty (never fired)
        assert!(logger.is_dirty("production").await.unwrap());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib audit::tests -- --nocapture`
Expected: FAIL — `is_dirty` and `mark_fired` don't exist

- [ ] **Step 3: Implement `is_dirty()` and `mark_fired()`**

Add to `src/audit.rs` in the `impl AuditLogger` block:

```rust
const DATA_MUTATION_ACTIONS: &[&str] = &[
    "content_create",
    "content_update",
    "content_delete",
    "schema_create",
    "schema_update",
    "schema_delete",
];

/// Execute a raw query on the audit pool. Used by tests to insert test data.
#[cfg(test)]
pub async fn execute_raw(&self, query: &str) -> eyre::Result<()> {
    sqlx::query(query).execute(self.pool.as_ref()).await?;
    Ok(())
}

pub async fn is_dirty(&self, environment: &str) -> eyre::Result<bool> {
    let last_fired: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT last_fired_at FROM webhook_state WHERE environment = ?"
    )
    .bind(environment)
    .fetch_optional(self.pool.as_ref())
    .await?;

    let last_fired_at = match last_fired {
        Some((Some(ts),)) => ts,
        _ => return Ok(true), // NULL or missing → never built → dirty
    };

    let latest_mutation: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT MAX(timestamp) FROM audit_log WHERE action IN ('content_create', 'content_update', 'content_delete', 'schema_create', 'schema_update', 'schema_delete')"
    )
    .fetch_one(self.pool.as_ref())
    .await?;

    match latest_mutation {
        Some((Some(ts),)) => Ok(ts > last_fired_at),
        _ => Ok(false), // No mutations at all → clean
    }
}

pub async fn mark_fired(&self, environment: &str) -> eyre::Result<String> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE webhook_state SET last_fired_at = ? WHERE environment = ?")
        .bind(&now)
        .bind(environment)
        .execute(self.pool.as_ref())
        .await?;
    Ok(now)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib audit::tests -- --nocapture`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/audit.rs
git commit -m "feat: add is_dirty() and mark_fired() to AuditLogger"
```

---

## Chunk 2: Config & State

### Task 3: Add webhook config to CLI args

**Files:**
- Modify: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add webhook fields to Config**

In `src/config.rs`, add fields to `Config`:

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub listen_addr: String,
    pub listen_port: u16,
    pub secure_cookies: bool,
    pub staging_webhook_url: Option<String>,
    pub production_webhook_url: Option<String>,
    pub webhook_check_interval: u64, // seconds
}
```

Update `Config::new()` to accept and store these:

```rust
pub fn new(
    data_dir: Option<PathBuf>,
    db_path: Option<PathBuf>,
    port: Option<u16>,
    secure_cookies: bool,
    staging_webhook_url: Option<String>,
    production_webhook_url: Option<String>,
    webhook_check_interval: Option<u64>,
) -> Self {
    let data_dir = data_dir.unwrap_or_else(|| PathBuf::from("data"));
    let db_path = db_path.unwrap_or_else(|| data_dir.join("substrukt.db"));
    Self {
        data_dir,
        db_path,
        listen_addr: "0.0.0.0".to_string(),
        listen_port: port.unwrap_or(3000),
        secure_cookies,
        staging_webhook_url,
        production_webhook_url,
        webhook_check_interval: webhook_check_interval.unwrap_or(300),
    }
}
```

- [ ] **Step 2: Add CLI args in `src/main.rs`**

Add to the `Cli` struct:

```rust
#[arg(long, global = true)]
staging_webhook_url: Option<String>,

#[arg(long, global = true)]
production_webhook_url: Option<String>,

#[arg(long, global = true, default_value = "300")]
webhook_check_interval: Option<u64>,
```

Update the `Config::new()` call in `main()`:

```rust
let config = Config::new(
    cli.data_dir,
    cli.db_path,
    cli.port,
    cli.secure_cookies,
    cli.staging_webhook_url,
    cli.production_webhook_url,
    cli.webhook_check_interval,
);
```

- [ ] **Step 3: Fix any other `Config::new()` call sites**

Search for `Config::new(` in `tests/integration.rs` and update to pass the new params:

```rust
let config = Config::new(
    Some(data_dir.path().to_path_buf()),
    Some(db_path),
    Some(0),
    false,
    None, // staging_webhook_url
    None, // production_webhook_url
    None, // webhook_check_interval
);
```

- [ ] **Step 4: Verify it compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs tests/integration.rs
git commit -m "feat: add webhook URL and interval config options"
```

### Task 4: Add `reqwest::Client` to AppState

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/state.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add reqwest dependency**

In `Cargo.toml`, add to `[dependencies]`:

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

- [ ] **Step 2: Add `http_client` to AppStateInner**

In `src/state.rs`, add:

```rust
pub struct AppStateInner {
    pub pool: SqlitePool,
    pub config: Config,
    pub templates: AutoReloader,
    pub cache: ContentCache,
    pub login_limiter: RateLimiter,
    pub api_limiter: RateLimiter,
    pub metrics_handle: PrometheusHandle,
    pub audit: AuditLogger,
    pub http_client: reqwest::Client,
}
```

- [ ] **Step 3: Construct client in `main.rs`**

In `run_server()`, before constructing `AppStateInner`:

```rust
let http_client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(10))
    .user_agent("Substrukt/0.1")
    .build()?;
```

Add `http_client` to the `AppStateInner` construction.

- [ ] **Step 4: Update test server construction**

In `tests/integration.rs`, add `http_client: reqwest::Client::new()` to the `AppStateInner` construction.

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check && cargo test`
Expected: compiles, all existing tests pass

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/state.rs src/main.rs tests/integration.rs
git commit -m "feat: add shared reqwest::Client to AppState"
```

---

## Chunk 3: Webhook Module

### Task 5: Create `src/webhooks.rs` — webhook firing and cron

**Files:**
- Create: `src/webhooks.rs`
- Modify: `src/lib.rs` (add `pub mod webhooks;`)

- [ ] **Step 1: Write the webhook module**

Create `src/webhooks.rs`:

```rust
use std::sync::Arc;

use eyre::Result;
use serde::Serialize;

use crate::audit::AuditLogger;
use crate::config::Config;

#[derive(Serialize)]
struct WebhookPayload {
    event_type: &'static str,
    environment: String,
    triggered_at: String,
    triggered_by: String,
}

pub enum TriggerSource {
    Cron,
    Manual,
}

/// Fire a webhook for the given environment. Returns Ok(true) if fired and timestamp updated,
/// Ok(false) if webhook URL not configured, Err if the HTTP call failed.
pub async fn fire_webhook(
    client: &reqwest::Client,
    audit: &AuditLogger,
    config: &Config,
    environment: &str,
    source: TriggerSource,
) -> Result<bool> {
    let url = match environment {
        "staging" => config.staging_webhook_url.as_deref(),
        "production" => config.production_webhook_url.as_deref(),
        _ => None,
    };

    let url = match url {
        Some(u) => u,
        None => return Ok(false),
    };

    let payload = WebhookPayload {
        event_type: "substrukt-publish",
        environment: environment.to_string(),
        triggered_at: chrono::Utc::now().to_rfc3339(),
        triggered_by: match source {
            TriggerSource::Cron => "cron".to_string(),
            TriggerSource::Manual => "manual".to_string(),
        },
    };

    let resp = client.post(url).json(&payload).send().await?;

    if resp.status().is_success() {
        audit.mark_fired(environment).await?;
        audit.log(
            "system",
            "webhook_fire",
            "webhook",
            environment,
            Some(&serde_json::json!({"status": "success", "triggered_by": &payload.triggered_by}).to_string()),
        );
        Ok(true)
    } else {
        let status = resp.status();
        audit.log(
            "system",
            "webhook_fire",
            "webhook",
            environment,
            Some(&serde_json::json!({"status": "failed", "http_status": status.as_u16()}).to_string()),
        );
        eyre::bail!("Webhook returned HTTP {status}")
    }
}

/// Spawn the background cron task that auto-fires the staging webhook when dirty.
pub fn spawn_cron(
    client: reqwest::Client,
    audit: AuditLogger,
    config: Config,
) {
    let interval = std::time::Duration::from_secs(config.webhook_check_interval);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;

            if config.staging_webhook_url.is_none() {
                continue;
            }

            match audit.is_dirty("staging").await {
                Ok(true) => {
                    tracing::info!("Staging is dirty, firing webhook");
                    if let Err(e) = fire_webhook(&client, &audit, &config, "staging", TriggerSource::Cron).await {
                        tracing::warn!("Staging webhook failed: {e}");
                    }
                }
                Ok(false) => {
                    tracing::debug!("Staging is clean, skipping webhook");
                }
                Err(e) => {
                    tracing::warn!("Failed to check staging dirty state: {e}");
                }
            }
        }
    });
}
```

- [ ] **Step 2: Register module in `src/lib.rs`**

Add `pub mod webhooks;` to `src/lib.rs` (where all other modules are declared).

- [ ] **Step 3: Spawn cron in `run_server()`**

In `src/main.rs`, after constructing `state` and before building the router:

```rust
webhooks::spawn_cron(
    state.http_client.clone(),
    state.audit.clone(),
    state.config.clone(),
);
```

- [ ] **Step 4: Verify it compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/webhooks.rs src/lib.rs src/main.rs
git commit -m "feat: add webhook module with fire and cron logic"
```

---

## Chunk 4: API & UI Routes

### Task 6: Add publish API endpoints

**Files:**
- Modify: `src/routes/api.rs`

- [ ] **Step 1: Add publish handlers to `api.rs`**

Add two handler functions:

```rust
async fn publish(
    State(state): State<AppState>,
    _token: BearerToken,
    Path(environment): Path<String>,
) -> impl IntoResponse {
    if !matches!(environment.as_str(), "staging" | "production") {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Unknown environment"}))).into_response();
    }

    match crate::webhooks::fire_webhook(
        &state.http_client,
        &state.audit,
        &state.config,
        &environment,
        crate::webhooks::TriggerSource::Manual,
    ).await {
        Ok(true) => Json(serde_json::json!({"status": "triggered"})).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Webhook URL not configured"}))).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
```

- [ ] **Step 2: Register the route**

In the `pub fn routes()` function in `api.rs`, add:

```rust
.route("/publish/{environment}", axum::routing::post(publish))
```

- [ ] **Step 3: Verify it compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/routes/api.rs
git commit -m "feat: add POST /api/v1/publish/{environment} endpoint"
```

### Task 7: Add publish UI routes

**Files:**
- Create: `src/routes/publish.rs`
- Modify: `src/routes/mod.rs`

- [ ] **Step 1: Create `src/routes/publish.rs`**

```rust
use axum::extract::State;
use axum::extract::Path;
use axum::response::{IntoResponse, Redirect};
use tower_sessions::Session;

use crate::auth;
use crate::state::AppState;

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{environment}", axum::routing::post(publish))
}

async fn publish(
    State(state): State<AppState>,
    session: Session,
    Path(environment): Path<String>,
) -> impl IntoResponse {
    if !matches!(environment.as_str(), "staging" | "production") {
        return Redirect::to("/").into_response();
    }

    let label = if environment == "staging" { "Staging build" } else { "Production publish" };

    match crate::webhooks::fire_webhook(
        &state.http_client,
        &state.audit,
        &state.config,
        &environment,
        crate::webhooks::TriggerSource::Manual,
    ).await {
        Ok(true) => {
            auth::set_flash(&session, "success", &format!("{label} triggered")).await;
        }
        Ok(false) => {
            auth::set_flash(&session, "error", "Webhook URL not configured").await;
        }
        Err(e) => {
            tracing::warn!("Webhook failed for {environment}: {e}");
            auth::set_flash(&session, "error", "Webhook failed — check configuration").await;
        }
    }

    Redirect::to("/").into_response()
}
```

- [ ] **Step 2: Register routes in `src/routes/mod.rs`**

Add `pub mod publish;` at the top.

In `build_router()`, add the publish routes inside the auth-protected group (after `.nest("/settings", settings_routes)` and before `.route("/", ...)`):

```rust
let publish_routes = publish::routes();
// ...
.nest("/publish", publish_routes)
```

- [ ] **Step 3: Verify it compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/routes/publish.rs src/routes/mod.rs
git commit -m "feat: add UI publish routes with flash messages"
```

---

## Chunk 5: UI

### Task 8: Add publish buttons to sidebar nav

**Files:**
- Modify: `src/templates.rs`
- Modify: `templates/_nav.html`

- [ ] **Step 1: Register `get_publish_state()` template function**

In `src/templates.rs`, inside `create_reloader()`, after the `get_nav_schemas` function registration, add:

```rust
let audit_for_tpl = audit_logger.clone();
let config_for_tpl = config.clone();
env.add_function("get_publish_state", move || -> minijinja::Value {
    // Bridge async into sync context for minijinja.
    // Must use block_in_place (not block_on) because we're inside a tokio async context.
    // block_in_place tells the runtime this thread will block, allowing other tasks to proceed.
    let audit = audit_for_tpl.clone();
    let (staging_dirty, production_dirty) = tokio::task::block_in_place(|| {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(async {
            let s = audit.is_dirty("staging").await.unwrap_or(false);
            let p = audit.is_dirty("production").await.unwrap_or(false);
            (s, p)
        })
    });
    minijinja::context! {
        staging_configured => config_for_tpl.staging_webhook_url.is_some(),
        production_configured => config_for_tpl.production_webhook_url.is_some(),
        staging_dirty => staging_dirty,
        production_dirty => production_dirty,
    }
});
```

**Important**: This requires the tokio runtime to use the multi-thread scheduler (which is the default with `#[tokio::main]` and `tokio = { features = ["full"] }`). `block_in_place` panics on the single-thread (`current_thread`) scheduler. The existing `main.rs` uses `#[tokio::main]` which defaults to multi-thread, so this is safe. However, **tests using `#[tokio::test]` default to `current_thread`**. The test server in `tests/integration.rs` must use `#[tokio::test(flavor = "multi_thread")]` for any test that renders templates with `get_publish_state()`. Since the existing tests don't configure webhook URLs, `get_publish_state` will return all-false without hitting `block_in_place` for the dirty queries — but to be safe, update the test attribute for webhook tests to `#[tokio::test(flavor = "multi_thread")]`.

This requires updating `create_reloader()` signature to accept `AuditLogger` and `Config`:

```rust
pub fn create_reloader(schemas_dir: PathBuf, audit_logger: AuditLogger, config: Config) -> AutoReloader {
```

Update the call in `src/main.rs`:

```rust
let reloader = templates::create_reloader(config.schemas_dir(), audit_logger.clone(), config.clone());
```

Note: `audit_logger` must be created before `reloader` in `run_server()`. Currently `audit_logger` is created before `reloader`, so no reordering needed.

Also update the `create_reloader` call in `tests/integration.rs` (in `TestServer::start_with_webhooks`):

```rust
let reloader = templates::create_reloader(config.schemas_dir(), audit_logger.clone(), config.clone());
```

- [ ] **Step 2: Update `templates/_nav.html`**

Add publish section before the logout form (before the `<div class="mt-auto ...">` block):

```html
{% set publish = get_publish_state() %}
{% if publish.staging_configured or publish.production_configured %}
<div class="mt-3 mb-1 px-3 text-xs font-semibold text-gray-500 uppercase tracking-wider">Publish</div>
{% if publish.staging_configured %}
<form method="post" action="/publish/staging" class="px-1">
  <input type="hidden" name="_csrf" value="{{ csrf_token }}">
  <button type="submit" class="w-full flex items-center gap-2 px-2 py-2 rounded hover:bg-gray-700 text-sm text-left">
    <span class="w-2 h-2 rounded-full {% if publish.staging_dirty %}bg-amber-400{% else %}bg-green-400{% endif %}"></span>
    Build Staging
  </button>
</form>
{% endif %}
{% if publish.production_configured %}
<form method="post" action="/publish/production" class="px-1">
  <input type="hidden" name="_csrf" value="{{ csrf_token }}">
  <button type="submit" class="w-full flex items-center gap-2 px-2 py-2 rounded hover:bg-gray-700 text-sm text-left">
    <span class="w-2 h-2 rounded-full {% if publish.production_dirty %}bg-amber-400{% else %}bg-green-400{% endif %}"></span>
    Publish Production
  </button>
</form>
{% endif %}
{% endif %}
```

- [ ] **Step 3: Verify it compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: compiles

- [ ] **Step 4: Run existing tests to make sure nothing breaks**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: all existing tests pass

- [ ] **Step 5: Commit**

```bash
git add src/templates.rs templates/_nav.html src/main.rs
git commit -m "feat: add publish buttons with dirty indicators to sidebar nav"
```

---

## Chunk 6: Integration Tests

### Task 9: Test publish API endpoints

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Write integration test for publish API**

Add test that exercises the publish API endpoints. Since we can't set up a real webhook receiver in tests, we test the "not configured" case (webhook URLs are `None` in tests):

```rust
#[tokio::test]
async fn test_publish_api_no_webhook_configured() {
    let s = TestServer::start().await;
    s.setup_admin().await;
    let token = s.create_api_token("publish-test").await;

    let api = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Staging publish should 404 when no webhook URL configured
    let resp = api
        .post(s.url("/api/v1/publish/staging"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Production publish should 404 when no webhook URL configured
    let resp = api
        .post(s.url("/api/v1/publish/production"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Unknown environment should 404
    let resp = api
        .post(s.url("/api/v1/publish/unknown"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Write integration test with mock webhook server**

Add a test that starts a local HTTP server as the webhook target:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn test_publish_api_fires_webhook() {
    // Start a mock webhook receiver
    let (webhook_tx, mut webhook_rx) = tokio::sync::mpsc::channel::<String>(1);
    let mock_app = axum::Router::new().route("/webhook", axum::routing::post(move |body: String| {
        let tx = webhook_tx.clone();
        async move {
            let _ = tx.send(body).await;
            "ok"
        }
    }));
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(mock_listener, mock_app).await.unwrap() });

    let webhook_url = format!("http://{mock_addr}/webhook");

    // Start test server with webhook URL configured
    let s = TestServer::start_with_webhooks(Some(webhook_url.clone()), Some(webhook_url.clone())).await;
    s.setup_admin().await;
    let token = s.create_api_token("webhook-test").await;

    // Create some content first so there's something to publish
    s.create_schema(BLOG_SCHEMA).await;

    let api = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Fire staging publish
    let resp = api
        .post(s.url("/api/v1/publish/staging"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "triggered");

    // Verify webhook was received
    let payload = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        webhook_rx.recv(),
    ).await.unwrap().unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["event_type"], "substrukt-publish");
    assert_eq!(payload["environment"], "staging");
    assert_eq!(payload["triggered_by"], "manual");
}
```

This requires refactoring `TestServer`. Replace `start()` with `start_with_webhooks()` and have `start()` delegate:

```rust
impl TestServer {
    async fn start() -> Self {
        Self::start_with_webhooks(None, None).await
    }

    async fn start_with_webhooks(
        staging_webhook_url: Option<String>,
        production_webhook_url: Option<String>,
    ) -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let db_path = data_dir.path().join("test.db");
        let config = Config::new(
            Some(data_dir.path().to_path_buf()),
            Some(db_path),
            Some(0),
            false,
            staging_webhook_url,
            production_webhook_url,
            Some(3600), // long interval so cron doesn't interfere with tests
        );
        config.ensure_dirs().unwrap();

        let pool = db::init_pool(&config.db_path).await.unwrap();
        let session_store = SqliteStore::new(pool.clone());
        session_store.migrate().await.unwrap();
        let session_layer = SessionManagerLayer::new(session_store).with_secure(false);

        let audit_db_path = data_dir.path().join("audit.db");
        let audit_pool = substrukt::audit::init_pool(&audit_db_path).await.unwrap();
        let audit_logger = substrukt::audit::AuditLogger::new(audit_pool);

        let reloader = templates::create_reloader(config.schemas_dir(), audit_logger.clone(), config.clone());
        let content_cache = DashMap::new();
        cache::populate(&content_cache, &config.schemas_dir(), &config.content_dir());

        let metrics_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .build_recorder()
            .handle();

        let state = Arc::new(AppStateInner {
            pool,
            config,
            templates: reloader,
            cache: content_cache,
            login_limiter: RateLimiter::new(100, std::time::Duration::from_secs(60)),
            api_limiter: RateLimiter::new(1000, std::time::Duration::from_secs(60)),
            metrics_handle,
            audit: audit_logger,
            http_client: reqwest::Client::new(),
        });

        let app = routes::build_router(state).layer(session_layer);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async { rx.await.ok(); })
                .await
                .unwrap();
        });

        let client = Client::builder()
            .cookie_store(true)
            .redirect(redirect::Policy::none())
            .build()
            .unwrap();

        TestServer { base_url, client, _data_dir: data_dir, _shutdown: tx }
    }
}
```

Note: audit_logger must be created before reloader since `create_reloader` now takes it as a parameter.

- [ ] **Step 3: Run all tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: all tests pass including new ones

- [ ] **Step 4: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add integration tests for publish webhook endpoints"
```

### Task 10: Test dirty detection end-to-end

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Write dirty detection integration test**

```rust
#[tokio::test(flavor = "multi_thread")]
async fn test_dirty_state_tracking() {
    let (webhook_tx, mut webhook_rx) = tokio::sync::mpsc::channel::<String>(10);
    let mock_app = axum::Router::new().route("/webhook", axum::routing::post(move |body: String| {
        let tx = webhook_tx.clone();
        async move {
            let _ = tx.send(body).await;
            "ok"
        }
    }));
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(mock_listener, mock_app).await.unwrap() });

    let webhook_url = format!("http://{mock_addr}/webhook");
    let s = TestServer::start_with_webhooks(Some(webhook_url.clone()), Some(webhook_url)).await;
    s.setup_admin().await;
    let token = s.create_api_token("dirty-test").await;

    let api = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Create schema and content (creates audit entries)
    s.create_schema(BLOG_SCHEMA).await;

    // Publish staging
    let resp = api.post(s.url("/api/v1/publish/staging")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = webhook_rx.recv().await; // consume webhook

    // Publish again — should still fire (buttons always fire)
    let resp = api.post(s.url("/api/v1/publish/staging")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = webhook_rx.recv().await;

    // Production was never published — fires too
    let resp = api.post(s.url("/api/v1/publish/production")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = webhook_rx.recv().await.unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["environment"], "production");
}
```

- [ ] **Step 2: Run all tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add dirty state tracking integration test"
```

---

## Chunk 7: Cleanup

### Task 11: Format and lint

- [ ] **Step 1: Run cargo fmt**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo fmt`

- [ ] **Step 2: Run cargo clippy**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo clippy -- -D warnings`
Fix any warnings.

- [ ] **Step 3: Run full test suite**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit if any formatting changes**

```bash
git add -A
git commit -m "style: apply cargo fmt and clippy fixes"
```
