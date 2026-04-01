# Audit Log Viewer Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an admin-only audit log viewer page at `/settings/audit-log` with filtering and pagination.

**Architecture:** New query methods on `AuditLogger`, a new handler in `src/routes/settings.rs`, and a new template following the webhooks page pattern. Admin-only access, server-side filtering via query params, offset pagination with 100 entries per page.

**Tech Stack:** Rust, Axum, SQLite (sqlx), minijinja templates, twind CSS

**Spec:** `docs/superpowers/specs/2026-03-18-audit-log-viewer-design.md`

---

### Task 1: Data layer — `AuditLogEntry` struct and `list_audit_log` method

**Files:**
- Modify: `src/audit.rs`

- [ ] **Step 1: Write the failing tests for `list_audit_log`**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `src/audit.rs` (after the existing tests, around line 399):

```rust
#[tokio::test]
async fn test_list_audit_log_order_and_basic() {
    let pool = test_pool().await;
    let logger = AuditLogger::new(pool);

    // Insert entries directly (not via log() since that's fire-and-forget)
    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id, details) VALUES ('2026-01-01T00:00:00Z', 'user1', 'content_create', 'content', 'posts/1', '{\"title\":\"Hello\"}')")
        .await
        .unwrap();
    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id, details) VALUES ('2026-01-02T00:00:00Z', 'user2', 'login', 'session', '', NULL)")
        .await
        .unwrap();

    let (entries, has_next) = logger.list_audit_log(None, None, 1).await.unwrap();
    assert_eq!(entries.len(), 2);
    assert!(!has_next);
    // Reverse chronological
    assert_eq!(entries[0].action, "login");
    assert_eq!(entries[0].actor, "user2");
    assert_eq!(entries[1].action, "content_create");
    assert_eq!(entries[1].actor, "user1");
    assert_eq!(entries[1].details, Some("{\"title\":\"Hello\"}".to_string()));
    assert_eq!(entries[0].details, None);
}

#[tokio::test]
async fn test_list_audit_log_action_filter() {
    let pool = test_pool().await;
    let logger = AuditLogger::new(pool);

    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-01T00:00:00Z', 'user1', 'content_create', 'content', 'posts/1')")
        .await
        .unwrap();
    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-02T00:00:00Z', 'user1', 'login', 'session', '')")
        .await
        .unwrap();

    let (entries, _) = logger.list_audit_log(Some("login"), None, 1).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, "login");
}

#[tokio::test]
async fn test_list_audit_log_actor_filter() {
    let pool = test_pool().await;
    let logger = AuditLogger::new(pool);

    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-01T00:00:00Z', 'user1', 'content_create', 'content', 'posts/1')")
        .await
        .unwrap();
    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-02T00:00:00Z', 'user2', 'login', 'session', '')")
        .await
        .unwrap();

    let (entries, _) = logger.list_audit_log(None, Some("user1"), 1).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor, "user1");
}

#[tokio::test]
async fn test_list_audit_log_pagination() {
    let pool = test_pool().await;
    let logger = AuditLogger::new(pool);

    // Insert 105 entries
    for i in 0..105 {
        let ts = format!("2026-01-01T{:02}:{:02}:00Z", i / 60, i % 60);
        let query = format!(
            "INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('{ts}', 'user1', 'login', 'session', '')"
        );
        logger.execute_raw(&query).await.unwrap();
    }

    let (page1, has_next1) = logger.list_audit_log(None, None, 1).await.unwrap();
    assert_eq!(page1.len(), 100);
    assert!(has_next1);

    let (page2, has_next2) = logger.list_audit_log(None, None, 2).await.unwrap();
    assert_eq!(page2.len(), 5);
    assert!(!has_next2);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test test_list_audit_log -- --nocapture`

Expected: compilation errors — `list_audit_log` method does not exist yet.

- [ ] **Step 3: Implement `AuditLogEntry` struct and `list_audit_log` method**

Add the `AuditLogEntry` struct after `WebhookHistoryGroup` (around line 35 in `src/audit.rs`):

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub timestamp: String,
    pub actor: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub details: Option<String>,
}
```

Add `list_audit_log` method inside `impl AuditLogger`, after `list_webhook_history` (around line 198):

```rust
pub async fn list_audit_log(
    &self,
    action_filter: Option<&str>,
    actor_filter: Option<&str>,
    page: u32,
) -> eyre::Result<(Vec<AuditLogEntry>, bool)> {
    let page = page.max(1);
    let offset = (page - 1) as i64 * 100;
    let base = "SELECT id, timestamp, actor, action, resource_type, resource_id, details FROM audit_log";

    let mut conditions = Vec::new();
    if action_filter.is_some() {
        conditions.push("action = ?");
    }
    if actor_filter.is_some() {
        conditions.push("actor = ?");
    }

    let query = if conditions.is_empty() {
        format!("{base} ORDER BY timestamp DESC, id DESC LIMIT 101 OFFSET ?")
    } else {
        format!(
            "{base} WHERE {} ORDER BY timestamp DESC, id DESC LIMIT 101 OFFSET ?",
            conditions.join(" AND ")
        )
    };

    let mut q = sqlx::query_as::<_, (i64, String, String, String, String, String, Option<String>)>(&query);

    if let Some(action) = action_filter {
        q = q.bind(action);
    }
    if let Some(actor) = actor_filter {
        q = q.bind(actor);
    }
    q = q.bind(offset);

    let rows = q.fetch_all(self.pool.as_ref()).await?;
    let has_next = rows.len() > 100;
    let entries: Vec<AuditLogEntry> = rows
        .into_iter()
        .take(100)
        .map(|(id, timestamp, actor, action, resource_type, resource_id, details)| {
            AuditLogEntry { id, timestamp, actor, action, resource_type, resource_id, details }
        })
        .collect();

    Ok((entries, has_next))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test test_list_audit_log -- --nocapture`

Expected: all 4 new tests pass.

- [ ] **Step 5: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/audit.rs && git commit -m "feat: add list_audit_log query method with filtering and pagination"
```

---

### Task 2: Data layer — `list_audit_actors` method

**Files:**
- Modify: `src/audit.rs`

- [ ] **Step 1: Write the failing test for `list_audit_actors`**

Add this test in the `#[cfg(test)] mod tests` block:

```rust
#[tokio::test]
async fn test_list_audit_actors() {
    let pool = test_pool().await;
    let logger = AuditLogger::new(pool);

    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-01T00:00:00Z', 'zara', 'login', 'session', '')")
        .await
        .unwrap();
    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-02T00:00:00Z', 'alice', 'login', 'session', '')")
        .await
        .unwrap();
    logger
        .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-03T00:00:00Z', 'alice', 'logout', 'session', '')")
        .await
        .unwrap();

    let actors = logger.list_audit_actors().await.unwrap();
    assert_eq!(actors, vec!["alice", "zara"]);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test test_list_audit_actors -- --nocapture`

Expected: compilation error — `list_audit_actors` method does not exist.

- [ ] **Step 3: Implement `list_audit_actors`**

Add inside `impl AuditLogger`, after `list_audit_log`:

```rust
pub async fn list_audit_actors(&self) -> eyre::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT actor FROM audit_log ORDER BY actor")
            .fetch_all(self.pool.as_ref())
            .await?;
    Ok(rows.into_iter().map(|(actor,)| actor).collect())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test test_list_audit_actors -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/audit.rs && git commit -m "feat: add list_audit_actors query method"
```

---

### Task 3: Route handler and template

**Files:**
- Modify: `src/routes/settings.rs`
- Create: `templates/settings/audit_log.html`

- [ ] **Step 1: Add the `AuditLogFilter` struct and `audit_log_page` handler**

In `src/routes/settings.rs`, add the route to the router (inside `pub fn routes()`), after the webhooks routes (around line 35):

```rust
.route("/audit-log", get(audit_log_page))
```

Add the filter struct after `WebhookFilter` (around line 593):

```rust
#[derive(serde::Deserialize, Default)]
pub struct AuditLogFilter {
    #[serde(default)]
    action: String,
    #[serde(default)]
    actor: String,
    #[serde(default)]
    page: String,
}
```

Add the handler after `retry_webhook` (around line 701):

```rust
async fn audit_log_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    axum::extract::Query(filter): axum::extract::Query<AuditLogFilter>,
) -> axum::response::Result<Html<String>> {
    auth::require_role(&session, "admin").await?;

    let page: u32 = filter.page.parse().unwrap_or(1).max(1);

    let action_filter = if filter.action.is_empty() {
        None
    } else {
        Some(filter.action.as_str())
    };
    let actor_filter = if filter.actor.is_empty() {
        None
    } else {
        Some(filter.actor.as_str())
    };

    let (entries, has_next) = state
        .audit
        .list_audit_log(action_filter, actor_filter, page)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let actors = state
        .audit
        .list_audit_actors()
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let entry_data: Vec<minijinja::Value> = entries
        .iter()
        .map(|e| {
            minijinja::context! {
                id => e.id,
                timestamp => e.timestamp,
                actor => e.actor,
                action => e.action,
                resource_type => e.resource_type,
                resource_id => e.resource_id,
                details => e.details,
            }
        })
        .collect();

    let user_role = auth::current_user_role(&session).await.unwrap_or_default();
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/audit_log.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            user_role => user_role,
            entries => entry_data,
            actors => actors,
            filter_action => filter.action,
            filter_actor => filter.actor,
            page => page,
            has_next => has_next,
            has_prev => page > 1,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}
```

- [ ] **Step 2: Create the template**

Create `templates/settings/audit_log.html`:

```html
{% extends base_template %}
{% block title %}Audit Log — Substrukt{% endblock %}
{% block content %}
<h1 class="text-2xl font-bold tracking-tight mb-6">Audit Log</h1>

<div class="flex gap-3 mb-6">
  <select onchange="applyFilters()" id="filter-action"
    class="px-3 py-2 border border-border rounded-md bg-input-bg text-sm">
    <option value="">All Actions</option>
    <option value="content_create" {% if filter_action == "content_create" %}selected{% endif %}>content_create</option>
    <option value="content_update" {% if filter_action == "content_update" %}selected{% endif %}>content_update</option>
    <option value="content_delete" {% if filter_action == "content_delete" %}selected{% endif %}>content_delete</option>
    <option value="schema_create" {% if filter_action == "schema_create" %}selected{% endif %}>schema_create</option>
    <option value="schema_update" {% if filter_action == "schema_update" %}selected{% endif %}>schema_update</option>
    <option value="schema_delete" {% if filter_action == "schema_delete" %}selected{% endif %}>schema_delete</option>
    <option value="login" {% if filter_action == "login" %}selected{% endif %}>login</option>
    <option value="logout" {% if filter_action == "logout" %}selected{% endif %}>logout</option>
    <option value="user_create" {% if filter_action == "user_create" %}selected{% endif %}>user_create</option>
    <option value="token_create" {% if filter_action == "token_create" %}selected{% endif %}>token_create</option>
    <option value="token_delete" {% if filter_action == "token_delete" %}selected{% endif %}>token_delete</option>
    <option value="invite_create" {% if filter_action == "invite_create" %}selected{% endif %}>invite_create</option>
    <option value="invite_delete" {% if filter_action == "invite_delete" %}selected{% endif %}>invite_delete</option>
    <option value="import" {% if filter_action == "import" %}selected{% endif %}>import</option>
    <option value="export" {% if filter_action == "export" %}selected{% endif %}>export</option>
  </select>
  <select onchange="applyFilters()" id="filter-actor"
    class="px-3 py-2 border border-border rounded-md bg-input-bg text-sm">
    <option value="">All Actors</option>
    {% for a in actors %}
    <option value="{{ a }}" {% if filter_actor == a %}selected{% endif %}>{{ a }}</option>
    {% endfor %}
  </select>
</div>

{% if entries %}
<div class="bg-card border border-border-light rounded-lg overflow-hidden">
  <table class="w-full">
    <thead class="bg-card-alt">
      <tr>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Time</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Actor</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Action</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Resource</th>
        <th class="text-left px-4 py-2.5 text-sm font-medium text-muted">Details</th>
      </tr>
    </thead>
    <tbody class="divide-y divide-border-light">
      {% for entry in entries %}
      <tr class="hover:bg-card-alt">
        <td class="px-4 py-2.5 text-sm text-muted font-mono">{{ entry.timestamp[:19] }}</td>
        <td class="px-4 py-2.5 text-sm">{{ entry.actor }}</td>
        <td class="px-4 py-2.5 text-sm">
          <span class="px-2 py-0.5 rounded text-xs font-medium
            {% if entry.action is startingwith("content_") or entry.action is startingwith("schema_") %}bg-accent-soft text-accent
            {% elif entry.action in ["login", "logout", "user_create"] %}bg-success-soft text-success
            {% else %}bg-card-alt text-muted{% endif %}">
            {{ entry.action }}
          </span>
        </td>
        <td class="px-4 py-2.5 text-sm text-muted">
          {% if entry.resource_id %}{{ entry.resource_type }} / {{ entry.resource_id }}{% else %}{{ entry.resource_type }}{% endif %}
        </td>
        <td class="px-4 py-2.5 text-sm text-muted font-mono">
          {% if entry.details %}{{ entry.details[:80] }}{% if entry.details|length > 80 %}…{% endif %}{% else %}—{% endif %}
        </td>
      </tr>
      {% endfor %}
    </tbody>
  </table>
</div>

<div class="flex items-center justify-center gap-4 mt-4">
  {% if has_prev %}
  <a href="/settings/audit-log?{{ pagination_qs }}page={{ page - 1 }}" class="text-sm text-accent hover:underline">← Previous</a>
  {% else %}
  <span class="text-sm text-muted">← Previous</span>
  {% endif %}
  <span class="text-sm text-muted">Page {{ page }}</span>
  {% if has_next %}
  <a href="/settings/audit-log?{{ pagination_qs }}page={{ page + 1 }}" class="text-sm text-accent hover:underline">Next →</a>
  {% else %}
  <span class="text-sm text-muted">Next →</span>
  {% endif %}
</div>
{% else %}
<div class="bg-card border border-border-light rounded-lg p-8 text-center text-muted">
  No audit log entries.
</div>
{% endif %}

<script>
function applyFilters() {
  var action = document.getElementById('filter-action').value;
  var actor = document.getElementById('filter-actor').value;
  var params = new URLSearchParams();
  if (action) params.set('action', action);
  if (actor) params.set('actor', actor);
  var qs = params.toString();
  window.location.href = '/settings/audit-log' + (qs ? '?' + qs : '');
}
</script>
{% endblock %}
```

**Note on pagination links:** The template uses `pagination_qs` for preserving filter params in pagination links. The handler must compute this and pass it to the template. Add a `pagination_qs` variable to the handler's render context:

```rust
let mut pagination_params = Vec::new();
if !filter.action.is_empty() {
    pagination_params.push(format!("action={}", filter.action));
}
if !filter.actor.is_empty() {
    pagination_params.push(format!("actor={}", filter.actor));
}
// Trailing & so template can append page=N directly; empty string if no filters
let pagination_qs = if pagination_params.is_empty() {
    String::new()
} else {
    format!("{}&", pagination_params.join("&"))
};
```

And add `pagination_qs => pagination_qs` to the `minijinja::context!` block.

- [ ] **Step 3: Verify the build compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo build`

Expected: compiles successfully.

- [ ] **Step 4: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/routes/settings.rs templates/settings/audit_log.html && git commit -m "feat: add audit log viewer page with filters and pagination"
```

---

### Task 4: Navigation link

**Files:**
- Modify: `templates/_nav.html`

- [ ] **Step 1: Add the nav link**

In `templates/_nav.html`, inside the `{% if user_role == "admin" %}` block (around line 25-32), add the "Audit Log" link between the Webhooks and Data links:

```html
<a href="/settings/audit-log" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Audit Log</a>
```

It should go after the Webhooks link (`<a href="/settings/webhooks" ...>`) and before the Data link (`<a href="/settings/data" ...>`).

- [ ] **Step 2: Verify build compiles**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo build`

Expected: compiles.

- [ ] **Step 3: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add templates/_nav.html && git commit -m "feat: add audit log nav link for admins"
```

---

### Task 5: Integration tests

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Write integration tests**

Add these tests at the end of `tests/integration.rs`, before the final closing brace (if any). Add a section comment:

```rust
// ── Audit Log Viewer tests ─────────────────────────────────────

#[tokio::test]
async fn audit_log_page_requires_admin() {
    let s = TestServer::start().await;
    s.setup_admin().await;

    // Editor cannot access
    let editor = signup_user_with_role(&s, "audit-editor@test.com", "auditeditor", "editor").await;
    let resp = editor
        .get(s.url("/settings/audit-log"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Admin can access
    let resp = s
        .client
        .get(s.url("/settings/audit-log"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Audit Log"));
}

#[tokio::test]
async fn audit_log_shows_entries_and_filters() {
    let s = TestServer::start().await;
    s.setup_admin().await;

    // Create a schema (generates schema_create audit entry)
    let schema_json = r#"{
        "x-substrukt": {"title": "Audit Test", "slug": "audit-test", "storage": "directory"},
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"]
    }"#;
    s.create_schema(schema_json).await;

    // Small delay to let fire-and-forget audit log write complete
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Unfiltered page shows the entry
    let resp = s
        .client
        .get(s.url("/settings/audit-log"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("schema_create"));

    // Filter by action
    let resp = s
        .client
        .get(s.url("/settings/audit-log?action=schema_create"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("schema_create"));

    // Filter by non-matching action returns no entries in table
    let resp = s
        .client
        .get(s.url("/settings/audit-log?action=content_delete"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("No audit log entries."));
}
```

- [ ] **Step 2: Run the integration tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test audit_log_page_requires_admin audit_log_shows_entries_and_filters -- --nocapture`

Expected: both tests pass.

- [ ] **Step 3: Run the full test suite**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test`

Expected: all tests pass (existing + new).

- [ ] **Step 4: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add tests/integration.rs && git commit -m "test: add integration tests for audit log viewer"
```
