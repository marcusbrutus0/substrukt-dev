# Dynamic OpenAPI Spec Endpoint

## Overview

Serve a complete OpenAPI 3.1 spec at `GET /api/v1/openapi.json` that dynamically reflects the full API surface, including content models derived from user-defined JSON Schemas.

## Approach

Manual JSON construction using `serde_json::Value`. This is simpler than utoipa for our use case because the content endpoints are runtime-generated from user-defined JSON Schemas. No new crate dependencies needed.

## Architecture

### New module: `src/openapi.rs`

Single module with one public function:

```rust
pub fn generate_spec(config: &Config, data_dir: &Path) -> serde_json::Value
```

Builds the full OpenAPI 3.1 spec by:
1. Constructing the static portion (info, security schemes, fixed routes)
2. Iterating over all app directories in `data_dir`, loading their schemas
3. For each app + schema, generating content CRUD paths with request/response bodies derived from the user's JSON Schema

### Caching

Add an `openapi_cache: tokio::sync::RwLock<Option<serde_json::Value>>` field to `AppStateInner`. The handler reads from cache, generating on miss. The existing file watcher's rebuild cycle clears this cache (set to `None`), so the next request regenerates.

### Route

Added to the existing `api_global_routes()` in `src/routes/api.rs`:
```
GET /api/v1/openapi.json
```

No authentication required -- this is API documentation, like `/healthz`.

### Handler

```rust
async fn openapi_spec(State(state): State<AppState>) -> Json<serde_json::Value>
```

Reads from `openapi_cache`. On miss, calls `openapi::generate_spec()`, stores result, returns it.

## Static Routes Documented

All routes include request/response shapes, error responses, and auth requirements.

### Global (no app scope):
- `GET /api/v1/openapi.json` -- this endpoint (no auth)
- `GET /api/v1/backups/status` -- admin only
- `POST /api/v1/backups/trigger` -- admin only

### App-scoped (`/api/v1/apps/{app_slug}/...`):
- `GET /schemas` -- viewer+, list schemas
- `GET /schemas/{slug}` -- viewer+, get schema
- `GET /content/{schema_slug}` -- viewer+, list entries (query params: q, status)
- `POST /content/{schema_slug}` -- editor+, create entry
- `GET /content/{schema_slug}/{entry_id}` -- viewer+, get entry
- `PUT /content/{schema_slug}/{entry_id}` -- editor+, update entry
- `DELETE /content/{schema_slug}/{entry_id}` -- editor+, delete entry
- `GET /content/{schema_slug}/single` -- viewer+, get single
- `PUT /content/{schema_slug}/single` -- editor+, upsert single
- `DELETE /content/{schema_slug}/single` -- editor+, delete single
- `POST /content/{schema_slug}/{entry_id}/publish` -- editor+, publish
- `POST /content/{schema_slug}/{entry_id}/unpublish` -- editor+, unpublish
- `POST /uploads` -- editor+, upload file (multipart)
- `GET /uploads/{hash}` -- viewer+, get upload
- `POST /export` -- admin, export bundle
- `POST /import` -- admin, import bundle (multipart)
- `GET /deployments` -- viewer+, list deployments
- `POST /deployments/{slug}/fire` -- editor+, fire deployment

## Dynamic Content Schema Generation

For each app's schemas, the content CRUD endpoints get request/response bodies derived from the user's JSON Schema. The user's schema properties become the properties of the request body schema. The response wraps entries with `_id` and `_status` metadata fields.

## Auth Documentation

Security scheme: HTTP Bearer token. Documented in the spec's `securityDefinitions` / `components.securitySchemes`. Each endpoint notes the minimum role required (viewer, editor, admin) in its description.

## Cache Invalidation

The existing file watcher in `cache.rs` already rebuilds the content cache when files change. We extend `spawn_watcher` (or its rebuild path) to also clear the `openapi_cache`. This ensures schema file changes are reflected in the next spec request.

## Testing

One unit test in `src/openapi.rs` that verifies the generated spec structure (has info, paths, components). One integration test that hits `GET /api/v1/openapi.json` and verifies it returns valid JSON with the expected structure.

## Files Changed

- `src/openapi.rs` -- new module (spec generation logic)
- `src/lib.rs` -- add `pub mod openapi`
- `src/state.rs` -- add `openapi_cache` field
- `src/routes/api.rs` -- add handler and route
- `src/routes/mod.rs` -- wire openapi route outside auth middleware
- `src/cache.rs` -- clear openapi cache on file changes
- `src/main.rs` -- initialize `openapi_cache` field
- `tests/integration.rs` -- add `openapi_cache` field to test state, add test
