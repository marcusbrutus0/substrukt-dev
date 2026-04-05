use crate::config::Config;

pub fn prime_output(config: &Config) -> String {
    format!(
        r#"# Substrukt — AI Workflow Context

> Schema-driven CMS built in Rust. Define content types with JSON Schema,
> edit via web UI, store as JSON files on disk, serve via REST API.

## Architecture

- **Multi-app**: Each app is an isolated content space with its own schemas, content, uploads, and deployments
- **Storage**: Content is JSON files on disk at `data/<app>/content/`, NOT in a database
- **SQLite**: Only for infrastructure — users, sessions, API tokens, apps, upload metadata
- **Audit**: Separate `audit.db` for audit log, deployment config, backup config
- **Cache**: DashMap in-memory cache, invalidated by file watcher

## Data Directory

```
{data_dir}/
  substrukt.db          # users, sessions, tokens, apps
  audit.db              # audit log, deployments, backup config
  <app-slug>/           # per-app data
    schemas/            # JSON Schema files (<slug>.json)
    content/            # content entries
      <slug>/           # directory mode: one file per entry
      <slug>.json       # single-file mode: all entries in one array
    uploads/            # content-addressed files (SHA-256)
    _history/           # version history snapshots
```

## CLI Commands

```bash
# Start the server (default if no command given)
substrukt serve [--port 3000] [--data-dir data] [--secure-cookies]

# Import/export app data
substrukt import <path.tar.gz> --app <slug>
substrukt export <path.tar.gz> --app <slug>

# Create an API token
substrukt create-token <name> --app <slug>

# Output this AI context
substrukt prime
```

### CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--data-dir` | `data` | Root data directory |
| `--db-path` | `<data-dir>/substrukt.db` | SQLite database path |
| `-p, --port` | `3000` | HTTP listen port |
| `--secure-cookies` | off | Secure flag on session cookies (for HTTPS) |
| `--api-rate-limit` | `100` | API requests per IP per minute |
| `--version-history-count` | `10` | Content versions kept per entry |
| `--max-body-size` | `50` | Max request body in MB |
| `--trust-proxy-headers` | off | Trust X-Forwarded-For for rate limiting |

### Environment Variables (S3 Backups)

| Variable | Description |
|----------|-------------|
| `S3_BUCKET` | S3 bucket name |
| `S3_REGION` | AWS region or custom region name |
| `S3_ENDPOINT` | Custom S3-compatible endpoint (Minio, R2, B2) |
| `S3_ACCESS_KEY` | Access key ID |
| `S3_SECRET_KEY` | Secret access key |

## API Reference

All app-scoped endpoints are under `/api/v1/apps/:app_slug/`.
Authentication: `Authorization: Bearer <token>` header.

### Schemas (read-only)

```
GET  /api/v1/apps/:app/schemas              # List all schemas
GET  /api/v1/apps/:app/schemas/:slug        # Get one schema
```

### Content (CRUD)

```
GET    /api/v1/apps/:app/content/:schema                  # List entries (?status=all for drafts)
POST   /api/v1/apps/:app/content/:schema                  # Create entry (editor+)
GET    /api/v1/apps/:app/content/:schema/:id               # Get entry
PUT    /api/v1/apps/:app/content/:schema/:id               # Update entry (editor+)
DELETE /api/v1/apps/:app/content/:schema/:id               # Delete entry (editor+)
POST   /api/v1/apps/:app/content/:schema/:id/publish       # Publish entry (editor+)
POST   /api/v1/apps/:app/content/:schema/:id/unpublish     # Unpublish entry (editor+)
```

### Single-kind schemas

```
GET    /api/v1/apps/:app/content/:schema/single   # Get the single entry
PUT    /api/v1/apps/:app/content/:schema/single   # Upsert single entry (editor+)
DELETE /api/v1/apps/:app/content/:schema/single   # Delete single entry (editor+)
```

### Uploads

```
POST /api/v1/apps/:app/uploads          # Upload file (multipart, editor+)
GET  /api/v1/apps/:app/uploads/:hash    # Download file by hash
```

### Sync

```
POST /api/v1/apps/:app/export    # Export app data as tar.gz (admin)
POST /api/v1/apps/:app/import    # Import tar.gz bundle (admin, multipart)
```

### Deployments

```
GET  /api/v1/apps/:app/deployments              # List deployment targets
POST /api/v1/apps/:app/deployments/:slug/fire   # Trigger deployment webhook (editor+)
```

### Global endpoints (not app-scoped)

```
GET  /api/v1/openapi.json       # Auto-generated OpenAPI spec
GET  /api/v1/backups/status     # Backup status (admin)
POST /api/v1/backups/trigger    # Trigger manual backup (admin)
GET  /metrics                   # Prometheus metrics (unauthenticated)
GET  /healthz                   # Health check (unauthenticated)
```

## Roles

| Role | Permissions |
|------|-------------|
| admin | Full access: users, apps, deployments, backups, import/export |
| editor | CRUD content, uploads, fire deployments, create API tokens |
| viewer | Read-only access to schemas, content, uploads |

## Schema Format

```json
{{
  "x-substrukt": {{
    "title": "Blog Posts",
    "slug": "blog-posts",
    "storage": "directory",
    "kind": "collection"
  }},
  "type": "object",
  "properties": {{
    "title": {{ "type": "string" }},
    "body": {{ "type": "string", "format": "textarea" }},
    "cover": {{ "type": "string", "format": "upload" }}
  }},
  "required": ["title"]
}}
```

- **kind**: `collection` (multiple entries) or `single` (one entry, e.g. site settings)
- **storage**: `directory` (one file per entry) or `single-file` (all in one JSON array)
- **format: "upload"**: File upload field (stored as `{{hash, filename, mime}}` object)
- **format: "textarea"**: Multi-line text input
- **format: "reference"**: Cross-schema reference with `x-substrukt-reference`

## Common Workflows

### Create content type and add entries
```bash
# 1. Create a schema via the web UI or by placing a .json file in data/<app>/schemas/
# 2. Create content via API:
curl -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{{"title": "Hello World"}}' \
  http://localhost:3000/api/v1/apps/default/content/blog-posts
```

### Export and import between environments
```bash
# Export from local
substrukt export backup.tar.gz --app default

# Import to production via API
curl -X POST -H "Authorization: Bearer $TOKEN" \
  -F "bundle=@backup.tar.gz" \
  https://cms.example.com/api/v1/apps/default/import
```

### Trigger a deployment
```bash
curl -X POST -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/apps/default/deployments/production/fire
```

## Web UI Pages

| Path | Description |
|------|-------------|
| `/apps` | App list (dashboard) |
| `/apps/:app/schemas` | Schema management |
| `/apps/:app/content/:schema` | Content list / editor |
| `/apps/:app/uploads` | Upload browser |
| `/apps/:app/deployments` | Deployment targets |
| `/apps/:app/settings` | App settings, API tokens, import/export |
| `/settings/users` | User management (admin) |
| `/settings/profile` | Password change (all roles) |
| `/settings/audit-log` | Audit log viewer (admin) |
| `/settings/backups` | S3 backup config (admin) |
"#,
        data_dir = config.data_dir.display(),
    )
}

pub fn onboard_output() -> &'static str {
    r#"Substrukt Onboarding

Add this to your CLAUDE.md or AGENTS.md:

--- BEGIN SNIPPET ---
## CMS (Substrukt)

This project uses Substrukt as its CMS.
Run `substrukt prime` for full AI workflow context.

**Quick reference:**
- `substrukt serve` — Start the server (default port 3000)
- `substrukt prime` — Output AI-optimized workflow context
- API base: `http://localhost:3000/api/v1/apps/<app-slug>/`
- Auth: `Authorization: Bearer <token>` header
--- END SNIPPET ---

How it works:
  • `substrukt prime` provides dynamic workflow context (~150 lines)
  • AGENTS.md only needs this minimal pointer
  • The prime output covers CLI, API, schemas, roles, and common workflows
"#
}
