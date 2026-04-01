# Webhook Retry and History Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add automatic retry with exponential backoff for failed webhooks, record all attempts to a history table, and provide an admin UI to view history and manually retry.

**Architecture:** New `webhook_history` table in the audit DB stores every webhook attempt. `fire_webhook()` records the first attempt synchronously, returns the result, then spawns a background retry task on failure (up to 2 more attempts at 5s and 30s delays). A new `/settings/webhooks` admin page lists history grouped by `group_id` with filtering and a retry button.

**Tech Stack:** Rust/Axum, SQLite (sqlx), minijinja templates, htmx

---

## File Map

- **Create:** `audit_migrations/003_create_webhook_history.sql` — new table
- **Modify:** `src/audit.rs` — add `WebhookHistoryGroup` struct, `record_webhook_attempt()`, `list_webhook_history()` methods
- **Modify:** `src/webhooks.rs` — add `TriggerSource::Retry`, refactor `fire_webhook()` with attempt recording + background retry, remove `audit.log()` calls
- **Modify:** `src/routes/settings.rs:18-34` — add `/webhooks` and `/webhooks/retry` routes
- **Create:** `templates/settings/webhooks.html` — webhook history page
- **Modify:** `templates/_nav.html` — add "Webhooks" link (admin-only)
- **Modify:** `tests/integration.rs` — add webhook history and retry tests

---

### Task 1: Database migration and AuditLogger methods

**Files:**
- Create: `audit_migrations/003_create_webhook_history.sql`
- Modify: `src/audit.rs`

- [ ] **Step 1: Create the migration file**

Create `audit_migrations/003_create_webhook_history.sql`:

```sql
CREATE TABLE IF NOT EXISTS webhook_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    environment TEXT NOT NULL,
    trigger_source TEXT NOT NULL,
    status TEXT NOT NULL,
    http_status INTEGER,
    error_message TEXT,
    response_time_ms INTEGER,
    attempt INTEGER NOT NULL DEFAULT 1,
    group_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_webhook_history_env ON webhook_history (environment, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_webhook_history_group ON webhook_history (group_id);
```

- [ ] **Step 2: Add `WebhookHistoryGroup` struct to `src/audit.rs`**

Add after the `AuditLogger` struct definition (after line 21):

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct WebhookHistoryGroup {
    pub id: i64,
    pub environment: String,
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

- [ ] **Step 3: Add `record_webhook_attempt` method**

Add to `impl AuditLogger` (after `mark_fired`, after line 68):

```rust
pub async fn record_webhook_attempt(
    &self,
    environment: &str,
    trigger_source: &str,
    status: &str,
    http_status: Option<u16>,
    error_message: Option<&str>,
    response_time_ms: Option<i64>,
    attempt: i32,
    group_id: &str,
) -> eyre::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO webhook_history (environment, trigger_source, status, http_status, error_message, response_time_ms, attempt, group_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(environment)
    .bind(trigger_source)
    .bind(status)
    .bind(http_status.map(|s| s as i32))
    .bind(error_message)
    .bind(response_time_ms)
    .bind(attempt)
    .bind(group_id)
    .bind(&now)
    .execute(self.pool.as_ref())
    .await?;
    Ok(result.last_insert_rowid())
}
```

- [ ] **Step 4: Add `list_webhook_history` method**

Add after `record_webhook_attempt`:

```rust
pub async fn list_webhook_history(
    &self,
    environment_filter: Option<&str>,
    status_filter: Option<&str>,
) -> eyre::Result<Vec<WebhookHistoryGroup>> {
    // Get the latest attempt per group_id with attempt count
    let base = "SELECT h.id, h.environment, h.trigger_source, h.status, h.http_status, h.error_message, h.response_time_ms, g.attempt_count, h.group_id, h.created_at
        FROM webhook_history h
        INNER JOIN (
            SELECT group_id, MAX(id) AS max_id, COUNT(*) AS attempt_count
            FROM webhook_history
            GROUP BY group_id
        ) g ON h.id = g.max_id";

    let mut conditions = Vec::new();
    if environment_filter.is_some() {
        conditions.push("h.environment = ?");
    }
    if status_filter.is_some() {
        conditions.push("h.status = ?");
    }

    let query = if conditions.is_empty() {
        format!("{base} ORDER BY h.created_at DESC LIMIT 100")
    } else {
        format!("{base} WHERE {} ORDER BY h.created_at DESC LIMIT 100", conditions.join(" AND "))
    };

    let mut q = sqlx::query_as::<_, (i64, String, String, String, Option<i32>, Option<String>, Option<i64>, i32, String, String)>(&query);

    if let Some(env) = environment_filter {
        q = q.bind(env);
    }
    if let Some(status) = status_filter {
        q = q.bind(status);
    }

    let rows = q.fetch_all(self.pool.as_ref()).await?;

    Ok(rows
        .into_iter()
        .map(|(id, environment, trigger_source, status, http_status, error_message, response_time_ms, attempt_count, group_id, created_at)| {
            WebhookHistoryGroup {
                id,
                environment,
                trigger_source,
                status,
                http_status,
                error_message,
                response_time_ms,
                attempt_count,
                group_id,
                created_at,
            }
        })
        .collect())
}
```

- [ ] **Step 5: Add unit tests for the new methods**

Add to `#[cfg(test)] mod tests` in `src/audit.rs`:

```rust
#[tokio::test]
async fn test_record_webhook_attempt() {
    let pool = test_pool().await;
    let logger = AuditLogger::new(pool);
    let id = logger
        .record_webhook_attempt("staging", "manual", "success", Some(200), None, Some(150), 1, "test-group-1")
        .await
        .unwrap();
    assert!(id > 0);
}

#[tokio::test]
async fn test_list_webhook_history_grouped() {
    let pool = test_pool().await;
    let logger = AuditLogger::new(pool);

    // Two attempts in same group
    logger.record_webhook_attempt("staging", "manual", "failed", Some(500), Some("Server error"), Some(200), 1, "group-a").await.unwrap();
    logger.record_webhook_attempt("staging", "retry", "success", Some(200), None, Some(100), 2, "group-a").await.unwrap();

    // One attempt in different group
    logger.record_webhook_attempt("production", "manual", "success", Some(200), None, Some(50), 1, "group-b").await.unwrap();

    let all = logger.list_webhook_history(None, None).await.unwrap();
    assert_eq!(all.len(), 2); // two groups

    // Most recent first (group-b then group-a)
    assert_eq!(all[0].group_id, "group-b");
    assert_eq!(all[0].attempt_count, 1);
    assert_eq!(all[1].group_id, "group-a");
    assert_eq!(all[1].attempt_count, 2);
    assert_eq!(all[1].status, "success"); // latest attempt

    // Filter by environment
    let staging = logger.list_webhook_history(Some("staging"), None).await.unwrap();
    assert_eq!(staging.len(), 1);
    assert_eq!(staging[0].environment, "staging");

    // Filter by status
    let successful = logger.list_webhook_history(None, Some("success")).await.unwrap();
    assert_eq!(successful.len(), 2); // both groups ended in success
}
```

- [ ] **Step 6: Run tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: All tests pass (existing + 2 new unit tests).

- [ ] **Step 7: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add audit_migrations/003_create_webhook_history.sql src/audit.rs && git commit -m "feat: add webhook_history table and AuditLogger methods

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Refactor `fire_webhook` with history recording and background retry

**Files:**
- Modify: `src/webhooks.rs`

- [ ] **Step 1: Add `TriggerSource::Retry` variant**

In `src/webhooks.rs`, update the enum (line 15-18):

```rust
pub enum TriggerSource {
    Cron,
    Manual,
    Retry,
}
```

And update `triggered_by` match (line 46-49):

```rust
let triggered_by = match source {
    TriggerSource::Cron => "cron",
    TriggerSource::Manual => "manual",
    TriggerSource::Retry => "retry",
};
```

- [ ] **Step 2: Extract `attempt_webhook` helper**

Add a helper function that makes one HTTP attempt and returns structured result. Add before `fire_webhook`:

```rust
struct AttemptResult {
    success: bool,
    http_status: Option<u16>,
    error_message: Option<String>,
    response_time_ms: i64,
}

async fn attempt_webhook(
    client: &reqwest::Client,
    url: &str,
    auth_token: Option<&str>,
    payload: &WebhookPayload,
) -> AttemptResult {
    let start = std::time::Instant::now();
    let mut req = client.post(url).json(payload);
    if let Some(token) = auth_token {
        req = req.bearer_auth(token);
    }
    match req.send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_millis() as i64;
            if resp.status().is_success() {
                AttemptResult {
                    success: true,
                    http_status: Some(resp.status().as_u16()),
                    error_message: None,
                    response_time_ms: elapsed,
                }
            } else {
                AttemptResult {
                    success: false,
                    http_status: Some(resp.status().as_u16()),
                    error_message: Some(format!("HTTP {}", resp.status())),
                    response_time_ms: elapsed,
                }
            }
        }
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as i64;
            AttemptResult {
                success: false,
                http_status: None,
                error_message: Some(e.to_string()),
                response_time_ms: elapsed,
            }
        }
    }
}
```

- [ ] **Step 3: Rewrite `fire_webhook` with history recording and background retry**

Replace the `fire_webhook` function body (lines 22-90):

```rust
/// Fire a webhook for the given environment. Returns Ok(true) if the first attempt succeeded
/// and timestamp updated, Ok(false) if webhook URL not configured, Err if the first attempt failed
/// (background retries will continue automatically).
pub async fn fire_webhook(
    client: &reqwest::Client,
    audit: &AuditLogger,
    config: &Config,
    environment: &str,
    source: TriggerSource,
) -> Result<bool> {
    let (url, auth_token) = match environment {
        "staging" => (
            config.staging_webhook_url.as_deref(),
            config.staging_webhook_auth_token.as_deref(),
        ),
        "production" => (
            config.production_webhook_url.as_deref(),
            config.production_webhook_auth_token.as_deref(),
        ),
        _ => (None, None),
    };

    let url = match url {
        Some(u) => u,
        None => return Ok(false),
    };

    let triggered_by = match source {
        TriggerSource::Cron => "cron",
        TriggerSource::Manual => "manual",
        TriggerSource::Retry => "retry",
    };

    let payload = WebhookPayload {
        event_type: "substrukt-publish",
        environment: environment.to_string(),
        triggered_at: chrono::Utc::now().to_rfc3339(),
        triggered_by,
    };

    let group_id = uuid::Uuid::new_v4().to_string();

    // First attempt (synchronous)
    let result = attempt_webhook(client, url, auth_token, &payload).await;
    let status = if result.success { "success" } else { "failed" };

    if let Err(e) = audit
        .record_webhook_attempt(
            environment,
            triggered_by,
            status,
            result.http_status,
            result.error_message.as_deref(),
            Some(result.response_time_ms),
            1,
            &group_id,
        )
        .await
    {
        tracing::warn!("Failed to record webhook attempt: {e}");
    }

    if result.success {
        let _ = audit.mark_fired(environment).await;
        return Ok(true);
    }

    // Spawn background retries
    let client = client.clone();
    let audit = audit.clone();
    let url = url.to_string();
    let auth_token = auth_token.map(|s| s.to_string());
    let environment = environment.to_string();
    let group_id_clone = group_id.clone();

    tokio::spawn(async move {
        let delays = [
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(30),
        ];

        for (i, delay) in delays.iter().enumerate() {
            tokio::time::sleep(*delay).await;
            let attempt_num = (i + 2) as i32;

            let retry_result =
                attempt_webhook(&client, &url, auth_token.as_deref(), &payload).await;
            let retry_status = if retry_result.success { "success" } else { "failed" };

            if let Err(e) = audit
                .record_webhook_attempt(
                    &environment,
                    "retry",
                    retry_status,
                    retry_result.http_status,
                    retry_result.error_message.as_deref(),
                    Some(retry_result.response_time_ms),
                    attempt_num,
                    &group_id_clone,
                )
                .await
            {
                tracing::warn!("Failed to record webhook retry attempt: {e}");
            }

            if retry_result.success {
                let _ = audit.mark_fired(&environment).await;
                return;
            }
        }

        tracing::warn!(
            "Webhook for {} exhausted all retries (group {})",
            environment,
            group_id_clone
        );
    });

    eyre::bail!(
        "Webhook failed: {}",
        result.error_message.unwrap_or_default()
    )
}
```

- [ ] **Step 4: Verify compilation**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: No errors.

- [ ] **Step 5: Run tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: All tests pass. Existing webhook integration tests should still work since `fire_webhook` returns the same types.

- [ ] **Step 6: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/webhooks.rs && git commit -m "feat: add webhook retry with exponential backoff and history recording

First attempt is synchronous for immediate caller feedback.
On failure, spawns background retries at 5s and 30s delays.
Each attempt recorded to webhook_history table.
Removes audit_log calls for webhooks (replaced by webhook_history).

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: Webhooks settings page and routes

**Files:**
- Modify: `src/routes/settings.rs`
- Create: `templates/settings/webhooks.html`
- Modify: `templates/_nav.html`

- [ ] **Step 1: Add route registrations**

In `src/routes/settings.rs`, add two new routes to the `routes()` function (line 18-34). Add after the `/users/invitations/{id}/delete` route:

```rust
.route("/webhooks", get(webhooks_page))
.route("/webhooks/retry", axum::routing::post(retry_webhook))
```

- [ ] **Step 2: Add query params struct and webhooks page handler**

Add at the bottom of `src/routes/settings.rs` (before the closing of the file):

```rust
#[derive(serde::Deserialize, Default)]
pub struct WebhookFilter {
    #[serde(default)]
    environment: String,
    #[serde(default)]
    status: String,
}

async fn webhooks_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    axum::extract::Query(filter): axum::extract::Query<WebhookFilter>,
) -> axum::response::Result<Html<String>> {
    auth::require_role(&session, "admin").await?;
    let csrf_token = auth::ensure_csrf_token(&session).await;

    let env_filter = if filter.environment.is_empty() {
        None
    } else {
        Some(filter.environment.as_str())
    };
    let status_filter = if filter.status.is_empty() {
        None
    } else {
        Some(filter.status.as_str())
    };

    let history = state
        .audit
        .list_webhook_history(env_filter, status_filter)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let history_data: Vec<minijinja::Value> = history
        .iter()
        .map(|h| {
            minijinja::context! {
                id => h.id,
                environment => h.environment,
                trigger_source => h.trigger_source,
                status => h.status,
                http_status => h.http_status,
                error_message => h.error_message,
                response_time_ms => h.response_time_ms,
                attempt_count => h.attempt_count,
                group_id => h.group_id,
                created_at => h.created_at,
            }
        })
        .collect();

    let flash = auth::take_flash(&session).await;
    let user_role = auth::current_user_role(&session).await.unwrap_or_default();
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/webhooks.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            user_role => user_role,
            history => history_data,
            filter_environment => filter.environment,
            filter_status => filter.status,
            flash_kind => flash.as_ref().map(|(k, _)| k.as_str()),
            flash_message => flash.as_ref().map(|(_, m)| m.as_str()),
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}

#[derive(serde::Deserialize)]
pub struct RetryForm {
    environment: String,
}

async fn retry_webhook(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<RetryForm>,
) -> axum::response::Result<axum::response::Response> {
    auth::require_role(&session, "admin").await?;

    if !matches!(form.environment.as_str(), "staging" | "production") {
        auth::set_flash(&session, "error", "Invalid environment").await;
        return Ok(Redirect::to("/settings/webhooks").into_response());
    }

    match crate::webhooks::fire_webhook(
        &state.http_client,
        &state.audit,
        &state.config,
        &form.environment,
        crate::webhooks::TriggerSource::Manual,
    )
    .await
    {
        Ok(true) => {
            auth::set_flash(&session, "success", "Webhook triggered").await;
        }
        Ok(false) => {
            auth::set_flash(&session, "error", "Webhook URL not configured").await;
        }
        Err(_) => {
            auth::set_flash(&session, "error", "Webhook failed — retries in progress").await;
        }
    }

    Ok(Redirect::to("/settings/webhooks").into_response())
}
```

- [ ] **Step 3: Create the webhooks template**

Create `templates/settings/webhooks.html`:

```html
{% extends base_template %}
{% block title %}Webhooks — Substrukt{% endblock %}
{% block content %}
<h1 class="text-2xl font-bold tracking-tight mb-6">Webhooks</h1>

<div class="flex gap-3 mb-6">
  <select onchange="applyFilters()" id="filter-env"
    class="px-3 py-2 border border-border rounded-md bg-input-bg text-sm">
    <option value="">All Environments</option>
    <option value="staging" {% if filter_environment == "staging" %}selected{% endif %}>Staging</option>
    <option value="production" {% if filter_environment == "production" %}selected{% endif %}>Production</option>
  </select>
  <select onchange="applyFilters()" id="filter-status"
    class="px-3 py-2 border border-border rounded-md bg-input-bg text-sm">
    <option value="">All Statuses</option>
    <option value="success" {% if filter_status == "success" %}selected{% endif %}>Success</option>
    <option value="failed" {% if filter_status == "failed" %}selected{% endif %}>Failed</option>
  </select>
</div>

{% if history %}
<div class="bg-card border border-border-light rounded-lg overflow-hidden">
  <table class="w-full">
    <thead class="bg-card-alt">
      <tr>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Time</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Environment</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Source</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Status</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">HTTP</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Time (ms)</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Attempts</th>
        <th class="px-4 py-2.5"></th>
      </tr>
    </thead>
    <tbody class="divide-y divide-border-light">
      {% for entry in history %}
      <tr class="hover:bg-card-alt">
        <td class="px-4 py-2.5 text-sm text-muted font-mono">{{ entry.created_at[:19] }}</td>
        <td class="px-4 py-2.5 text-sm">
          <span class="px-2 py-0.5 rounded text-xs font-medium
            {% if entry.environment == 'production' %}bg-danger-soft text-danger{% else %}bg-accent-soft text-accent{% endif %}">
            {{ entry.environment }}
          </span>
        </td>
        <td class="px-4 py-2.5 text-sm text-muted">{{ entry.trigger_source }}</td>
        <td class="px-4 py-2.5 text-sm">
          {% if entry.status == "success" %}
          <span class="text-success font-medium">Success</span>
          {% else %}
          <span class="text-danger font-medium">Failed</span>
          {% endif %}
        </td>
        <td class="px-4 py-2.5 text-sm text-muted font-mono">{{ entry.http_status if entry.http_status else "—" }}</td>
        <td class="px-4 py-2.5 text-sm text-muted font-mono">{{ entry.response_time_ms if entry.response_time_ms else "—" }}</td>
        <td class="px-4 py-2.5 text-sm">
          {% if entry.attempt_count > 1 %}
          <span class="px-2 py-0.5 rounded bg-card-alt text-xs font-medium">{{ entry.attempt_count }} attempts</span>
          {% else %}
          <span class="text-muted text-xs">1</span>
          {% endif %}
        </td>
        <td class="px-4 py-2.5 text-right">
          {% if entry.status == "failed" %}
          <form method="post" action="/settings/webhooks/retry" class="inline">
            <input type="hidden" name="_csrf" value="{{ csrf_token }}">
            <input type="hidden" name="environment" value="{{ entry.environment }}">
            <button type="submit" class="text-accent hover:underline text-sm">Retry</button>
          </form>
          {% endif %}
        </td>
      </tr>
      {% if entry.status == "failed" and entry.error_message %}
      <tr class="bg-card">
        <td colspan="8" class="px-4 py-2 text-xs text-danger font-mono">{{ entry.error_message }}</td>
      </tr>
      {% endif %}
      {% endfor %}
    </tbody>
  </table>
</div>
{% else %}
<div class="bg-card border border-border-light rounded-lg p-8 text-center text-muted">
  No webhook activity yet.
</div>
{% endif %}

<script>
function applyFilters() {
  var env = document.getElementById('filter-env').value;
  var status = document.getElementById('filter-status').value;
  var params = new URLSearchParams();
  if (env) params.set('environment', env);
  if (status) params.set('status', status);
  var qs = params.toString();
  window.location.href = '/settings/webhooks' + (qs ? '?' + qs : '');
}
</script>
{% endblock %}
```

- [ ] **Step 4: Add "Webhooks" link to nav**

In `templates/_nav.html`, add the Webhooks link after the "API Tokens" link and before the admin-gated "Data" link. Find the block:

```html
  <a href="/settings/tokens" class="block px-3 py-2 rounded hover:bg-sidebar-hover">API Tokens</a>
  {% if user_role == "admin" %}
  <a href="/settings/data" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Data</a>
```

Replace with:

```html
  <a href="/settings/tokens" class="block px-3 py-2 rounded hover:bg-sidebar-hover">API Tokens</a>
  {% if user_role == "admin" %}
  <a href="/settings/webhooks" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Webhooks</a>
  <a href="/settings/data" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Data</a>
```

- [ ] **Step 5: Verify compilation**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo check`
Expected: No errors.

- [ ] **Step 6: Run tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/routes/settings.rs templates/settings/webhooks.html templates/_nav.html && git commit -m "feat: add webhook history page with filtering and retry button

Admin-only /settings/webhooks page shows webhook attempt history
grouped by fire event. Supports filtering by environment and status.
Failed entries have a retry button.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4: Integration tests

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add webhook history recording test**

Add to the end of `tests/integration.rs`:

```rust
#[tokio::test]
async fn webhook_fire_records_history() {
    // Start a mock webhook receiver
    let (webhook_tx, mut webhook_rx) = tokio::sync::mpsc::channel::<String>(10);
    let mock_app = axum::Router::new().route(
        "/webhook",
        axum::routing::post(move |body: String| {
            let tx = webhook_tx.clone();
            async move {
                tx.send(body).await.ok();
                "ok"
            }
        }),
    );
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(axum::serve(mock_listener, mock_app).into_future());
    let webhook_url = format!("http://{mock_addr}/webhook");

    let s = TestServer::start_with_webhooks(Some(webhook_url.clone()), Some(webhook_url)).await;
    s.setup_admin().await;

    // Trigger staging publish
    let csrf = s.get_csrf("/").await;
    s.client
        .post(s.url("/publish/staging"))
        .form(&[("_csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();

    // Wait for webhook to arrive
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), webhook_rx.recv())
        .await
        .unwrap();

    // Check webhooks page shows history
    let resp = s.client.get(s.url("/settings/webhooks")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("staging"));
    assert!(body.contains("Success") || body.contains("success"));
}
```

- [ ] **Step 2: Add webhook retry on failure test**

```rust
#[tokio::test]
async fn webhook_failure_triggers_retries() {
    // Mock server that fails the first request, succeeds on 2nd (first retry at 5s)
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count_clone = call_count.clone();
    let mock_app = axum::Router::new().route(
        "/webhook",
        axum::routing::post(move || {
            let count = count_clone.clone();
            async move {
                let n = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n < 1 {
                    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "fail")
                } else {
                    (axum::http::StatusCode::OK, "ok")
                }
            }
        }),
    );
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(axum::serve(mock_listener, mock_app).into_future());
    let webhook_url = format!("http://{mock_addr}/webhook");

    let s = TestServer::start_with_webhooks(Some(webhook_url.clone()), None).await;
    s.setup_admin().await;

    // Trigger publish (first attempt will fail)
    let csrf = s.get_csrf("/").await;
    let resp = s
        .client
        .post(s.url("/publish/staging"))
        .form(&[("_csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    // Should redirect (flash says failed)
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // Wait for first retry to complete (5s delay + margin)
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;

    // Should have 2 total calls (initial + first retry at 5s which succeeds)
    assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);

    // Webhooks page should show the group with multiple attempts
    let resp = s.client.get(s.url("/settings/webhooks")).send().await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("2 attempts") || body.contains("attempts"));
}
```

- [ ] **Step 3: Add retry button test**

```rust
#[tokio::test]
async fn webhook_retry_button_fires_new_webhook() {
    // Mock server that always succeeds
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count_clone = call_count.clone();
    let mock_app = axum::Router::new().route(
        "/webhook",
        axum::routing::post(move || {
            let count = count_clone.clone();
            async move {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                "ok"
            }
        }),
    );
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(axum::serve(mock_listener, mock_app).into_future());
    let webhook_url = format!("http://{mock_addr}/webhook");

    let s = TestServer::start_with_webhooks(Some(webhook_url.clone()), Some(webhook_url)).await;
    s.setup_admin().await;

    // Use the retry button to fire a webhook
    let csrf = s.get_csrf("/settings/webhooks").await;
    let resp = s
        .client
        .post(s.url("/settings/webhooks/retry"))
        .form(&[("_csrf", csrf.as_str()), ("environment", "staging")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // Webhook should have been called
    assert!(call_count.load(std::sync::atomic::Ordering::SeqCst) >= 1);

    // History page should show the entry
    let resp = s.client.get(s.url("/settings/webhooks")).send().await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("staging"));
    assert!(body.contains("Success") || body.contains("success"));
}
```

- [ ] **Step 4: Add non-admin access test**

```rust
#[tokio::test]
async fn non_admin_cannot_access_webhooks_page() {
    let s = TestServer::start().await;
    s.setup_admin().await;

    let editor = signup_user_with_role(&s, "wheditor@test.com", "wheditor", "editor").await;
    let resp = editor.get(s.url("/settings/webhooks")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
```

- [ ] **Step 5: Run all tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`
Expected: All tests pass. Note: `webhook_failure_triggers_retries` takes ~8 seconds due to retry delays.

- [ ] **Step 6: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add tests/integration.rs && git commit -m "test: add integration tests for webhook history and retry

Tests cover: history recording on fire, background retry on failure
(mock server fails once then succeeds), retry button, admin-only access.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```
