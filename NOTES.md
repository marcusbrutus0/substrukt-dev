# NOTES.md

Working notes for Claude Code. Update this file as you learn things while building substrukt.

## Architectural Decisions

- **Session layer ordering**: Session layer applied via `.layer(session_layer)` on the final Router (outermost). Auth middleware applied inside via `from_fn_with_state`. Axum applies layers outside-in, so session runs before auth — Session is in request extensions when auth middleware checks it. Verified working.
- **Auth middleware reads Session from extensions**: Can't use `Session` as a function parameter in `from_fn_with_state` middleware because it creates extraction ordering issues. Instead, read from `request.extensions().get::<Session>()`.
- **Content storage modes**: `directory` = one JSON file per entry in `data/content/<slug>/<entry-id>.json`. `single-file` = all entries in `data/content/<slug>.json` as a JSON array with `_id` fields.
- **Upload storage**: Content-addressed via SHA-256. Files at `data/uploads/<first-2-hex>/<remaining-hex>` with `.meta.json` sidecar.
- **Form generation**: Done in Rust (not templates) via `content::form::render_form_fields`. Returns raw HTML string. Must use `|safe` filter in templates.

## Code Style & Patterns

- Route handlers return `impl IntoResponse` or `axum::response::Result<Html<String>>`
- Template rendering: acquire `state.templates.read().await`, get template, render with `minijinja::context!`
- Schema files are plain JSON Schema with `x-substrukt` extension for metadata
- Error handling: `eyre::Result` for internal functions, axum error responses for routes

## Dependency Versions (Critical)

These specific version combos are required due to trait compatibility:

```toml
axum = "0.8"                    # uses axum-core 0.5
tower-sessions = "0.14"         # uses tower-sessions-core 0.14 (axum-core 0.5)
tower-sessions-sqlx-store = "0.15"  # uses tower-sessions-core 0.14
rand = "0.9"                    # 0.8's `gen()` is reserved keyword in edition 2024
argon2 = "0.5"                  # uses rand_core 0.6 internally
```

For argon2 OsRng: `use argon2::password_hash::rand_core::OsRng` (NOT `rand::rngs::OsRng`)

## Lessons Learned

- tower-sessions ecosystem has persistent version lag between the main crate and store crates. Always check `cargo tree -d` for duplicate `tower-sessions-core` or `axum-core`.
- Rust 2024 edition reserves `gen` as keyword — breaks `rng.gen()` from rand 0.8. Use `rng.random()` with rand 0.9 instead.
- `cargo build` / `git` not available without direnv — always source it first.
- Session cookie `Secure` flag must be disabled for HTTP dev (`.with_secure(false)`). Otherwise browsers won't send session cookies over plain HTTP.
- Upload fields in schemas use `type: "string", format: "upload"` but are stored as `{hash, filename, mime}` objects. Content validation patches the schema at runtime to accept either string or object for upload fields.
- HTML forms can't send PUT/DELETE. Schema update uses POST, deletes use `fetch()` with DELETE and return 204.
- serde_json uses BTreeMap for JSON objects — properties iterate alphabetically, not in insertion order. This affects `generate_entry_id` which picks the first string field.
- Per-entry publish/unpublish: `set_entry_status` writes `_status` directly to disk without going through `save_entry` (avoids re-validation and snapshots). `save_entry` now respects explicit `_status` in incoming data for API inline status. `publish_all_drafts` removed; publish routes only fire webhooks.
- API token creation requires editor+ role. Viewers can view the tokens page but cannot create tokens. This means "viewer API token" tests must be done through the UI session path, not via API bearer tokens.
- Test infrastructure: `signup_user_with_role` creates a user with a specific role via the admin invite flow. Use the returned `Client` for session-based tests with that role.
- **Configurable deployments**: Replaced hardcoded staging/production webhook system with admin-managed deployment targets. Each deployment has its own webhook URL, auth token, include_drafts toggle, and optional auto-deploy with debounce. Data stored in `deployments` table in audit.db. Auto-deploy uses one tokio task per deployment with `CancellationToken` from `tokio-util`, tracked in `DashMap<i64, CancellationToken>` on AppState. Background tasks poll `deployment_state.is_dirty` then fire webhook + debounce sleep. Old CLI webhook flags, `/publish` routes, and `/settings/webhooks` page removed.
- SQLite foreign keys require `PRAGMA foreign_keys = ON` per connection. Added `.pragma("foreign_keys", "ON")` to both `init_pool()` and `test_pool()` in audit.rs. Without this, CASCADE deletes silently do nothing.
