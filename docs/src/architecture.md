# Architecture

## Overview

Substrukt is a single Rust binary that handles everything: web UI, REST API, file storage, background tasks, and database management. There are no external services beyond the filesystem and embedded SQLite.

```
                          +------------------+
                          |   Web Browser    |
                          | (htmx + twind)   |
                          +--------+---------+
                                   |
                          +--------+---------+
                          |   Axum Router    |
                          |  (tower layers)  |
                          +--------+---------+
                                   |
              +--------------------+--------------------+
              |                    |                     |
     +--------+--------+  +-------+--------+  +---------+---------+
     |  UI Routes       |  |  API Routes    |  |  Metrics/Health   |
     |  (SSR + htmx)    |  |  (/api/v1/*)   |  |  (/metrics)       |
     +--------+---------+  +-------+--------+  +-------------------+
              |                    |
     +--------+--------+  +-------+--------+
     |  Session Auth    |  |  Bearer Token  |
     |  (cookies)       |  |  Auth          |
     +--------+---------+  +-------+--------+
              |                    |
              +--------------------+
                        |
         +--------------+--------------+
         |              |              |
  +------+------+ +-----+-----+ +-----+------+
  |  Schemas    | | Content   | | Uploads    |
  |  (JSON      | | (JSON     | | (SHA-256   |
  |   files)    | |  files)   | |  files)    |
  +------+------+ +-----+-----+ +-----+------+
         |              |              |
         +--------------+--------------+
                        |
                  +-----+------+
                  | Filesystem |
                  | (data dir) |
                  +-----+------+
                        |
         +--------------+--------------+
         |                             |
  +------+------+            +--------+--------+
  | substrukt.db|            |    audit.db     |
  | (users,     |            | (audit log,     |
  |  tokens,    |            |  webhook state) |
  |  uploads)   |            +-----------------+
  +-------------+
```

## Request flow

1. **HTTP request** arrives at the Axum router
2. **Tower layers** run in order: CatchPanic, TraceLayer, metrics tracking, session management
3. **Route matching**: UI routes go through session auth middleware; API routes go through bearer token extraction
4. **CSRF verification** runs for mutating UI requests (POST/PUT/DELETE)
5. **Handler** processes the request, interacting with schemas, content, or uploads
6. **Response** is rendered (HTML via minijinja for UI, JSON for API)

## Key modules

| Module | Responsibility |
|--------|---------------|
| `main.rs` | CLI parsing, server startup, shutdown signal |
| `config.rs` | Configuration struct and directory helpers |
| `state.rs` | Shared application state (AppState) |
| `templates.rs` | minijinja environment with auto-reload |
| `cache.rs` | DashMap content cache, file watcher, populate/rebuild |
| `rate_limit.rs` | Per-IP sliding window rate limiter |
| `metrics.rs` | Prometheus recorder and metrics middleware |
| `audit.rs` | Audit logger with async writes, dirty tracking |
| `webhooks.rs` | Webhook firing and background cron |
| `db/` | SQLite pool initialization and migrations |
| `db/models.rs` | User, ApiToken queries |
| `auth/` | Session management, CSRF, login/logout |
| `auth/token.rs` | Bearer token generation, hashing, extraction |
| `schema/` | Schema file CRUD and validation |
| `schema/models.rs` | SubstruktMeta, StorageMode, Kind types |
| `content/` | Content entry CRUD (directory and single-file) |
| `content/form.rs` | JSON Schema to HTML form generation and parsing |
| `uploads/` | Content-addressed file storage |
| `sync/` | tar.gz export/import |
| `routes/` | All HTTP route handlers |

## Technology choices

| Component | Technology | Why |
|-----------|-----------|-----|
| Web framework | Axum 0.8 | Tower middleware ecosystem, async, type-safe extractors |
| Database | SQLite via sqlx | Embedded, no external service, WAL mode for concurrency |
| Templating | minijinja | Fast, safe, Jinja2-compatible, auto-reload support |
| Frontend | htmx + twind | No build step, minimal JS, responsive styling |
| Content cache | DashMap | Concurrent reads without locks, lock-free updates |
| File watching | notify | Cross-platform filesystem events with debouncing |
| Metrics | metrics + metrics-exporter-prometheus | Standard Prometheus format |
| Password hashing | Argon2 | Memory-hard, recommended by OWASP |
| Session storage | tower-sessions-sqlx-store | SQLite-backed, integrates with Axum |

## Concurrency model

- **tokio** async runtime handles all I/O
- **DashMap** provides lock-free concurrent reads for the content cache
- **Async audit writes** -- audit log entries are spawned as fire-and-forget tasks
- **File watcher** runs in a background thread with debounced events
- **Webhook cron** runs as a background tokio task on a timer
- **Rate limiters** use DashMap for lock-free per-IP tracking

## Data ownership

| Data | Stored in | Managed by |
|------|-----------|------------|
| Users, passwords | `substrukt.db` | `db/models.rs` |
| Sessions | `substrukt.db` | tower-sessions |
| API tokens | `substrukt.db` | `db/models.rs` |
| Upload metadata | `substrukt.db` | `uploads/mod.rs` |
| Upload references | `substrukt.db` | `uploads/mod.rs` |
| Schemas | JSON files | `schema/mod.rs` |
| Content entries | JSON files | `content/mod.rs` |
| Upload files | Binary files | `uploads/mod.rs` |
| Audit log | `audit.db` | `audit.rs` |
| Webhook state | `audit.db` | `audit.rs` |
