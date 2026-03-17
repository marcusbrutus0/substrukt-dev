# P1 Features: Search, Markdown, References, Versioning

Four features for Substrukt, implemented in order. Each builds on existing patterns in the codebase.

## 1. Content Search & Filtering

### Problem
The content list (`/content/{slug}`) shows a flat table with no way to find entries. Unusable beyond ~50 entries.

### Design

Add `?q=` query parameter to both the UI list route and the API list endpoint. Case-insensitive substring match across all string values in each entry.

**routes/content.rs** — `list_entries` handler:
- Accept `Query<SearchParams>` with `q: Option<String>`
- After loading entries via `content::list_entries()`, filter: for each entry, check if any string value in `entry.data` contains the query (case-insensitive)
- Extract the filtering logic into a function in `content/mod.rs`: `fn filter_entries(entries: Vec<ContentEntry>, query: &str) -> Vec<ContentEntry>` that walks each entry's JSON values recursively

**routes/api.rs** — `list_entries` handler:
- Same `Query<SearchParams>`, same filtering logic

**templates/content/list.html**:
- Add a search input above the table with `name="q"`, `hx-get="/content/{schema_slug}"`, `hx-trigger="input changed delay:300ms"`, `hx-target="#main-content"`
- htmx will serialize the input value as `?q=<value>` automatically when the input has a `name` attribute
- Show result count ("N entries" or "N of M entries" when filtered)
- Pass the current `q` value to the template so it persists in the search input after htmx swaps

**content/mod.rs** — new functions:
```rust
pub fn matches_query(data: &Value, query: &str) -> bool {
    // Recursively walk the Value:
    // - String: check if lowercase contains query
    // - Object: recurse into values
    // - Array: recurse into elements
    // - Other: skip
}

pub fn filter_entries(entries: Vec<ContentEntry>, query: &str) -> Vec<ContentEntry> {
    let query_lower = query.to_lowercase();
    entries.into_iter().filter(|e| matches_query(&e.data, &query_lower)).collect()
}
```

### Files changed
- `src/content/mod.rs` — add `filter_entries`, `matches_query`
- `src/routes/content.rs` — accept query param, filter entries
- `src/routes/api.rs` — accept query param, filter entries
- `templates/content/list.html` — search input, result count

---

## 2. Markdown Field Type

### Problem
`textarea` fields are plain text. No way to write formatted content (blog posts, documentation).

### Design

New format: `"format": "markdown"` on `"type": "string"` fields. Renders an EasyMDE editor in the form. Stores raw markdown — rendering is the consumer's responsibility.

**form.rs** — `render_field`:
- New match arm for `("string", Some("markdown"))`:
```rust
("string", Some("markdown")) => {
    let val = value.and_then(|v| v.as_str()).unwrap_or("");
    // Renders a textarea with data-markdown attribute
    // EasyMDE will auto-attach via JS in base.html
}
```
- No changes to `form_data_to_json` — markdown is just a string, same as textarea

**base.html**:
- Add EasyMDE CSS and JS from CDN
- Wrap init in a reusable function and call it on both `DOMContentLoaded` and `htmx:afterSwap` (since `hx-boost="true"` replaces `#main-content` without re-firing `DOMContentLoaded`):
```js
function initMarkdownEditors() {
  document.querySelectorAll('[data-markdown]:not(.easymde-attached)').forEach(function(el) {
    el.classList.add('easymde-attached');
    new EasyMDE({ element: el, spellChecker: false, status: false });
  });
}
initMarkdownEditors();
document.body.addEventListener('htmx:afterSwap', initMarkdownEditors);
```
- The `easymde-attached` class prevents double-initialization
- Also call `initMarkdownEditors()` inside `addArrayItem` after appending new DOM

**Validation**: No schema patching needed. Markdown is stored as `type: "string"`, validated as string.

**Display columns**: Update `get_display_columns` in `routes/content.rs` to skip `format: "markdown"` (like uploads are skipped) — long markdown text would wreck the table layout.

### Files changed
- `src/content/form.rs` — new match arm for markdown
- `src/routes/content.rs` — skip markdown in display columns
- `templates/base.html` — EasyMDE CDN includes + init script with htmx:afterSwap handling

---

## 3. Content References

### Problem
No way to link entries across schemas (e.g. blog post -> author).

### Design

New format: `"format": "reference"` on `"type": "string"` fields. Target schema specified via `"x-substrukt-reference": { "schema": "authors" }` in the field's schema definition.

**Storage**: The JSON file stores the entry ID as a plain string (e.g. `"author": "john-doe"`).

**Form rendering** — needs access to target schema entries to populate a `<select>`:

`form.rs` currently has no access to the cache or state. Rather than coupling it to AppState, pre-compute reference options and pass them in.

New type:
```rust
// In content/form.rs
use std::collections::HashMap;

/// Map from field path to list of (id, label) options
pub type ReferenceOptions = HashMap<String, Vec<(String, String)>>;
```

Change `render_form_fields` signature to add `ref_options: &ReferenceOptions`. This also requires updating `render_field` to accept and thread `ref_options` through, since it calls `render_form_fields` recursively in both the object case (line 166) and array case (lines 193, 200):

```rust
pub fn render_form_fields(
    schema: &Value,
    data: Option<&Value>,
    prefix: &str,
    ref_options: &ReferenceOptions,
) -> String

fn render_field(
    name: &str,
    label: &str,
    field_type: &str,
    format: Option<&str>,
    schema: &Value,
    value: Option<&Value>,
    required: bool,
    ref_options: &ReferenceOptions,
) -> String
```

All three internal recursive calls in `render_field` must pass `ref_options` through:
- Object case: `render_form_fields(schema, value, name, ref_options)`
- Array existing items: `render_form_fields(&items_schema, Some(item), &item_name, ref_options)`
- Array template: `render_form_fields(&items_schema, None, &template_name, ref_options)`

In `routes/content.rs`, before calling `render_form_fields`:
- Scan the schema for properties with `format: "reference"`
- For each, read `x-substrukt-reference.schema` to get the target slug
- Look up entries from the cache by iterating keys with the target slug prefix (e.g. `"authors/"`)
- Build label from the entry's first string field value or the entry ID
- Populate the `ReferenceOptions` map keyed by field name

New match arm in `render_field`:
```rust
("string", Some("reference")) => {
    // Look up options from ref_options using the field name
    // Render a <select> with options
    // Include a "-- Select --" empty option
    // Pre-select the current value
}
```

**form_data_to_json**: No changes needed — reference fields parse as plain strings (same as the default string case).

**Validation**: Reference fields are already `"type": "string"` in the schema, so they validate correctly without any patching. No `patch_reference_types` is needed. For "does the referenced entry exist?" checking: do this in the route handler after `validate_content` returns, using the cache. Log a warning if a referenced entry is missing but do not block the save (the entry might be created later, or the referenced schema might not be loaded yet).

**API deep resolution**: In `routes/api.rs`, after loading entries, resolve references before returning. The resolution function lives in `routes/api.rs` (private to the API module) since it's API-specific behavior:

```rust
fn resolve_references(
    data: &mut Value,
    schema: &SchemaFile,
    cache: &ContentCache,
) {
    // Walk schema properties
    // For fields with format: "reference":
    //   Read x-substrukt-reference.schema to get target slug
    //   If data[field] is a string ID, look up "{target_slug}/{id}" in cache
    //   Replace the string with the full entry object (clone from cache)
    //   One level deep only — do not recurse into the resolved object
}
```

Apply to `list_entries`, `get_entry`, `get_single` in the API routes. The schema is already loaded in each handler, so no extra lookup is needed.

**Display columns**: Skip reference fields in `get_display_columns` (like uploads and markdown).

### Files changed
- `src/content/form.rs` — new `ReferenceOptions` type, updated signatures for `render_form_fields` and `render_field`, new match arm, thread `ref_options` through recursive calls
- `src/routes/content.rs` — build reference options from cache, pass to form, skip references in display columns, all `render_form_fields` call sites updated
- `src/routes/api.rs` — `resolve_references` function, apply to GET endpoints

---

## 4. Content Versioning (Last N)

### Problem
Content edits overwrite the JSON file. No undo, no history.

### Design

Before every save, snapshot the current version into a history directory. Keep the last N versions (configurable via `--version-history-count`, default 10).

**History directory structure**:
```
data/_history/<schema-slug>/<entry-id>/
  1710700000.json   # unix timestamp
  1710700100.json
  1710700200.json
```

This is a sibling of `data/content/`, not inside it, so the file watcher (which watches `data/content/` and `data/schemas/`) will not trigger spurious cache rebuilds.

**Config** — add `version_history_count: usize` field to `Config` struct. Update `Config::new` to accept the new parameter and store it:
```rust
pub fn new(
    data_dir: Option<PathBuf>,
    db_path: Option<PathBuf>,
    port: Option<u16>,
    secure_cookies: bool,
    // ... existing params ...
    version_history_count: usize,
) -> Self {
    // ...
    Self {
        // ... existing fields ...
        version_history_count,
    }
}
```

Also add a `history_dir` helper:
```rust
pub fn history_dir(&self) -> PathBuf {
    self.data_dir.join("_history")
}
```

**CLI** — add to `Cli` struct:
```rust
#[arg(long, global = true, default_value = "10")]
version_history_count: usize,
```

Update the `Config::new` call in `main.rs` to pass `cli.version_history_count`.

**New module: `src/history.rs`**:
```rust
pub fn snapshot_entry(
    data_dir: &Path,
    schema_slug: &str,
    entry_id: &str,
    current_data: &Value,
    max_versions: usize,
) -> eyre::Result<()>

pub fn list_versions(
    data_dir: &Path,
    schema_slug: &str,
    entry_id: &str,
) -> eyre::Result<Vec<VersionInfo>>

pub fn get_version(
    data_dir: &Path,
    schema_slug: &str,
    entry_id: &str,
    timestamp: u64,
) -> eyre::Result<Option<Value>>

pub struct VersionInfo {
    pub timestamp: u64,
    pub size: u64,
}
```

**Integration with save** — call `snapshot_entry` in the route handler before `save_entry`, not inside `save_entry` itself. This keeps `save_entry` unchanged.

In `routes/content.rs::update_entry`:
```rust
if let Ok(Some(current)) = content::get_entry(&content_dir, &schema_file, &entry_id) {
    let _ = history::snapshot_entry(
        &state.config.data_dir,
        &schema_slug,
        &entry_id,
        &current.data,
        state.config.version_history_count,
    );
}
```

Same pattern in `routes/api.rs::update_entry`. For `routes/api.rs::upsert_single`, use `"_single"` as the entry_id:
```rust
if let Ok(Some(current)) = content::get_entry(&content_dir, &schema_file, "_single") {
    let _ = history::snapshot_entry(
        &state.config.data_dir,
        &schema_slug,
        "_single",
        &current.data,
        state.config.version_history_count,
    );
}
```

**New routes** — add to `routes/content.rs::routes()`:
- `GET /content/{schema_slug}/{entry_id}/history` — list versions page
- `POST /content/{schema_slug}/{entry_id}/revert/{timestamp}` — revert to a version (with CSRF). Reverting loads the historical version and saves it as the new current version (which itself creates a new history snapshot).

**New template**: `templates/content/history.html` — table of versions with timestamps (formatted as human-readable dates) and a "Revert" button for each.

**Edit page link**: Add a "History" link on the content edit page (next to Cancel) that goes to the history page.

**Single-file mode**: Snapshot the individual entry's data object (not the whole array). The history directory uses the entry's `_id` as the entry-id portion of the path.

**Delete behavior**: When an entry is deleted, its history stays. Allows recovery via manual file copy. History can be cleaned up via a future "purge" command.

**Export/Import**: History is NOT included in export bundles. The export in `sync/mod.rs` only exports `["schemas", "content", "uploads"]` directories, so `_history` is already excluded — no code change needed.

### Files changed
- `src/history.rs` — new module (snapshot, list, get, prune)
- `src/lib.rs` — add `pub mod history;`
- `src/config.rs` — add `version_history_count` field, update `Config::new` signature, add `history_dir` helper
- `src/main.rs` — add `--version-history-count` CLI flag, pass to `Config::new`
- `src/routes/content.rs` — snapshot before update, new history/revert routes added to `routes()`
- `src/routes/api.rs` — snapshot before update_entry and upsert_single
- `templates/content/history.html` — new template
- `templates/content/edit.html` — add History link

---

## Build Order

1. **Search & filtering** — isolated, no model changes, immediately useful
2. **Markdown field** — isolated, one new match arm + CDN include
3. **References** — touches form.rs signature (all call sites), API resolution logic
4. **Versioning** — new module, config changes, new routes/templates

Each feature gets its own branch, small atomic commits, merged to main when complete.

## Testing Strategy

Each feature needs integration tests in `tests/integration.rs`:

1. **Search**: Create entries, verify `?q=` filters correctly via both UI and API
2. **Markdown**: Create schema with markdown field, create/edit entry, verify data stored as string
3. **References**: Create two schemas, create entries, verify reference select renders, verify API returns resolved objects
4. **Versioning**: Create entry, update it N times, verify history list, verify revert restores old data
