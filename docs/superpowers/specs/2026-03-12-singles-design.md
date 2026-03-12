# Singles: Single-Object Content Type

## Summary

Add support for "single" objects in Substrukt — one-off content items (site settings, homepage hero, about page) where there's exactly one instance per schema, no list view, and no ID management. Singles are implemented as a thin wrapper on the existing collection system, reusing all content CRUD, validation, form rendering, and upload handling.

## Schema Metadata

Add an optional `kind` field to the `x-substrukt` metadata block:

```json
{
  "x-substrukt": {
    "title": "Site Settings",
    "slug": "site-settings",
    "storage": "directory",
    "kind": "single"
  }
}
```

- `kind` is optional, defaults to `"collection"` (fully backward compatible).
- Valid values: `"single"`, `"collection"`. Invalid values are rejected by serde deserialization of the `Kind` enum.
- `SubstruktMeta` struct gets a new `Kind` enum field (`Single`, `Collection`), with `Collection` as the default.
- Schema create/edit UI gets a dropdown or toggle for selecting the kind.
- `id_field` is ignored for singles (the fixed `_single` ID is always used).

## Content Storage & CRUD

Singles reuse the existing content entry system with a fixed entry ID:

- **Fixed ID**: When `kind` is `"single"`, the entry uses the fixed ID `"_single"`. No ID generation from fields, no slugification. The entry ID is always passed as `Some("_single")` to `save_entry`, bypassing `generate_entry_id`.
- **Storage mode**: Uses whichever `storage` mode the schema specifies (directory or single-file). In practice, a single with directory mode creates `data/content/site-settings/_single.json`.
- **Create vs Update**: Saving a single checks if `_single` exists — updates if yes, creates if no. The form always behaves like an edit form.
- **Lazy creation**: No file is written until the user first saves. An empty form is shown for singles that haven't been saved yet.
- **List entries**: Still works (returns 0 or 1 entries). The UI just never shows the list view for singles.
- **Delete**: Deleting a single's entry resets it to the "never edited" state (empty form on next visit).

The content module itself requires no changes. All behavioral differences live in the routes and UI layer.

## Web Routes & UI

**Routing behavior:**

No new web routes are registered. The existing `{entry_id}` wildcard routes handle `_single` as an entry ID naturally:

- `GET /content/{schema_slug}` — the `list_entries` handler checks the schema kind. For singles, it redirects to `/content/{schema_slug}/_single/edit`.
- `GET /content/{schema_slug}/_single/edit` — matched by the existing `/{schema_slug}/{entry_id}/edit` route. The `edit_entry_page` handler is modified: when the schema kind is `Single` and the `_single` entry does not exist, it renders an empty form instead of returning 404.
- `POST /content/{schema_slug}/_single` — matched by the existing `/{schema_slug}/{entry_id}` route. Creates or updates.

**Post-save redirect:** For singles, `create_entry` and `update_entry` redirect directly to `/content/{schema_slug}/_single/edit` instead of `/content/{schema_slug}` (avoiding a double redirect through the list handler).

**UI changes:**

- The "New Entry" button is hidden for singles.
- Schema list/dashboard indicates which schemas are singles vs collections.
- The edit form works identically to collection entry editing — same form rendering, validation, upload handling.

## API Routes

Dedicated `/single` sub-path within the existing content namespace:

- `GET /api/v1/content/{schema_slug}/single` — returns the `_single` entry's data directly (unwrapped, not `{id, data}`), or 404 if not yet created.
- `PUT /api/v1/content/{schema_slug}/single` — creates or updates the `_single` entry. Validates against schema.
- `DELETE /api/v1/content/{schema_slug}/single` — deletes the single entry.

**Route precedence:** The literal `/single` routes must be registered *before* the `{entry_id}` wildcard routes in the Axum router so they take precedence. This means an entry with ID `"single"` in a collection would be inaccessible via the API (an acceptable trade-off — `"single"` is reserved).

**Collection endpoint guard:** `POST /api/v1/content/{schema_slug}` (create entry) rejects requests for schemas with `kind: "single"`, returning 400 with an error directing the caller to use the `PUT /single` endpoint instead. This prevents creating multiple entries in a single-kind schema.

## Export/Import

No special handling needed:

- The `_single` entry is a regular content entry — bundled and restored like any other.
- The schema's `kind` field is part of the schema JSON and round-trips naturally.

## Edge Cases

- **Changing kind from `collection` to `single`**: Existing entries remain on disk. Only `_single` is accessible via the single UI/API. No destructive behavior.
- **Changing kind from `single` to `collection`**: The `_single` entry becomes a regular entry with ID `_single` in the list view.
- **Reserved ID**: `"single"` is reserved as a path segment in the API. Collection entries cannot use this ID via the API (the literal route takes precedence). This is documented but not enforced at the content layer — it only affects API routing.
- **Dashboard display**: Entry count for singles shows 0 or 1, which is correct. No special display treatment needed.
