# Role-Based Access Control — Design Spec

## Goal

Replace the current all-users-are-equal auth model with three roles: admin, editor, viewer. Gate routes and UI elements based on role.

## Roles

| Role | Content | Schemas | Settings/Users/Tokens | Import/Export | Publish |
|---|---|---|---|---|---|
| admin | full CRUD | full CRUD | full access | yes | yes |
| editor | full CRUD | read-only (view list) | own tokens only | no | yes |
| viewer | read-only (list + view) | read-only (view list) | own tokens only | no | no |

## Decisions

- **Three roles**: admin, editor, viewer
- **API tokens inherit creator's role** — no independent token scopes
- **First/setup user is admin**, all pre-existing users become admin via migration
- **Admins select role at invite time** — new user gets that role on signup
- **Role cached in session** to avoid DB lookups per request

## Database Changes

### Migration: `006_add_roles.sql`

```sql
ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'editor';
UPDATE users SET role = 'admin';

ALTER TABLE invitations ADD COLUMN role TEXT NOT NULL DEFAULT 'editor';
```

- Default is `'editor'` for safety (new users without explicit role assignment)
- All existing users become `'admin'` (they had full access before)

### Model Changes

- `User` struct: add `role: String` field
- `Invitation` struct: add `role: String` field
- `create_user` / `create_user_with_email`: accept role parameter
- `create_invitation`: accept role parameter

## Auth Middleware Changes

### Role Checking

Add to `src/auth/mod.rs`:

- `current_user_role(session) -> Option<String>` — reads role from session
- `login_user(session, user_id, role)` — stores both user_id and role in session
- `require_role(session, min_role) -> Result<i64, Response>` — checks role hierarchy: admin > editor > viewer. Returns user_id on success, 403 on failure.

Replace the hardcoded `require_admin` in settings.rs (which checks `user_id == 1`) with `require_role(session, "admin")`.

### Session Storage

Store role in session alongside user_id:
- `USER_ID_KEY = "user_id"`
- `USER_ROLE_KEY = "user_role"`

Set both on login and signup.

## Route Gating

### Admin-only routes
- `POST /schemas/` (create)
- `GET /schemas/new`
- `GET /schemas/{slug}/edit`
- `POST /schemas/{slug}` (update)
- `PUT /schemas/{slug}` (update — alternate method)
- `DELETE /schemas/{slug}` (delete)
- `GET /settings/users`
- `POST /settings/users/invite`
- `POST /settings/users/invitations/{id}/delete`
- `GET /settings/data`
- `POST /settings/data/import`
- `POST /settings/data/export`

### Editor+ routes (editor and admin)
- `GET /content/{slug}/new`
- `POST /content/{slug}/new`
- `POST /content/{slug}/{id}` (update)
- `DELETE /content/{slug}/{id}` (delete)
- `POST /content/{slug}/{id}/revert/{ts}` (revert)
- `POST /publish/{environment}`
- `POST /uploads` (upload file — part of content creation)
- `POST /settings/tokens` (create own token)
- `POST /settings/tokens/{id}/delete` (delete own token)

### Viewer+ routes (all authenticated users)
- `GET /` (dashboard)
- `GET /schemas/` (list)
- `GET /content/{slug}` (list entries)
- `GET /content/{slug}/{id}/edit` (view entry — form is read-only for viewers)
- `GET /content/{slug}/{id}/history` (view history)
- `GET /uploads`
- `GET /uploads/file/{hash}/{filename}`
- `GET /settings/tokens` (view own tokens)

### API routes
- Look up the API token's `user_id`, then look up that user's role
- Apply the same role gates as the UI routes
- Return 403 for unauthorized operations

## Template Changes

### Pass `user_role` to templates

Make `user_role` available as a template variable in all authenticated pages (via the template context in route handlers or as a global function in `src/templates.rs`).

### Navigation (`_nav.html`)
- Always show: Dashboard, Content schemas (list), Uploads
- Editor+: show Publish
- Admin only: show Schemas (management), Users, Data (import/export)
- Always show: API Tokens (own tokens)

### Content pages
- Viewers: hide "New" button, hide "Edit"/"Delete" buttons, make form fields read-only (or show a read-only view)
- Editors+: show all CRUD buttons

### Schema pages
- Non-admins who visit `/schemas/` see the list but no "New Schema" button
- Non-admins cannot access edit/delete routes (403 from middleware)

## What Does NOT Change

- Upload storage and serving (all authenticated users can view uploads)
- Content storage format (JSON files)
- Session mechanism (tower-sessions)
- CSRF protection
- Rate limiting
- Audit logging format (actor remains user_id string)
- Export/import bundle format
