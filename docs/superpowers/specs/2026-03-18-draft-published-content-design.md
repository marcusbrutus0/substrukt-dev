# Draft/Published Content States Design

## Goal

Add draft/published workflow to content entries so users can save work-in-progress content that is only visible to production consumers after an explicit publish action. Staging environments see all content including drafts.

## Current State

- Content entries are JSON files on disk, cached in a DashMap
- No content status concept — all saved content is immediately available via the API
- `POST /api/publish/{environment}` fires a webhook to notify external builders
- Staging and production webhooks share the same dirty-check mechanism (audit log mutations vs `last_fired_at`)
- `_id` is an existing system metadata field injected into entries (visible in API responses)
- Content is validated against JSON Schema before saving
- Version history snapshots the entry data before each update
- Export/import bundles raw content JSON files for deployment

## Scope

In scope: `_status` metadata field, API status filtering, bulk publish action, draft badge in UI, backwards-compatible migration.

Out of scope: per-entry publish, per-schema publish, scheduled publishing, draft previews with share links, editorial workflows (review/approve).

## Design

### Data Model

Each content entry gets a `_status` metadata field stored in the JSON file alongside user data. Values: `"draft"` or `"published"`.

```json
{
  "_id": "my-post",
  "_status": "draft",
  "title": "My Post",
  "body": "..."
}
```

- On create: `_status` is always set to `"draft"`
- On edit: `_status` is preserved (editing a published entry keeps it published)
- On production publish: all entries with `_status: "draft"` get flipped to `"published"` and re-saved to disk
- Cache stores the full entry including `_status`. The content list API reads from disk (current behavior); status filtering is applied to the results after reading. Cache is used for reference resolution and single-entry lookups as before.

### `_status` Injection Point

`_status` is injected in `save_entry` after JSON Schema validation, the same way `_id` is injected for single-file storage. This avoids validation failures with schemas that use `additionalProperties: false`.

- **Create** (`entry_id` is `None`): `save_entry` injects `_status: "draft"` into the data after validation
- **Update** (`entry_id` is `Some`): `save_entry` reads the existing entry's `_status` from cache/disk and preserves it in the new data after validation. If no existing entry is found (e.g., first `upsert_single` call), falls back to `"draft"`.
- **Revert**: the revert handler calls `save_entry` as an update, so the current `_status` is preserved — reverting a published entry keeps it published, even if the historical snapshot had `_status: "draft"` or no `_status` at all

This centralizes `_status` management in one place rather than requiring each of the 6+ call sites to handle it.

### `_status` in API Responses

`_status` is stripped from API responses. Unlike `_id` (which consumers need to reference entries), `_status` is internal metadata for the CMS workflow. A `strip_internal_status` helper in `content/mod.rs` clones the entry data and removes the `_status` key. Called in API handlers that return entry data: `list_entries`, `get_entry`, and `get_single` (3 response points in `routes/api.rs`).

### `_status` in Search

The `filter_entries` function does case-insensitive substring search across all string values. Currently it searches all keys including `_id`. This feature adds a change: the search function skips keys starting with `_` (both `_status` and `_id`). This is a minor behavioral change — `_id` values will no longer match search queries, which is acceptable since users don't see or care about internal IDs in search results.

### `_status` in Version History

Version history snapshots include `_status` as part of the stored data (no special handling). However, on revert, the current entry's `_status` is preserved (see Injection Point above). This means:
- Snapshots faithfully record what the entry looked like at that point in time
- Reverting never changes publish status — that's controlled only by the bulk publish action

### `_status` in Export/Import

Exported content retains `_status` as-is in the JSON files. On import, entries keep whatever `_status` they had when exported. This is the right default for the sync workflow (GitHub Action deploys content to cloud instance) — if content was published locally, it should be published on the target too.

Import validation: the `validate_imported_content` function in the sync module runs JSON Schema validation on imported entries. Since imported data may contain `_status`, and schemas with `additionalProperties: false` would reject it, the import validation must strip `_`-prefixed keys (`_status`, `_id`) from the data before validating against the schema. The `_`-prefixed keys are then preserved in the actual stored data (they're only stripped for validation purposes).

### Backwards Compatibility

Existing entries on disk won't have `_status`. On load into cache, entries without `_status` are treated as `"published"`. No disk migration needed — the field gets written naturally on next create or edit.

### API Changes

**Existing endpoints — `status` query parameter:**

- `GET /api/content/{slug}` — default returns published-only. Accepts `?status=draft` (drafts only) or `?status=all` (everything).
- `GET /api/content/{slug}/{entry_id}` — returns the entry regardless of status (specific ID = direct access). `_status` stripped from response.
- `POST /api/content/{slug}` — creates entry as draft. No change to request/response format.
- `PUT /api/content/{slug}/{entry_id}` — updates entry, preserves current `_status`. No change to request/response format.

**Single schemas:**

- `GET /api/content/{slug}/single` — returns the single entry only if published by default. `?status=all` returns it regardless. If the entry exists but is draft and no `?status=all`, returns 404. This endpoint needs a query parameter added.

**Modified endpoint behavior:**

- `POST /api/publish/production` — before firing the webhook, call `publish_all_drafts`. This function iterates all schemas, then all entries per schema (reading from disk via existing `list_entries`). For each draft entry, it mutates `_status` to `"published"` in the data and writes back to disk (bypassing `save_entry` to avoid validation/snapshot overhead — this is a metadata-only change). Cache is updated per-entry after each successful write. The file watcher's 200ms debounce window handles the rapid writes without thrashing. If any write fails, return error and do not fire webhook. Entries already flipped stay flipped (best-effort, not atomic).
- `POST /api/publish/staging` — does NOT flip statuses. Just fires the webhook. Staging = preview including drafts.

**Publish permission:** The publish endpoint currently requires `editor` role. This is unchanged — editors can already create/edit all content, so allowing them to publish is consistent.

### Webhook Integration

**Staging webhook:**
- Fires on the existing cron/dirty check — any content mutation marks the environment dirty
- No status change — drafts stay as drafts
- External builders hitting the staging API should use `?status=all` to see both draft and published content

**Production webhook:**
- `POST /api/publish/production` flips all drafts to published, then fires the webhook
- Dirty check unchanged — considers content mutations since last production publish

### Publish and Version History

Bulk publish does NOT create version history snapshots. The status flip is a metadata change, not a content change — the entry data itself is identical. This avoids creating N snapshots for N published entries. If the user wants to undo a publish, they can't via history (the content didn't change), but they could re-save an entry to make it draft again in a future version (out of scope for now).

### UI Changes

**Content list page:**
- Draft entries show a `Draft` badge: `<span class="px-2 py-0.5 rounded text-xs font-medium bg-accent-soft text-accent">Draft</span>`
- Published entries show no badge (published is the normal/default state)
- The web UI always shows all entries regardless of status (both draft and published). Status filtering is API-only.

**Content edit page:**
- No changes to the form
- Status is not user-editable per entry — controlled by the bulk publish action

**Publish buttons:**
- Current "Publish to staging/production" buttons remain. Production publish now also moves drafts to published.

### Files Changed

- `src/content/mod.rs` — inject `_status` in `save_entry` (after validation), add `publish_all_drafts` function, add `strip_internal_status` helper, add status filtering to list functions, exclude `_`-prefixed keys from search
- `src/routes/api.rs` — add `status` query parameter to content list and single endpoints, strip `_status` from responses, call `publish_all_drafts` before production webhook fire
- `src/routes/content.rs` — pass `_status` info to templates for badge rendering, ensure revert preserves `_status` (handled by `save_entry`)
- `src/sync/mod.rs` — strip `_`-prefixed keys before validation on import (preserve in stored data)
- `templates/content/list.html` — add draft badge next to entry titles
- `templates/content/singles.html` — add draft badge if single entry is draft

### Testing

Unit tests:
- `_status` set to `"draft"` on create, injected after validation
- `_status` preserved on edit (update path)
- `publish_all_drafts` flips all drafts to published
- Status filtering: default returns published-only, `?status=draft` returns drafts, `?status=all` returns both
- Missing `_status` treated as published (backwards compat)
- `_status` excluded from content search
- `_status` stripped from API response data

Integration tests:
- Create entry → verify draft → publish production → verify published
- API default returns only published entries
- API `?status=all` returns both draft and published
- API `PUT` update preserves `_status` (edit published entry stays published)
- Staging publish fires webhook without changing statuses
- Production publish flips statuses then fires webhook
- Existing entries without `_status` treated as published in API
- Revert a published entry preserves published status
- Single schema: draft single returns 404 by default, `?status=all` returns it
- First `upsert_single` creates entry as draft
- Schema with `additionalProperties: false` — create entry succeeds (status injected after validation)
- Import with `_status` in data and `additionalProperties: false` schema — import succeeds

### Error Handling

- Publish with no draft entries: succeeds silently (fires webhook — there may be schema changes or other reasons to notify)
- Missing `_status` on existing entries: treat as `"published"` for backwards compatibility
- Invalid `?status=` value: ignore the filter, return published-only (same as default)
- Publish write failure: return error, do not fire webhook. Entries already flipped stay flipped (best-effort, not atomic).
