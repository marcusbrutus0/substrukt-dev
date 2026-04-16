# allowthem Auth Integration Design

Replace substrukt's built-in authentication system with allowthem, an embeddable auth library. allowthem handles user identity, sessions, roles, API tokens, invitations, and auth audit logging. substrukt retains app-level authorization (app_access join table), content audit logging, CSRF (except login), and rate limiting.

## Approach

Big-bang migration on a feature branch. Add allowthem-core as a dependency, rewrite auth middleware/extractors, migrate existing data, update all routes, remove old auth code. One branch, atomic commits per logical piece.

## Dependency & State Setup

Add `allowthem-core` as a path dependency (`../allowthem/crates/core`). Remove `argon2`.

AppState gains two new fields:

```rust
pub struct AppState {
    // ... existing fields
    pub ath: AllowThem,                        // direct DB ops (create_user, etc.)
    pub auth_client: Arc<EmbeddedAuthClient>,  // session/role validation
}
```

Built during startup via `AllowThemBuilder::with_pool(pool.clone())` — shares substrukt's existing SQLite pool. Allowthem auto-creates its `allowthem_`-prefixed tables via its own migrations.

On startup, bootstrap roles: ensure "admin", "editor", "viewer" roles exist in allowthem via `create_role()` (idempotent, skip if exists).

## Session Handling — Two Cookies

Two cookies coexist:

- `allowthem_session` — auth identity, managed by allowthem (SHA-256 hashed token in DB, sliding-window renewal)
- tower-sessions cookie — UI state only (flash messages, CSRF tokens)

## Auth Middleware

### require_auth

Rewritten to:

1. Read `allowthem_session` cookie from request
2. Call `auth_client.validate_session(token)` -> `Option<User>` (allowthem's User type)
3. If valid: stash allowthem `User` in request extensions
4. If invalid/missing: redirect to `/login` (htmx-aware)
5. Public path exemptions unchanged: `/login`, `/setup`, `/signup`, `/api/*`, `/healthz`, `/metrics`

### verify_csrf

Unchanged — still reads `_csrf` from tower-sessions, constant-time comparison. Only exception: the login form uses allowthem's double-submit cookie pattern.

### Role checking

Changes from `auth::require_role(&session, "editor")` to:

1. Extract allowthem `User` from request extensions
2. Call `auth_client.check_role(user_id, role_name)`
3. Hits allowthem's `user_roles` join table

### AppContext extractor

Still checks substrukt's `app_access` join table for non-admin users. Gets `user_id` from the allowthem `User` in request extensions instead of tower-session.

## Database Schema Changes

### New substrukt tables

- `app_access(app_id, user_id)` — already exists, stays. `user_id` now references `allowthem_users.id`.
- `app_tokens(api_token_id INTEGER NOT NULL, app_id INTEGER NOT NULL, PRIMARY KEY(api_token_id, app_id))` — new join table mapping allowthem's API tokens to substrukt apps.

### Removed substrukt tables

- `users` — replaced by `allowthem_users`
- `invitations` — replaced by `allowthem_invitations`
- `api_tokens` — replaced by `allowthem_api_tokens`

### Migration strategy

A new sqlx migration that:

1. **Data migration first**: Rust function runs at startup before sqlx migrations. Reads existing `users`, `invitations`, `api_tokens` from substrukt's tables. Creates corresponding records in allowthem's tables via `ath.create_user()`, `ath.create_invitation()`, `ath.create_api_token()`, etc. Maps old user IDs to new allowthem user IDs. Updates `app_access.user_id` references. Populates `app_tokens` from old `api_tokens.app_id`. Assigns roles based on old `users.role` column. **Fails hard if migration fails — do not proceed.**
2. **Schema migration**: sqlx migration drops `users`, `invitations`, `api_tokens` tables. Creates `app_tokens` table.

## Route Changes

### Login/Logout

- `POST /login`: Calls `ath.login(identifier, password)` -> session token. Sets `allowthem_session` cookie. Initializes tower-session for flash/CSRF. Login form uses allowthem's double-submit CSRF pattern.
- `POST /logout`: Calls `auth_client.logout(token)`. Clears allowthem cookie. Flushes tower-session.
- `GET /login`: Renders login template with allowthem CSRF cookie token.

### Setup

- Calls `ath.create_user(email, password, username)` instead of local `create_user()`
- Creates/ensures "admin" role, assigns to new user via `ath.assign_role()`
- Gated behind "no users exist" check via `ath.list_users()`

### Signup/Invitations

- `GET /signup?token=...`: Validates via `ath.validate_invitation(raw_token)`
- `POST /signup`: Creates user via `ath.create_user()`, assigns role from invitation metadata, grants app access in substrukt's `app_access` table
- `POST /settings/users/invite`: Creates invitation via `ath.create_invitation()` with role in metadata

### User management

- List users: `ath.list_users()` + role info via `ath.get_user_roles()`
- Password change: `ath.update_user_password()`
- User deletion: `ath.delete_user()` (cascades in allowthem) + substrukt cleans up `app_access` and `app_tokens` rows

### API tokens

- Create: `ath.create_api_token(user_id, name, expires_at)` + insert `app_tokens(token_id, app_id)`
- List: `ath.list_api_tokens(user_id)` joined with `app_tokens` for app association
- Delete: `ath.delete_api_token(id)` + delete from `app_tokens`

### API bearer auth

- Extract bearer token from header
- Call `ath.validate_api_token(raw)` -> `user_id`
- Look up `app_tokens` to verify token is scoped to requested app
- Get user role via `auth_client.check_role()`

## Audit Logging

### Split by domain

- **Auth events** (login, logout, registration, password change, invitation, token CRUD): Handled by allowthem automatically, stored in `allowthem_audit_log`.
- **Content events** (content CRUD, publish/unpublish, schema changes, deployments, backups): Stay in substrukt's `audit_log` table in `audit.db`.

### Audit UI — tabbed, no merge

- **Tab 1: "Auth Events"** — queries `ath.get_audit_log()` with own pagination
- **Tab 2: "Activity"** — queries substrukt's `audit_log` with own pagination
- Default tab: Activity
- Tabs switch via htmx partial loads

## What Gets Removed

- `db/models.rs`: `User` struct, `hash_password()`, `verify_password()`, all user/invitation/token CRUD functions
- `auth/mod.rs`: `login_user()`, `logout_user()` session helpers
- `auth/token.rs`: Token hash/lookup logic
- `argon2` dependency from `Cargo.toml`
- `users`, `invitations`, `api_tokens` tables (after data migration)

## What Stays Unchanged

- All content routes, schema routes, upload routes, deployment routes, backup routes
- `app_access` logic (reads user_id from allowthem User instead of session)
- tower-sessions for flash messages and CSRF (non-login)
- Rate limiting middleware
- Template rendering (updated to read from allowthem User type where needed)
- Content audit logging in `audit.db`
