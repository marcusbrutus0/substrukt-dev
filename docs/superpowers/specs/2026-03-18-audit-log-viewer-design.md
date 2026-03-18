# Audit Log Viewer UI Design

## Goal

Add an admin-only audit log viewer page at `/settings/audit-log` so admins can answer "who changed what and when?" by browsing, filtering, and paginating the existing `audit_log` table.

## Current State

- Audit events are logged to a separate `audit.db` SQLite database via `AuditLogger::log()` (fire-and-forget async)
- 16 action types: `content_create`, `content_update`, `content_delete`, `schema_create`, `schema_update`, `schema_delete`, `login`, `logout`, `signup`, `token_create`, `token_delete`, `invite_create`, `invite_delete`, `import`, `export`, `publish`
- Table schema: `id INTEGER PRIMARY KEY`, `timestamp TEXT`, `actor TEXT`, `action TEXT`, `resource_type TEXT`, `resource_id TEXT`, `details TEXT`
- Indexes on `timestamp` and `action`
- No existing UI to view these logs — only webhook history has a viewer
- Webhook history page (`/settings/webhooks`) is the closest existing pattern

## Scope

In scope: paginated log viewer with action and actor filters, admin-only access, nav link.

Out of scope: date range filtering, full-text search, log export/download, log retention/cleanup, detail expansion/modal.

## Design

### Data Layer

Add to `src/audit.rs`:

**`AuditLogEntry` struct:**
```rust
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

**`list_audit_log` method on `AuditLogger`:**
- Parameters: `action_filter: Option<&str>`, `actor_filter: Option<&str>`, `page: u32` (1-based)
- Page size: 100 entries
- Query: `SELECT id, timestamp, actor, action, resource_type, resource_id, details FROM audit_log` with optional `WHERE` clauses, `ORDER BY timestamp DESC, id DESC`, `LIMIT 101 OFFSET (page-1)*100`
- Returns: `(Vec<AuditLogEntry>, bool)` — entries (capped at 100) and `has_next` (true if 101 rows were returned)
- The `LIMIT 101` trick avoids a separate `COUNT(*)` query

**`list_audit_actors` method on `AuditLogger`:**
- Query: `SELECT DISTINCT actor FROM audit_log ORDER BY actor`
- Returns: `Vec<String>`
- Used to populate the actor filter dropdown

### Route

`GET /settings/audit-log` added to the settings router in `src/routes/settings.rs`.

**Handler: `audit_log_page`**
- Admin-only: `auth::require_role(&session, "admin").await?`
- Accepts `Query<AuditLogFilter>` with fields: `action: String`, `actor: String`, `page: String`
- Parses `page` to `u32`, defaults to 1 if missing/invalid/zero
- Calls `list_audit_log` and `list_audit_actors` on `state.audit`
- Renders `settings/audit_log.html` with context: entries, filters, page, has_next, has_prev, actors list

**`AuditLogFilter` struct:**
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

### Template

`templates/settings/audit_log.html` — extends `base_template`, follows webhooks page pattern.

**Filter bar:**
Two `<select>` dropdowns with `onchange="applyFilters()"`:
- Action filter: hardcoded `<option>` list of the 16 known action types, with "All Actions" default
- Actor filter: populated from `actors` context variable, with "All Actors" default
- Both preserve selected state via template conditionals

**Table:**
- Wrapper: `bg-card border border-border-light rounded-lg overflow-hidden`
- Header: `bg-card-alt` with columns: Time, Actor, Action, Resource, Details
- Body: `divide-y divide-border-light`, rows with `hover:bg-card-alt`
- Time column: `text-sm text-muted font-mono`, truncated to first 19 chars (remove timezone)
- Actor column: `text-sm`
- Action column: badge with category-based color:
  - Content actions (`content_*`): `bg-accent-soft text-accent`
  - Schema actions (`schema_*`): `bg-accent-soft text-accent`
  - Auth actions (`login`, `logout`, `signup`): `bg-success-soft text-success`
  - Admin actions (everything else): `bg-card-alt text-muted`
- Resource column: `text-sm text-muted` — shows `resource_type` + `resource_id` if non-empty (e.g., "content / my-post"), or just `resource_type`
- Details column: `text-sm text-muted font-mono` — shows first 80 chars of details with ellipsis if truncated, or "—" if null

**Pagination:**
Below the table, a flex row with:
- "Previous" link (disabled on page 1)
- "Page N" text
- "Next" link (disabled when `!has_next`)
- Links are `<a>` tags that preserve current filters in query params
- Styling: `text-sm text-accent hover:underline` for active, `text-sm text-muted cursor-default` for disabled

**Empty state:**
`bg-card border border-border-light rounded-lg p-8 text-center text-muted` with "No audit log entries."

**JavaScript:**
```javascript
function applyFilters() {
    var action = document.getElementById('filter-action').value;
    var actor = document.getElementById('filter-actor').value;
    var params = new URLSearchParams();
    if (action) params.set('action', action);
    if (actor) params.set('actor', actor);
    // Reset to page 1 when filters change
    var qs = params.toString();
    window.location.href = '/settings/audit-log' + (qs ? '?' + qs : '');
}
```

### Navigation

Add "Audit Log" link in `templates/_nav.html` inside the `{% if user_role == "admin" %}` block, between Webhooks and Data:

```html
<a href="/settings/audit-log" class="block px-3 py-2 rounded hover:bg-sidebar-hover">Audit Log</a>
```

### Files Changed

- `src/audit.rs` — add `AuditLogEntry` struct, `list_audit_log` method, `list_audit_actors` method, unit tests
- `src/routes/settings.rs` — add `AuditLogFilter` struct, `audit_log_page` handler, route registration
- `templates/settings/audit_log.html` — new template (filter bar, table, pagination)
- `templates/_nav.html` — add admin-only "Audit Log" nav link

### Testing

Unit tests in `audit.rs`:
- `list_audit_log` returns entries in reverse chronological order
- `list_audit_log` with action filter returns only matching entries
- `list_audit_log` with actor filter returns only matching entries
- `list_audit_log` pagination: page 1 returns first 100, page 2 returns next batch, `has_next` correct
- `list_audit_actors` returns distinct actors sorted

Integration tests:
- Admin can access `/settings/audit-log` and sees the page
- Non-admin (editor/viewer) is rejected
- Entries created via the API appear in the audit log page
- Action filter query param filters results
- Actor filter query param filters results

### Error Handling

- Invalid page number (non-numeric, zero, negative): treat as page 1
- Unknown action/actor filter values: pass to SQL WHERE clause — returns empty results, which is correct
- Empty audit log: shows "No audit log entries." message
