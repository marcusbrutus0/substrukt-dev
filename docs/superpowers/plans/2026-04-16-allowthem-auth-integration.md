# allowthem Auth Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace substrukt's built-in auth with allowthem-core, keeping app-level authorization on substrukt's side.

**Architecture:** Upgrade substrukt to sqlx 0.9.0-alpha.1 (same as allowthem). Shared SQLite pool — allowthem's `allowthem_`-prefixed tables live alongside substrukt's tables in the same database. `AllowThemBuilder::with_pool(pool)` shares the pool. `app_access.user_id` and `app_tokens.api_token_id` use TEXT (UUID) columns. A one-time data migration moves existing users into allowthem tables on first startup. API tokens and invitations are NOT migrated (short-lived; users recreate them).

**Tech Stack:** allowthem-core (path dep), allowthem-server (path dep), sqlx 0.9.0-alpha.1 (shared), tower-sessions (kept for flash/CSRF)

**sqlx upgrade notes:** substrukt upgrades from sqlx 0.8 to 0.9.0-alpha.1 to share a pool with allowthem. Key breaking changes to handle: migration API changes, `sqlx-toml` config may be needed, `SqliteConnectOptions` API changes. The upgrade is Task 2.

---

### Task 1: Add `create_user_with_hash` to allowthem-core

Substrukt's existing users have Argon2 password hashes. allowthem's `create_user()` takes plaintext and re-hashes. We need a way to import users with existing hashes.

**Files:**
- Modify: `/home/nambiar/projects/wavefunk/allowthem/crates/core/users.rs`

- [ ] **Step 1: Add the method to Db**

In `crates/core/users.rs`, add after `create_user`:

```rust
/// Import a user with a pre-existing password hash (for migration from external systems).
/// The hash must be a valid Argon2 PHC string. No validation is performed on it.
pub async fn create_user_with_hash(
    &self,
    email: Email,
    password_hash: &str,
    username: Option<Username>,
) -> Result<User, AuthError> {
    let id = UserId::new();
    let now = Utc::now();
    let uname = username.as_ref().map(|u| u.as_str());

    sqlx::query(
        "INSERT INTO allowthem_users (id, email, username, password_hash, email_verified, is_active, created_at, updated_at)
         VALUES (?, ?, ?, ?, FALSE, TRUE, ?, ?)"
    )
    .bind(id)
    .bind(email.as_str())
    .bind(uname)
    .bind(password_hash)
    .bind(now)
    .bind(now)
    .execute(self.pool())
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db) if db.message().contains("UNIQUE") => {
            AuthError::Conflict(format!("user already exists: {}", email.as_str()))
        }
        other => AuthError::Database(other),
    })?;

    self.get_user(id).await
}
```

- [ ] **Step 2: Run allowthem tests**

Run: `cd /home/nambiar/projects/wavefunk/allowthem && cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
cd /home/nambiar/projects/wavefunk/allowthem
git add crates/core/users.rs
git commit -m "feat: add create_user_with_hash for migration imports"
```

---

### Task 2: Dependencies, sqlx upgrade, and state setup

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/state.rs`
- Modify: `src/db/mod.rs` (if sqlx 0.9 API changes require it)

- [ ] **Step 1: Update Cargo.toml dependencies**

Upgrade sqlx and add allowthem:

```toml
# Change:
sqlx = { version = "0.9.0-alpha.1", features = ["runtime-tokio", "sqlite", "migrate", "uuid", "chrono"] }
# Add:
allowthem-core = { path = "../allowthem/crates/core" }
allowthem-server = { path = "../allowthem/crates/server" }
# Remove:
# argon2 = "0.5"
```

Also check if `tower-sessions-sqlx-store` 0.15 is compatible with sqlx 0.9. If not, find the compatible version or replace session store. tower-sessions may need updating too.

- [ ] **Step 1b: Fix sqlx 0.9 compilation issues**

Run `cargo check` and fix any sqlx API breakages. Common changes:
- `SqliteConnectOptions` import path may differ
- `sqlx::migrate!()` macro usage may differ
- Query return types may change
- Pool creation API may change

Fix `src/db/mod.rs` `init_pool()` if needed. Fix all `sqlx::query!` calls if the macro API changed. This may require adding a `sqlx.toml` config file.

Run: `cargo check` and iterate until clean.

- [ ] **Step 2: Update AppState**

Replace `src/state.rs` with:

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use allowthem_core::{AllowThem, AuthClient, EmbeddedAuthClient};
use dashmap::DashMap;
use metrics_exporter_prometheus::PrometheusHandle;
use minijinja_autoreload::AutoReloader;
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::audit::AuditLogger;
use crate::backup::S3Config;
use crate::config::Config;
use crate::rate_limit::RateLimiter;

pub type ContentCache = DashMap<String, serde_json::Value>;
pub type EtagCache = DashMap<String, String>;
pub type OpenApiCache = Arc<std::sync::RwLock<Option<serde_json::Value>>>;

pub struct AppStateInner {
    pub pool: SqlitePool,
    pub config: Config,
    pub templates: AutoReloader,
    pub cache: ContentCache,
    pub etag_cache: EtagCache,
    pub login_limiter: RateLimiter,
    pub api_limiter: RateLimiter,
    pub metrics_handle: PrometheusHandle,
    pub audit: AuditLogger,
    pub http_client: reqwest::Client,
    pub deploy_tasks: DashMap<i64, CancellationToken>,
    pub s3_config: Option<S3Config>,
    pub backup_trigger: Option<mpsc::Sender<()>>,
    pub backup_running: AtomicBool,
    pub backup_cancel: Option<CancellationToken>,
    pub openapi_cache: OpenApiCache,
    pub ath: AllowThem,
    pub auth_client: Arc<dyn AuthClient>,
    pub has_users: AtomicBool,
}

pub type AppState = Arc<AppStateInner>;

impl axum::extract::FromRef<AppState> for AllowThem {
    fn from_ref(state: &AppState) -> Self {
        state.ath.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn AuthClient> {
    fn from_ref(state: &AppState) -> Self {
        state.auth_client.clone()
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Fails with errors in main.rs (AppStateInner construction missing new fields). That's expected — Task 3 fixes it.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/state.rs
git commit -m "feat: add allowthem deps, update AppState with auth fields"
```

---

### Task 3: Startup wiring and role bootstrap

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add allowthem initialization to run_server**

After the audit logger init (line ~188), add allowthem setup:

```rust
    // allowthem auth system (shares substrukt's pool)
    let ath = allowthem_core::AllowThemBuilder::with_pool(pool.clone())
        .cookie_secure(config.secure_cookies)
        .build()
        .await
        .expect("Failed to initialize allowthem");

    // Bootstrap roles (idempotent)
    for role_name in ["admin", "editor", "viewer"] {
        let rn = allowthem_core::RoleName::new(role_name);
        if ath.db().get_role_by_name(&rn).await.unwrap_or(None).is_none() {
            ath.db().create_role(&rn, None).await
                .expect("Failed to create role");
        }
    }

    // Check if any users exist (for setup redirect)
    let has_users = !ath.db().list_users().await
        .unwrap_or_default()
        .is_empty();

    let auth_client: Arc<dyn allowthem_core::AuthClient> =
        Arc::new(allowthem_core::EmbeddedAuthClient::new(ath.clone(), "/login"));
```

- [ ] **Step 2: Add new fields to AppState construction**

In the `Arc::new(AppStateInner { ... })` block, add:

```rust
        ath,
        auth_client,
        has_users: AtomicBool::new(has_users),
```

- [ ] **Step 3: Add imports to main.rs**

Add at top:

```rust
use std::sync::atomic::AtomicBool;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: Should compile (other files may warn about unused fields, that's fine).

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire up allowthem init, role bootstrap, has_users flag"
```

---

### Task 4: Auth middleware rewrite

**Files:**
- Modify: `src/auth/mod.rs`

- [ ] **Step 1: Rewrite auth/mod.rs**

Replace the entire file with:

```rust
pub mod token;

use axum::{
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use tower_sessions::Session;

use crate::state::AppState;

/// Return a redirect response that works correctly with htmx.
fn htmx_aware_redirect(request: &Request, location: &str) -> Response {
    let is_htmx = request.headers().get("HX-Request").is_some();
    if is_htmx {
        (
            [(
                axum::http::header::HeaderName::from_static("hx-redirect"),
                axum::http::header::HeaderValue::from_str(location)
                    .expect("redirect location is valid header value"),
            )],
            "",
        )
            .into_response()
    } else {
        Redirect::to(location).into_response()
    }
}

const FLASH_KEY: &str = "_flash";
const CSRF_KEY: &str = "_csrf";

/// Get the current authenticated user from request extensions.
/// Set by require_auth middleware when session is valid.
pub fn current_user(request: &Request) -> Option<&allowthem_core::User> {
    request.extensions().get::<allowthem_core::User>()
}

/// Get cached user role from request extensions.
/// Set by require_auth middleware alongside the User.
pub fn current_user_role_from_ext(extensions: &axum::http::Extensions) -> Option<String> {
    extensions.get::<CurrentUserRole>().map(|r| r.0.clone())
}

/// Newtype to store the user's primary role in request extensions.
#[derive(Clone)]
pub struct CurrentUserRole(pub String);

/// Check that the current user (from extensions) has at least the given role level.
/// Role hierarchy: admin > editor > viewer.
/// Returns the user's UUID on success, or a 403 response on failure.
pub fn require_role(
    extensions: &axum::http::Extensions,
    min_role: &str,
) -> axum::response::Result<allowthem_core::UserId> {
    let user = extensions
        .get::<allowthem_core::User>()
        .ok_or(axum::response::ErrorResponse::from(
            Redirect::to("/login").into_response(),
        ))?;
    let role = extensions
        .get::<CurrentUserRole>()
        .map(|r| r.0.as_str())
        .unwrap_or("");

    let role_level = |r: &str| -> u8 {
        match r {
            "admin" => 3,
            "editor" => 2,
            "viewer" => 1,
            _ => 0,
        }
    };

    if role_level(role) >= role_level(min_role) {
        Ok(user.id)
    } else {
        Err(axum::response::ErrorResponse::from(
            (
                axum::http::StatusCode::FORBIDDEN,
                "Insufficient permissions",
            )
                .into_response(),
        ))
    }
}

/// Store a flash message in the tower-session.
pub async fn set_flash(session: &Session, kind: &str, message: &str) {
    let flash = serde_json::json!({"kind": kind, "message": message});
    let _ = session.insert(FLASH_KEY, flash).await;
}

/// Consume and return the flash message from the tower-session, if any.
pub async fn take_flash(session: &Session) -> Option<(String, String)> {
    if let Ok(Some(flash)) = session.get::<serde_json::Value>(FLASH_KEY).await {
        let _ = session.remove::<serde_json::Value>(FLASH_KEY).await;
        let kind = flash["kind"].as_str().unwrap_or("info").to_string();
        let message = flash["message"].as_str().unwrap_or("").to_string();
        Some((kind, message))
    } else {
        None
    }
}

/// Get or create a CSRF token for the tower-session.
pub async fn ensure_csrf_token(session: &Session) -> String {
    if let Ok(Some(token)) = session.get::<String>(CSRF_KEY).await {
        return token;
    }
    let token = hex::encode(rand::random::<[u8; 32]>());
    let _ = session.insert(CSRF_KEY, &token).await;
    token
}

/// Verify a submitted CSRF token against the tower-session.
pub async fn verify_csrf_token(session: &Session, submitted: &str) -> bool {
    if let Ok(Some(expected)) = session.get::<String>(CSRF_KEY).await {
        if expected.len() != submitted.len() {
            return false;
        }
        use subtle::ConstantTimeEq;
        expected.as_bytes().ct_eq(submitted.as_bytes()).into()
    } else {
        false
    }
}

/// Middleware: verify CSRF token on mutating requests (POST/PUT/DELETE).
pub async fn verify_csrf(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }

    let session = match request.extensions().get::<Session>().cloned() {
        Some(s) => s,
        None => return next.run(request).await,
    };

    if let Some(token) = request
        .headers()
        .get("X-CSRF-Token")
        .and_then(|v| v.to_str().ok())
    {
        if verify_csrf_token(&session, token).await {
            return next.run(request).await;
        }
        return csrf_error_response(&state);
    }

    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("application/x-www-form-urlencoded") {
        let (parts, body) = request.into_parts();
        let bytes = match axum::body::to_bytes(body, 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => {
                return (axum::http::StatusCode::BAD_REQUEST, "Body too large").into_response();
            }
        };

        let body_str = std::str::from_utf8(&bytes).unwrap_or("");
        let csrf_value = body_str
            .split('&')
            .find_map(|pair| pair.strip_prefix("_csrf="));

        if let Some(token) = csrf_value
            && verify_csrf_token(&session, token).await
        {
            let request = Request::from_parts(parts, axum::body::Body::from(bytes));
            return next.run(request).await;
        }

        return csrf_error_response(&state);
    }

    if content_type.starts_with("multipart/form-data") {
        return next.run(request).await;
    }

    next.run(request).await
}

fn csrf_error_response(state: &AppState) -> Response {
    use axum::response::Html;
    let html = crate::routes::render_error(
        state,
        403,
        "Your session may have expired. Please go back and try again.",
        false,
    );
    (axum::http::StatusCode::FORBIDDEN, Html(html)).into_response()
}

/// Middleware: validate allowthem session cookie. Redirect to /setup or /login if needed.
/// On success, inserts `allowthem_core::User` and `CurrentUserRole` into request extensions.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();

    // Allow public paths
    if path.starts_with("/login")
        || path.starts_with("/setup")
        || path.starts_with("/signup")
        || path.starts_with("/api/")
    {
        return next.run(request).await;
    }

    // Redirect to setup if no users exist
    if !state.has_users.load(std::sync::atomic::Ordering::Relaxed) {
        return htmx_aware_redirect(&request, "/setup");
    }

    // Parse allowthem session cookie
    let cookie_header = request
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok());

    let token = cookie_header.and_then(|h| {
        allowthem_core::parse_session_cookie(h, state.auth_client.session_cookie_name())
    });

    let token = match token {
        Some(t) => t,
        None => return htmx_aware_redirect(&request, "/login"),
    };

    // Validate session
    let user = match state.auth_client.validate_session(&token).await {
        Ok(Some(u)) => u,
        _ => return htmx_aware_redirect(&request, "/login"),
    };

    // Resolve primary role (first matching: admin > editor > viewer)
    let role = resolve_user_role(&state, &user.id).await;

    request.extensions_mut().insert(CurrentUserRole(role));
    request.extensions_mut().insert(user);
    next.run(request).await
}

/// Determine the user's highest role. Checks admin > editor > viewer.
async fn resolve_user_role(state: &AppState, user_id: &allowthem_core::UserId) -> String {
    for role_name in ["admin", "editor", "viewer"] {
        let rn = allowthem_core::RoleName::new(role_name);
        if state.auth_client.check_role(user_id, &rn).await.unwrap_or(false) {
            return role_name.to_string();
        }
    }
    String::new()
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Errors in files that still use old auth functions (`login_user`, `current_user_id`, etc.). That's expected — subsequent tasks fix those callers.

- [ ] **Step 3: Commit**

```bash
git add src/auth/mod.rs
git commit -m "feat: rewrite auth middleware to use allowthem sessions"
```

---

### Task 5: Rewrite login, logout, setup routes

**Files:**
- Modify: `src/routes/auth.rs`

- [ ] **Step 1: Rewrite routes/auth.rs**

Replace the entire file. Key changes:
- Login calls `ath.db().find_for_login()` + `allowthem_core::verify_password()` + creates allowthem session + sets cookie
- Logout calls `auth_client.logout()` + clears cookie + flushes tower-session
- Setup calls `ath.db().create_user()` + assigns admin role + creates session
- `has_users` AtomicBool flipped to true after first user creation

```rust
use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use tower_sessions::Session;

use crate::auth;
use crate::state::AppState;

pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
        .route("/setup", get(setup_page).post(setup_submit))
        .route("/signup", get(signup_page).post(signup_submit))
}

#[derive(serde::Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[derive(serde::Deserialize)]
struct SetupForm {
    username: String,
    password: String,
    confirm_password: String,
}

async fn login_page(
    session: Session,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let csrf_token = auth::ensure_csrf_token(&session).await;
    render_template(&state, "login.html", minijinja::context! {
        csrf_token => csrf_token,
    })
}

async fn login_submit(
    session: Session,
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
    let ip = "direct".to_string(); // simplified; could extract from headers
    // Rate limiting
    if !state.login_limiter.check(&ip) {
        let csrf_token = auth::ensure_csrf_token(&session).await;
        return render_template(&state, "login.html", minijinja::context! {
            csrf_token => csrf_token,
            error => "Too many login attempts. Please try again later.",
        }).into_response();
    }

    // Find user by email or username
    let user = match state.ath.db().find_for_login(&form.username).await {
        Ok(u) => u,
        Err(_) => {
            let csrf_token = auth::ensure_csrf_token(&session).await;
            return render_template(&state, "login.html", minijinja::context! {
                csrf_token => csrf_token,
                error => "Invalid username or password.",
            }).into_response();
        }
    };

    // Verify password
    let hash = match &user.password_hash {
        Some(h) => h,
        None => {
            let csrf_token = auth::ensure_csrf_token(&session).await;
            return render_template(&state, "login.html", minijinja::context! {
                csrf_token => csrf_token,
                error => "Invalid username or password.",
            }).into_response();
        }
    };

    match allowthem_core::verify_password(&form.password, hash) {
        Ok(true) => {}
        _ => {
            let csrf_token = auth::ensure_csrf_token(&session).await;
            return render_template(&state, "login.html", minijinja::context! {
                csrf_token => csrf_token,
                error => "Invalid username or password.",
            }).into_response();
        }
    }

    // Create allowthem session
    let token = allowthem_core::generate_token();
    let token_hash = allowthem_core::hash_token(&token);
    let expires = chrono::Utc::now() + state.ath.session_config().ttl;
    if let Err(e) = state.ath.db().create_session(
        user.id, token_hash, None, None, expires,
    ).await {
        tracing::error!("Failed to create session: {e}");
        let csrf_token = auth::ensure_csrf_token(&session).await;
        return render_template(&state, "login.html", minijinja::context! {
            csrf_token => csrf_token,
            error => "Login failed. Please try again.",
        }).into_response();
    }

    // Audit log
    let _ = state.ath.db().log_audit(
        allowthem_core::AuditEvent::Login,
        Some(&user.id), None, None, None, None,
    ).await;

    // Set cookie and redirect
    let cookie = state.ath.session_cookie(&token);
    let mut resp = Redirect::to("/apps").into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie.parse().unwrap(),
    );
    resp
}

async fn logout(
    State(state): State<AppState>,
    session: Session,
    request: axum::extract::Request,
) -> Response {
    // Parse and invalidate allowthem session
    if let Some(cookie_header) = request.headers().get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = allowthem_core::parse_session_cookie(
            cookie_header, state.auth_client.session_cookie_name()
        ) {
            let _ = state.auth_client.logout(&token).await;
        }
    }

    // Flush tower-session (flash/CSRF)
    let _ = session.flush().await;

    // Clear allowthem cookie by setting max-age=0
    let clear_cookie = format!(
        "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax",
        state.auth_client.session_cookie_name()
    );
    let mut resp = Redirect::to("/login").into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        clear_cookie.parse().unwrap(),
    );
    resp
}

async fn setup_page(
    session: Session,
    State(state): State<AppState>,
) -> Response {
    if state.has_users.load(std::sync::atomic::Ordering::Relaxed) {
        return Redirect::to("/login").into_response();
    }
    let csrf_token = auth::ensure_csrf_token(&session).await;
    render_template(&state, "setup.html", minijinja::context! {
        csrf_token => csrf_token,
    }).into_response()
}

async fn setup_submit(
    session: Session,
    State(state): State<AppState>,
    Form(form): Form<SetupForm>,
) -> Response {
    if state.has_users.load(std::sync::atomic::Ordering::Relaxed) {
        return Redirect::to("/login").into_response();
    }

    // Validate
    if form.password != form.confirm_password {
        let csrf_token = auth::ensure_csrf_token(&session).await;
        return render_template(&state, "setup.html", minijinja::context! {
            csrf_token => csrf_token,
            error => "Passwords do not match.",
        }).into_response();
    }
    if form.password.len() < 8 {
        let csrf_token = auth::ensure_csrf_token(&session).await;
        return render_template(&state, "setup.html", minijinja::context! {
            csrf_token => csrf_token,
            error => "Password must be at least 8 characters.",
        }).into_response();
    }

    // Create user in allowthem — use username as email placeholder if needed
    let email = match allowthem_core::Email::new(format!("{}@local", form.username)) {
        Ok(e) => e,
        Err(_) => {
            let csrf_token = auth::ensure_csrf_token(&session).await;
            return render_template(&state, "setup.html", minijinja::context! {
                csrf_token => csrf_token,
                error => "Invalid username.",
            }).into_response();
        }
    };
    let username = allowthem_core::Username::new(form.username.clone());

    let user = match state.ath.db().create_user(email, &form.password, Some(username)).await {
        Ok(u) => u,
        Err(e) => {
            let csrf_token = auth::ensure_csrf_token(&session).await;
            return render_template(&state, "setup.html", minijinja::context! {
                csrf_token => csrf_token,
                error => format!("Failed to create user: {e}"),
            }).into_response();
        }
    };

    // Assign admin role
    let admin_role_name = allowthem_core::RoleName::new("admin");
    if let Some(role) = state.ath.db().get_role_by_name(&admin_role_name).await.unwrap_or(None) {
        let _ = state.ath.db().assign_role(&user.id, &role.id).await;
    }

    // Mark that users exist
    state.has_users.store(true, std::sync::atomic::Ordering::Relaxed);

    // Create session and set cookie
    let token = allowthem_core::generate_token();
    let token_hash = allowthem_core::hash_token(&token);
    let expires = chrono::Utc::now() + state.ath.session_config().ttl;
    let _ = state.ath.db().create_session(user.id, token_hash, None, None, expires).await;

    let _ = state.ath.db().log_audit(
        allowthem_core::AuditEvent::Register,
        Some(&user.id), None, None, None, None,
    ).await;

    let cookie = state.ath.session_cookie(&token);
    let mut resp = Redirect::to("/apps").into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie.parse().unwrap(),
    );
    resp
}

// signup_page and signup_submit are placeholders — Task 6 implements them fully.
async fn signup_page() -> impl IntoResponse {
    "signup placeholder"
}
async fn signup_submit() -> impl IntoResponse {
    "signup placeholder"
}

fn render_template(state: &AppState, template: &str, ctx: minijinja::Value) -> Html<String> {
    let Ok(env) = state.templates.acquire_env() else {
        return Html("<h1>500</h1><p>Template error</p>".to_string());
    };
    match env.get_template(template) {
        Ok(tmpl) => Html(tmpl.render(ctx).unwrap_or_else(|e| {
            format!("<h1>500</h1><p>Render error: {e}</p>")
        })),
        Err(e) => Html(format!("<h1>500</h1><p>Template not found: {e}</p>")),
    }
}

pub fn client_ip(headers: &axum::http::HeaderMap, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = xff.split(',').next() {
                return first.trim().to_string();
            }
        }
    }
    "direct".to_string()
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: May have errors from other files referencing old auth functions. Focus on this file compiling.

- [ ] **Step 3: Commit**

```bash
git add src/routes/auth.rs
git commit -m "feat: rewrite login/logout/setup to use allowthem"
```

---

### Task 6: Rewrite invitation and signup routes

**Files:**
- Modify: `src/routes/auth.rs` (replace signup placeholders)

- [ ] **Step 1: Implement signup_page**

Replace the placeholder `signup_page` in `src/routes/auth.rs`:

```rust
#[derive(serde::Deserialize)]
struct SignupQuery {
    token: Option<String>,
}

#[derive(serde::Deserialize)]
struct SignupForm {
    token: String,
    username: String,
    password: String,
    confirm_password: String,
}

async fn signup_page(
    session: Session,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<SignupQuery>,
) -> Response {
    let raw_token = match query.token {
        Some(t) => t,
        None => return render_error_page(&state, "Invalid signup link."),
    };

    let invitation = match state.ath.db().validate_invitation(&raw_token).await {
        Ok(Some(inv)) => inv,
        _ => return render_error_page(&state, "This invitation link is invalid or has expired."),
    };

    let csrf_token = auth::ensure_csrf_token(&session).await;
    let email = invitation.email.as_ref().map(|e| e.as_str()).unwrap_or("");
    render_template(&state, "signup.html", minijinja::context! {
        csrf_token => csrf_token,
        token => raw_token,
        email => email,
    }).into_response()
}

async fn signup_submit(
    session: Session,
    State(state): State<AppState>,
    Form(form): Form<SignupForm>,
) -> Response {
    let ip = "direct".to_string();
    if !state.login_limiter.check(&ip) {
        return render_error_page(&state, "Too many attempts. Please try again later.");
    }

    // Validate invitation
    let invitation = match state.ath.db().validate_invitation(&form.token).await {
        Ok(Some(inv)) => inv,
        _ => return render_error_page(&state, "This invitation link is invalid or has expired."),
    };

    // Validate form
    if form.username.trim().is_empty() {
        return render_signup_error(&state, &session, &form.token, &invitation, "Username is required.").await;
    }
    if form.password.len() < 8 {
        return render_signup_error(&state, &session, &form.token, &invitation, "Password must be at least 8 characters.").await;
    }
    if form.password != form.confirm_password {
        return render_signup_error(&state, &session, &form.token, &invitation, "Passwords do not match.").await;
    }

    // Check username uniqueness
    let username = allowthem_core::Username::new(form.username.clone());
    if state.ath.db().get_user_by_username(&username).await.is_ok() {
        return render_signup_error(&state, &session, &form.token, &invitation, "Username is already taken.").await;
    }

    // Create user
    let email = invitation.email.clone().unwrap_or_else(|| {
        allowthem_core::Email::new(format!("{}@local", form.username)).unwrap()
    });
    let user = match state.ath.db().create_user(email, &form.password, Some(username)).await {
        Ok(u) => u,
        Err(e) => return render_signup_error(&state, &session, &form.token, &invitation, &format!("Failed to create account: {e}")).await,
    };

    // Consume invitation
    let _ = state.ath.db().consume_invitation(invitation.id).await;

    // Assign role from invitation metadata (default: editor)
    let role_str = invitation.metadata.as_deref().unwrap_or("editor");
    let role_name = allowthem_core::RoleName::new(role_str);
    if let Some(role) = state.ath.db().get_role_by_name(&role_name).await.unwrap_or(None) {
        let _ = state.ath.db().assign_role(&user.id, &role.id).await;
    }

    // Auto-grant access to all apps for non-admins
    if role_str != "admin" {
        if let Ok(apps) = crate::db::models::list_apps(&state.pool).await {
            for app in apps {
                let _ = crate::db::models::grant_app_access(
                    &state.pool, app.id, &user.id.to_string(),
                ).await;
            }
        }
    }

    state.has_users.store(true, std::sync::atomic::Ordering::Relaxed);

    // Create session
    let token = allowthem_core::generate_token();
    let token_hash = allowthem_core::hash_token(&token);
    let expires = chrono::Utc::now() + state.ath.session_config().ttl;
    let _ = state.ath.db().create_session(user.id, token_hash, None, None, expires).await;

    let _ = state.ath.db().log_audit(
        allowthem_core::AuditEvent::Register,
        Some(&user.id), None, None, None, None,
    ).await;

    let cookie = state.ath.session_cookie(&token);
    let mut resp = Redirect::to("/apps").into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie.parse().unwrap(),
    );
    resp
}

async fn render_signup_error(
    state: &AppState,
    session: &Session,
    token: &str,
    invitation: &allowthem_core::Invitation,
    error: &str,
) -> Response {
    let csrf_token = auth::ensure_csrf_token(session).await;
    let email = invitation.email.as_ref().map(|e| e.as_str()).unwrap_or("");
    render_template(state, "signup.html", minijinja::context! {
        csrf_token => csrf_token,
        token => token,
        email => email,
        error => error,
    }).into_response()
}

fn render_error_page(state: &AppState, message: &str) -> Response {
    let html = crate::routes::render_error(state, 400, message, false);
    (axum::http::StatusCode::BAD_REQUEST, Html(html)).into_response()
}
```

- [ ] **Step 2: Commit**

```bash
git add src/routes/auth.rs
git commit -m "feat: implement invitation signup flow with allowthem"
```

---

### Task 7: Update db/models.rs — remove old auth models, update app_access

**Files:**
- Modify: `src/db/models.rs`

- [ ] **Step 1: Remove User struct and auth CRUD functions**

Remove from `src/db/models.rs`:
- `User` struct and its `hash_password()`, `verify_password()` impls
- `create_user()`, `find_user_by_username()`, `user_count()`, `list_users()`, `find_user_by_id()`, `update_user_password()`, `create_user_with_email()`, `find_user_by_email()`, `find_user_role()`, `get_username_map()`
- `Invitation` struct and all invitation CRUD functions
- `ApiToken` struct and all token CRUD functions
- Related imports (`argon2`, etc.)

- [ ] **Step 2: Update app_access functions to use TEXT user_id**

Update `grant_app_access`, `revoke_app_access`, `user_has_app_access`, `list_app_users` to accept `&str` user_id instead of `i64`:

```rust
pub async fn grant_app_access(pool: &SqlitePool, app_id: i64, user_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO app_access (app_id, user_id) VALUES (?, ?)")
        .bind(app_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn revoke_app_access(pool: &SqlitePool, app_id: i64, user_id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM app_access WHERE app_id = ? AND user_id = ?")
        .bind(app_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn user_has_app_access(pool: &SqlitePool, app_id: i64, user_id: &str) -> sqlx::Result<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM app_access WHERE app_id = ? AND user_id = ?",
    )
    .bind(app_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}
```

- [ ] **Step 3: Add app_tokens CRUD functions**

```rust
pub async fn create_app_token(pool: &SqlitePool, api_token_id: &str, app_id: i64) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO app_tokens (api_token_id, app_id) VALUES (?, ?)")
        .bind(api_token_id)
        .bind(app_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_app_for_token(pool: &SqlitePool, api_token_id: &str) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar("SELECT app_id FROM app_tokens WHERE api_token_id = ?")
        .bind(api_token_id)
        .fetch_optional(pool)
        .await
}

pub async fn list_app_tokens(pool: &SqlitePool, app_id: i64) -> sqlx::Result<Vec<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT api_token_id FROM app_tokens WHERE app_id = ?",
    )
    .bind(app_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn delete_app_token(pool: &SqlitePool, api_token_id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM app_tokens WHERE api_token_id = ?")
        .bind(api_token_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 4: Remove auth-related tests from models.rs**

Remove test functions that test User CRUD, password hashing, token CRUD, invitation CRUD. Keep tests for App CRUD, app_access, and the new app_tokens functions.

- [ ] **Step 5: Commit**

```bash
git add src/db/models.rs
git commit -m "refactor: remove old auth models, update app_access to TEXT user_id, add app_tokens"
```

---

### Task 8: Rewrite BearerToken extractor

**Files:**
- Modify: `src/auth/token.rs`

- [ ] **Step 1: Replace token.rs**

```rust
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};

use crate::db::models;
use crate::state::AppState;

/// Bearer token extractor for API routes.
/// Validates the token via allowthem, then checks app scoping via substrukt's app_tokens table.
pub struct BearerToken {
    pub user: allowthem_core::User,
    pub role: String,
    pub app_id: Option<i64>,
}

impl FromRequestParts<AppState> for BearerToken {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let unauthorized = || {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        };

        // Extract bearer token
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(unauthorized)?;

        let raw_token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(unauthorized)?;

        // Validate via allowthem
        let user_id = state
            .ath
            .db()
            .validate_api_token(raw_token)
            .await
            .map_err(|_| unauthorized())?
            .ok_or_else(unauthorized)?;

        let user = state
            .ath
            .db()
            .get_user(user_id)
            .await
            .map_err(|_| unauthorized())?;

        if !user.is_active {
            return Err(unauthorized());
        }

        // Get the allowthem token info to find its ID for app scoping
        // We need the token ID. allowthem's validate_api_token only returns user_id.
        // Look up app association: hash the raw token and find it in app_tokens.
        // Actually, we need the allowthem token ID. Let's get it from the user's tokens list.
        // For now, look up app_id by checking all the user's tokens against app_tokens.
        let token_id = find_token_id_by_raw(&state.ath, raw_token).await;
        let app_id = match &token_id {
            Some(tid) => models::find_app_for_token(&state.pool, tid).await.unwrap_or(None),
            None => None,
        };

        // Resolve role
        let role = crate::auth::resolve_user_role(state, &user.id).await;

        Ok(BearerToken { user, role, app_id })
    }
}

/// Find the allowthem ApiTokenId for a raw token by checking the user's token list.
/// This is needed because validate_api_token only returns UserId, not the token ID.
async fn find_token_id_by_raw(ath: &allowthem_core::AllowThem, raw_token: &str) -> Option<String> {
    // Hash the raw token the same way allowthem does
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    let _hash = hex::encode(hasher.finalize());

    // We can't easily get the token ID from just the hash without allowthem exposing it.
    // Workaround: store the allowthem token ID in app_tokens at creation time.
    // The token ID is returned by create_api_token and stored in app_tokens.
    // At validation time, we validate via allowthem (get user_id), then we need the token_id
    // to look up app scoping. We'll need to add a method or use a different approach.
    //
    // Simplest: store token_hash -> api_token_id mapping in app_tokens.
    // Or: add validate_api_token_full to allowthem that returns (UserId, ApiTokenId).
    //
    // For now: we'll store api_token_id in app_tokens at creation time, and at
    // validation time we check which app the token belongs to by listing user tokens
    // and matching. This is O(n) but tokens per user is small.
    None // Placeholder — see Task 8 step 2 for the fix
}

pub fn require_api_role(token: &BearerToken, min_role: &str) -> Result<(), Response> {
    let role_level = |r: &str| -> u8 {
        match r {
            "admin" => 3,
            "editor" => 2,
            "viewer" => 1,
            _ => 0,
        }
    };
    if role_level(&token.role) >= role_level(min_role) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Forbidden"})),
        )
            .into_response())
    }
}

pub fn require_token_app(token: &BearerToken, app_id: i64) -> Result<(), Response> {
    match token.app_id {
        Some(tid) if tid == app_id => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Token not scoped to this app"})),
        )
            .into_response()),
    }
}
```

- [ ] **Step 2: Fix token-to-app lookup**

The above has a gap: we need to map a raw bearer token to its `app_tokens` entry. The cleanest approach: store `token_hash` in `app_tokens` alongside `api_token_id`.

Update `app_tokens` schema to include `token_hash`:

In `src/db/models.rs`, update the app_tokens functions:

```rust
pub async fn create_app_token(
    pool: &SqlitePool,
    api_token_id: &str,
    app_id: i64,
    token_hash: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO app_tokens (api_token_id, app_id, token_hash) VALUES (?, ?, ?)"
    )
    .bind(api_token_id)
    .bind(app_id)
    .bind(token_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_app_for_token_hash(pool: &SqlitePool, token_hash: &str) -> sqlx::Result<Option<(String, i64)>> {
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT api_token_id, app_id FROM app_tokens WHERE token_hash = ?"
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

Then update the BearerToken extractor to compute the hash and look up directly:

```rust
async fn from_request_parts(
    parts: &mut Parts,
    state: &AppState,
) -> Result<Self, Self::Rejection> {
    // ... (auth_header, raw_token extraction as before) ...

    // Validate via allowthem
    let user_id = state.ath.db()
        .validate_api_token(raw_token).await
        .map_err(|_| unauthorized())?
        .ok_or_else(unauthorized)?;

    let user = state.ath.db().get_user(user_id).await
        .map_err(|_| unauthorized())?;
    if !user.is_active {
        return Err(unauthorized());
    }

    // Look up app scoping via token hash
    use sha2::{Sha256, Digest};
    let hash = hex::encode(Sha256::digest(raw_token.as_bytes()));
    let app_id = models::find_app_for_token_hash(&state.pool, &hash)
        .await
        .unwrap_or(None)
        .map(|(_, aid)| aid);

    let role = crate::auth::resolve_user_role(state, &user.id).await;

    Ok(BearerToken { user, role, app_id })
}
```

- [ ] **Step 3: Commit**

```bash
git add src/auth/token.rs src/db/models.rs
git commit -m "feat: rewrite bearer token auth with allowthem + app scoping"
```

---

### Task 9: Update AppContext extractor

**Files:**
- Modify: `src/app_context.rs`

- [ ] **Step 1: Rewrite AppContext to use allowthem User from extensions**

```rust
use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Json, Response};
use axum_htmx::HxRequest;
use tower_sessions::Session;

use crate::auth::{CurrentUserRole, ensure_csrf_token};
use crate::config::Config;
use crate::db::models::{self, App};
use crate::routes::render_error_with_nav;
use crate::schema;
use crate::state::AppState;

pub struct AppContext {
    pub app: App,
}

impl AppContext {
    pub fn nav_schemas(&self, config: &Config) -> Vec<minijinja::Value> {
        let schemas_dir = config.app_schemas_dir(&self.app.slug);
        match schema::list_schemas(&schemas_dir) {
            Ok(schemas) => schemas
                .iter()
                .map(|s| {
                    minijinja::context! {
                        title => s.meta.title,
                        slug => s.meta.slug,
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn template_context(&self) -> minijinja::Value {
        minijinja::context! {
            id => self.app.id,
            slug => self.app.slug,
            name => self.app.name,
        }
    }
}

impl FromRequestParts<AppState> for AppContext {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let HxRequest(is_htmx) = HxRequest::from_request_parts(parts, state)
            .await
            .unwrap_or(HxRequest(false));

        // Get user info from extensions (set by require_auth middleware)
        let user = parts.extensions.get::<allowthem_core::User>().cloned();
        let role = parts.extensions.get::<CurrentUserRole>()
            .map(|r| r.0.clone())
            .unwrap_or_default();
        let current_username = user.as_ref()
            .and_then(|u| u.username.as_ref())
            .map(|u| u.to_string())
            .unwrap_or_default();

        // CSRF from tower-session
        let session = parts.extensions.get::<Session>().cloned();
        let csrf_token = if let Some(ref s) = session {
            ensure_csrf_token(s).await
        } else {
            String::new()
        };

        let err_nav = |status: u16, msg: &str| {
            let html = render_error_with_nav(
                state, status, msg, is_htmx, &role, &current_username, &csrf_token,
            );
            (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Html(html))
                .into_response()
        };

        let params: HashMap<String, String> =
            match Path::<HashMap<String, String>>::from_request_parts(parts, state).await {
                Ok(Path(params)) => params,
                Err(_) => return Err(err_nav(404, "Not found")),
            };

        let slug = params
            .get("app_slug")
            .ok_or_else(|| err_nav(404, "Not found"))?;

        let app = models::find_app_by_slug(&state.pool, slug)
            .await
            .map_err(|_| err_nav(500, "Internal error"))?
            .ok_or_else(|| err_nav(404, "App not found"))?;

        // Auth check: user must be in extensions
        let user = user.ok_or_else(|| err_nav(403, "Not authenticated"))?;

        // Admins have access to all apps; others need explicit access
        if role != "admin" {
            let has_access = models::user_has_app_access(&state.pool, app.id, &user.id.to_string())
                .await
                .map_err(|_| err_nav(500, "Internal error"))?;
            if !has_access {
                return Err(err_nav(403, "You do not have access to this app"));
            }
        }

        Ok(AppContext { app })
    }
}

/// API route extractor — resolves app, no auth check (bearer does that).
pub struct ApiAppContext {
    pub app: App,
}

impl FromRequestParts<AppState> for ApiAppContext {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let params: HashMap<String, String> =
            match Path::<HashMap<String, String>>::from_request_parts(parts, state).await {
                Ok(Path(params)) => params,
                Err(_) => {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({"error": "Not found"})),
                    ));
                }
            };

        let slug = params.get("app_slug").ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Not found"})),
            )
        })?;

        let app = models::find_app_by_slug(&state.pool, slug)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Internal error"})),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "App not found"})),
                )
            })?;

        Ok(ApiAppContext { app })
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add src/app_context.rs
git commit -m "refactor: AppContext reads user from extensions instead of session"
```

---

### Task 10: Update routes that use auth helpers

This task updates all route files that call old auth functions. The pattern is consistent:
- `auth::require_role(&session, "admin")` → `auth::require_role(request.extensions(), "admin")`
- `auth::current_user_id(&session)` → get `User` from extensions
- `auth::current_user_role(&session)` → get `CurrentUserRole` from extensions
- `auth::current_username(&session)` → get from `User.username` in extensions

**Files:**
- Modify: `src/routes/settings.rs`
- Modify: `src/routes/content.rs`
- Modify: `src/routes/schemas.rs`
- Modify: `src/routes/apps.rs`
- Modify: `src/routes/deployments.rs`
- Modify: `src/routes/uploads.rs`
- Modify: `src/routes/api.rs`
- Modify: `src/routes/mod.rs`

- [ ] **Step 1: Update settings.rs**

Key changes in settings.rs:
- `users_page`: use `auth::require_role(request.extensions(), "admin")`, list users from `state.ath.db().list_users()`, list invitations from `state.ath.db().list_pending_invitations()`
- `invite_user`: create invitation via `state.ath.db().create_invitation()` with role in metadata
- `profile_page`: get current user from extensions
- `change_password`: verify old password via `allowthem_core::verify_password()`, update via `state.ath.db().update_user_password()`
- `delete_invitation`: delete via `state.ath.db().delete_invitation()`
- `audit_log_page`: tabbed UI — see Task 12

Each handler needs to accept `axum::extract::Request` (or `Extension<User>`) to access extensions. The simplest approach: add `request: axum::extract::Request` parameter and extract user/role from `request.extensions()`.

However, since Axum handlers destructure request parts, and we already have `Session` extractor, we can use axum's `Extension` extractor:

```rust
use axum::Extension;

async fn users_page(
    Extension(user): Extension<allowthem_core::User>,
    Extension(role): Extension<auth::CurrentUserRole>,
    session: Session,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // role check
    if role.0 != "admin" {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    // ... rest of handler using state.ath.db() for user/invitation queries
}
```

Apply this pattern across all handlers. Each handler that previously used `auth::require_role(&session, ...)` now checks `role.0` directly or calls `auth::require_role(extensions, ...)`.

**Note:** The `Extension` extractor returns 500 if the extension is missing. Since `require_auth` middleware sets these extensions for all authenticated routes, this is safe. For the `not_found` fallback (which bypasses middleware on some paths), we need to handle the case where extensions are absent.

- [ ] **Step 2: Update routes/mod.rs not_found handler**

```rust
async fn not_found(
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    session: Session,
    request: axum::extract::Request,
    State(state): State<AppState>,
) -> Response {
    if uri.path().starts_with("/api/") {
        return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
    }

    // Check if user is authenticated via extensions
    let user = request.extensions().get::<allowthem_core::User>();
    if user.is_none() {
        return Redirect::to("/login").into_response();
    }

    let role = request.extensions().get::<auth::CurrentUserRole>()
        .map(|r| r.0.as_str())
        .unwrap_or("");
    let username = user
        .and_then(|u| u.username.as_ref())
        .map(|u| u.to_string())
        .unwrap_or_default();
    let csrf_token = auth::ensure_csrf_token(&session).await;

    let html = render_error_with_nav(
        &state, 404, "Page not found", is_htmx, role, &username, &csrf_token,
    );
    (axum::http::StatusCode::NOT_FOUND, Html(html)).into_response()
}
```

- [ ] **Step 3: Update routes/mod.rs build_router**

Update imports:
```rust
use crate::auth::{require_auth, verify_csrf};
```

The `build_router` function stays the same — it still applies `verify_csrf` and `require_auth` as middleware layers.

- [ ] **Step 4: Update API route role checking**

In `src/routes/api.rs`, the `require_api_role` and `require_token_app` functions move to `auth/token.rs` (already done in Task 8). Update API handlers to use the new `BearerToken` struct which already contains `user`, `role`, and `app_id`.

- [ ] **Step 5: Update remaining route files**

For each of `content.rs`, `schemas.rs`, `apps.rs`, `deployments.rs`, `uploads.rs`:
- Replace `auth::require_role(&session, "role")` with `auth::require_role(request.extensions(), "role")` or use `Extension<CurrentUserRole>` extractor
- Replace `auth::current_user_id(&session)` with getting the user ID from `Extension<allowthem_core::User>`
- Replace `auth::current_user_role(&session)` with `Extension<CurrentUserRole>`
- Replace `auth::current_username(&session)` with `user.username`

The template context variables `user_role`, `current_username`, `csrf_token` stay the same — just sourced from different places.

- [ ] **Step 6: Compile and fix**

Run: `cargo check`
Fix any remaining compilation errors.

- [ ] **Step 7: Commit**

```bash
git add src/routes/
git commit -m "refactor: update all routes to use allowthem user from extensions"
```

---

### Task 11: Audit log tabbed UI

**Files:**
- Modify: `src/routes/settings.rs` (audit_log_page handler)
- Modify: `templates/settings/audit_log.html`

- [ ] **Step 1: Split audit_log_page into two tab handlers**

```rust
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        // ... existing routes ...
        .route("/audit-log", get(audit_log_page))
        .route("/audit-log/auth", get(audit_log_auth_tab))
        .route("/audit-log/activity", get(audit_log_activity_tab))
}

async fn audit_log_page(
    Extension(role): Extension<auth::CurrentUserRole>,
    Extension(user): Extension<allowthem_core::User>,
    session: Session,
    State(state): State<AppState>,
) -> Response {
    if role.0 != "admin" {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    // Render page with tabs, default to activity tab loaded
    // ... template render ...
}

async fn audit_log_auth_tab(
    Extension(role): Extension<auth::CurrentUserRole>,
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<PaginationParams>,
) -> impl IntoResponse {
    if role.0 != "admin" {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    let page = params.page.unwrap_or(1);
    let limit = 50u32;
    let offset = ((page - 1) * limit as u64) as u32;

    let entries = state.ath.db().get_audit_log(None, limit, offset)
        .await
        .unwrap_or_default();

    // Render auth events partial template
    // ...
}

async fn audit_log_activity_tab(
    Extension(role): Extension<auth::CurrentUserRole>,
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<PaginationParams>,
) -> impl IntoResponse {
    if role.0 != "admin" {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    // Query substrukt's audit.db as before
    // Render activity partial template
    // ...
}
```

- [ ] **Step 2: Update audit_log.html template with tabs**

Add two htmx tabs:
- "Activity" tab loads `/settings/audit-log/activity` into content area
- "Auth Events" tab loads `/settings/audit-log/auth` into content area
- Default active: Activity
- Use `hx-get`, `hx-target`, `hx-swap` for tab switching

- [ ] **Step 3: Commit**

```bash
git add src/routes/settings.rs templates/settings/audit_log.html
git commit -m "feat: tabbed audit log UI (auth events + activity)"
```

---

### Task 12: Database schema migration

**Files:**
- Create: `migrations/NNN_allowthem_migration.sql` (next migration number)
- Create: `src/db/migration.rs`
- Modify: `src/db/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create the data migration module**

Create `src/db/migration.rs`:

```rust
use sqlx::SqlitePool;

/// Migrate existing users from substrukt's users table into allowthem.
/// Must run BEFORE the SQL migration that drops old tables.
/// Returns a map of old_user_id -> new_allowthem_user_id.
pub async fn migrate_users_to_allowthem(
    substrukt_pool: &SqlitePool,
    ath: &allowthem_core::AllowThem,
) -> eyre::Result<std::collections::HashMap<i64, String>> {
    let mut id_map = std::collections::HashMap::new();

    // Check if old users table exists
    let table_exists: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='users'"
    )
    .fetch_optional(substrukt_pool)
    .await?;

    if table_exists.is_none() {
        tracing::info!("No old users table found, skipping migration");
        return Ok(id_map);
    }

    #[derive(sqlx::FromRow)]
    struct OldUser {
        id: i64,
        username: String,
        password_hash: String,
        email: Option<String>,
        role: String,
    }

    let old_users: Vec<OldUser> = sqlx::query_as(
        "SELECT id, username, password_hash, email, role FROM users"
    )
    .fetch_all(substrukt_pool)
    .await?;

    if old_users.is_empty() {
        tracing::info!("No users to migrate");
        return Ok(id_map);
    }

    tracing::info!("Migrating {} users to allowthem", old_users.len());

    for old_user in &old_users {
        let email_str = old_user.email.clone()
            .unwrap_or_else(|| format!("{}@local", old_user.username));
        let email = allowthem_core::Email::new(email_str)
            .map_err(|e| eyre::eyre!("Invalid email for user {}: {e}", old_user.username))?;
        let username = allowthem_core::Username::new(old_user.username.clone());

        // Check if user already exists (idempotent)
        if let Ok(existing) = ath.db().get_user_by_username(&username).await {
            id_map.insert(old_user.id, existing.id.to_string());
            continue;
        }

        let new_user = ath.db().create_user_with_hash(
            email,
            &old_user.password_hash,
            Some(username),
        ).await.map_err(|e| eyre::eyre!("Failed to migrate user {}: {e}", old_user.username))?;

        // Assign role
        let role_name = allowthem_core::RoleName::new(&old_user.role);
        if let Some(role) = ath.db().get_role_by_name(&role_name).await.unwrap_or(None) {
            ath.db().assign_role(&new_user.id, &role.id).await
                .map_err(|e| eyre::eyre!("Failed to assign role: {e}"))?;
        }

        id_map.insert(old_user.id, new_user.id.to_string());
        tracing::info!("Migrated user {} -> {}", old_user.username, new_user.id);
    }

    // Update app_access references
    #[derive(sqlx::FromRow)]
    struct OldAccess {
        app_id: i64,
        user_id: i64,
    }

    let old_access: Vec<OldAccess> = sqlx::query_as(
        "SELECT app_id, user_id FROM app_access"
    )
    .fetch_all(substrukt_pool)
    .await
    .unwrap_or_default();

    // We'll recreate app_access with TEXT user_id in the SQL migration.
    // Store the mapping for later use.
    // For now, write the mappings to a temp table.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _user_id_migration (old_id INTEGER PRIMARY KEY, new_id TEXT NOT NULL)"
    )
    .execute(substrukt_pool)
    .await?;

    for (old_id, new_id) in &id_map {
        sqlx::query("INSERT OR REPLACE INTO _user_id_migration (old_id, new_id) VALUES (?, ?)")
            .bind(old_id)
            .bind(new_id)
            .execute(substrukt_pool)
            .await?;
    }

    tracing::info!("User migration complete: {} users migrated", id_map.len());
    Ok(id_map)
}
```

- [ ] **Step 2: Add mod declaration**

In `src/db/mod.rs`, add:

```rust
pub mod migration;
```

- [ ] **Step 3: Create SQL migration**

Determine next migration number (check `migrations/` directory). Create `migrations/NNN_allowthem_migration.sql`:

```sql
-- Recreate app_access with TEXT user_id (UUID from allowthem)
CREATE TABLE app_access_new (
    app_id INTEGER NOT NULL,
    user_id TEXT NOT NULL,
    PRIMARY KEY (app_id, user_id)
);

-- Copy data using the migration mapping table
INSERT INTO app_access_new (app_id, user_id)
SELECT a.app_id, m.new_id
FROM app_access a
JOIN _user_id_migration m ON a.user_id = m.old_id;

DROP TABLE app_access;
ALTER TABLE app_access_new RENAME TO app_access;

-- Create app_tokens table
CREATE TABLE IF NOT EXISTS app_tokens (
    api_token_id TEXT NOT NULL,
    app_id INTEGER NOT NULL,
    token_hash TEXT NOT NULL,
    PRIMARY KEY (api_token_id, app_id)
);

-- Drop old auth tables
DROP TABLE IF EXISTS api_tokens;
DROP TABLE IF EXISTS invitations;
DROP TABLE IF EXISTS users;

-- Clean up migration helper
DROP TABLE IF EXISTS _user_id_migration;
```

- [ ] **Step 4: Wire migration into startup**

In `src/main.rs`, after allowthem init and role bootstrap, before `db::init_pool` runs migrations:

Actually, the order matters:
1. `db::init_pool` runs substrukt's migrations (including old ones that create `users` table)
2. AllowThem builds (creates allowthem_ tables in the shared DB)
3. Data migration runs (reads old users, creates in allowthem, writes mapping table)
4. Then the NEW migration (NNN_allowthem_migration.sql) runs... but migrations already ran in step 1.

This is a problem. sqlx runs ALL migrations in `init_pool`. We can't run data migration between SQL migrations.

**Fix:** Run `init_pool` WITHOUT the new migration first, then run data migration, then run the new migration manually. Or: split into two phases.

Simpler approach: make the data migration idempotent and run it as a Rust function during startup. The SQL migration checks for the temp table:

1. `init_pool` runs all migrations including the new one
2. But the new migration uses `_user_id_migration` table which doesn't exist yet
3. This would fail!

**Better approach:** Don't use a SQL migration for this. Do everything in Rust:

```rust
// In main.rs startup:
// 1. init_pool (old migrations run, old tables exist)
let pool = db::init_pool(&config.db_path).await?;

// 2. Build allowthem
let ath = AllowThemBuilder::with_pool(pool.clone()).build().await?;

// 3. Run data migration (Rust)
let id_map = db::migration::migrate_users_to_allowthem(&pool, &ath).await?;

// 4. Run schema changes in Rust
db::migration::finalize_schema(&pool, &id_map).await?;
```

Add `finalize_schema` to migration.rs:

```rust
pub async fn finalize_schema(
    pool: &SqlitePool,
    id_map: &std::collections::HashMap<i64, String>,
) -> eyre::Result<()> {
    // Check if migration already done
    let old_users_exist: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='users'"
    )
    .fetch_optional(pool)
    .await?;

    if old_users_exist.is_none() {
        return Ok(()); // Already migrated
    }

    tracing::info!("Finalizing schema migration...");

    // Recreate app_access with TEXT user_id
    sqlx::query("CREATE TABLE app_access_new (app_id INTEGER NOT NULL, user_id TEXT NOT NULL, PRIMARY KEY (app_id, user_id))")
        .execute(pool).await?;

    for (old_id, new_id) in id_map {
        sqlx::query("INSERT OR IGNORE INTO app_access_new (app_id, user_id) SELECT app_id, ? FROM app_access WHERE user_id = ?")
            .bind(new_id)
            .bind(old_id)
            .execute(pool).await?;
    }

    sqlx::query("DROP TABLE app_access").execute(pool).await?;
    sqlx::query("ALTER TABLE app_access_new RENAME TO app_access").execute(pool).await?;

    // Create app_tokens
    sqlx::query("CREATE TABLE IF NOT EXISTS app_tokens (api_token_id TEXT NOT NULL, app_id INTEGER NOT NULL, token_hash TEXT NOT NULL, PRIMARY KEY (api_token_id, app_id))")
        .execute(pool).await?;

    // Drop old tables
    sqlx::query("DROP TABLE IF EXISTS api_tokens").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS invitations").execute(pool).await?;
    sqlx::query("DROP TABLE IF EXISTS users").execute(pool).await?;

    tracing::info!("Schema migration complete");
    Ok(())
}
```

- [ ] **Step 5: Commit**

```bash
git add src/db/migration.rs src/db/mod.rs src/main.rs
git commit -m "feat: data migration from old auth tables to allowthem"
```

---

### Task 13: Update settings routes (users, profile, invitations)

**Files:**
- Modify: `src/routes/settings.rs`

- [ ] **Step 1: Rewrite users_page**

```rust
async fn users_page(
    Extension(user): Extension<allowthem_core::User>,
    Extension(role): Extension<auth::CurrentUserRole>,
    session: Session,
    State(state): State<AppState>,
    HxRequest(is_htmx): HxRequest,
) -> Response {
    if role.0 != "admin" {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    let csrf_token = auth::ensure_csrf_token(&session).await;
    let users = state.ath.db().list_users().await.unwrap_or_default();
    let invitations = state.ath.db().list_pending_invitations().await.unwrap_or_default();

    // Build user list with roles
    let mut user_list = Vec::new();
    for u in &users {
        let roles = state.ath.db().get_user_roles(&u.id).await.unwrap_or_default();
        let role_str = roles.first().map(|r| r.name.as_str().to_string()).unwrap_or_default();
        user_list.push(minijinja::context! {
            id => u.id.to_string(),
            username => u.username.as_ref().map(|n| n.to_string()).unwrap_or_default(),
            email => u.email.as_str(),
            role => role_str,
            created_at => u.created_at.to_rfc3339(),
        });
    }

    let invite_list: Vec<_> = invitations.iter().map(|inv| {
        minijinja::context! {
            id => inv.id.to_string(),
            email => inv.email.as_ref().map(|e| e.as_str().to_string()).unwrap_or_default(),
            role => inv.metadata.clone().unwrap_or_default(),
            expires_at => inv.expires_at.to_rfc3339(),
        }
    }).collect();

    // render template...
    // (template rendering follows existing pattern)
}
```

- [ ] **Step 2: Rewrite invite_user**

```rust
async fn invite_user(
    Extension(role): Extension<auth::CurrentUserRole>,
    Extension(current_user): Extension<allowthem_core::User>,
    session: Session,
    State(state): State<AppState>,
    Form(form): Form<InviteForm>,
) -> Response {
    if role.0 != "admin" {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    // Validate email
    let email = match allowthem_core::Email::new(form.email.clone()) {
        Ok(e) => e,
        Err(_) => { /* render error */ }
    };

    // Check if user already exists with this email
    if state.ath.db().get_user_by_email(&email).await.is_ok() {
        // render error: user already exists
    }

    // Validate role
    if !["admin", "editor", "viewer"].contains(&form.role.as_str()) {
        // render error: invalid role
    }

    let expires = chrono::Utc::now() + chrono::Duration::days(7);
    let (raw_token, _invitation) = state.ath.db().create_invitation(
        Some(&email),
        Some(&form.role),  // role stored in metadata
        Some(current_user.id),
        expires,
    ).await.map_err(|e| /* render error */)?;

    // Build signup URL
    let signup_url = format!("/signup?token={}", raw_token);

    // Render page with invite URL shown
    // ...
}
```

- [ ] **Step 3: Rewrite profile and change_password**

```rust
async fn profile_page(
    Extension(user): Extension<allowthem_core::User>,
    Extension(role): Extension<auth::CurrentUserRole>,
    session: Session,
    State(state): State<AppState>,
    HxRequest(is_htmx): HxRequest,
) -> Response {
    let flash = auth::take_flash(&session).await;
    let csrf_token = auth::ensure_csrf_token(&session).await;
    let username = user.username.as_ref().map(|u| u.to_string()).unwrap_or_default();
    // render settings/profile.html with username, flash, csrf_token
}

async fn change_password(
    Extension(user): Extension<allowthem_core::User>,
    session: Session,
    State(state): State<AppState>,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    // Verify current password
    let hash = user.password_hash.as_ref().ok_or(/* error */)?;
    match allowthem_core::verify_password(&form.current_password, hash) {
        Ok(true) => {}
        _ => {
            auth::set_flash(&session, "error", "Current password is incorrect.").await;
            return Redirect::to("/settings/profile").into_response();
        }
    }

    // Validate new password
    if form.new_password.len() < 8 { /* flash error */ }
    if form.new_password != form.confirm_password { /* flash error */ }

    // Update via allowthem
    state.ath.db().update_user_password(user.id, &form.new_password).await
        .map_err(|_| /* error */)?;

    auth::set_flash(&session, "success", "Password updated successfully.").await;
    Redirect::to("/settings/profile").into_response()
}
```

- [ ] **Step 4: Rewrite delete_invitation**

```rust
async fn delete_invitation(
    Extension(role): Extension<auth::CurrentUserRole>,
    State(state): State<AppState>,
    session: Session,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    if role.0 != "admin" {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    let inv_id = allowthem_core::InvitationId::from_uuid(
        uuid::Uuid::parse_str(&id).unwrap_or_default()
    );
    let _ = state.ath.db().delete_invitation(inv_id).await;
    auth::set_flash(&session, "info", "Invitation deleted.").await;
    Redirect::to("/settings/users").into_response()
}
```

- [ ] **Step 5: Compile and fix**

Run: `cargo check`

- [ ] **Step 6: Commit**

```bash
git add src/routes/settings.rs
git commit -m "feat: rewrite settings routes to use allowthem"
```

---

### Task 14: Update integration tests

**Files:**
- Modify: `tests/integration.rs`

This is the largest single task. The TestServer setup needs allowthem, and many tests need updated assertions.

- [ ] **Step 1: Update TestServer::start()**

```rust
async fn start() -> Self {
    let data_dir = tempfile::tempdir().unwrap();
    let db_path = data_dir.path().join("test.db");
    let mut config = Config::new(
        Some(data_dir.path().to_path_buf()),
        Some(db_path),
        Some(0),
        false,
        10,
        10,
    );
    config.allow_private_webhooks = true;
    config.ensure_dirs().unwrap();
    config.ensure_app_dirs("default").unwrap();

    let pool = db::init_pool(&config.db_path).await.unwrap();
    let session_store = SqliteStore::new(pool.clone());
    session_store.migrate().await.unwrap();
    let session_layer = SessionManagerLayer::new(session_store).with_secure(false);

    let audit_db_path = data_dir.path().join("audit.db");
    let audit_pool = substrukt::audit::init_pool(&audit_db_path).await.unwrap();
    let audit_logger = substrukt::audit::AuditLogger::new(audit_pool);

    // allowthem setup (shares pool)
    let ath = allowthem_core::AllowThemBuilder::with_pool(pool.clone())
        .cookie_secure(false)
        .build()
        .await
        .unwrap();

    // Bootstrap roles
    for role_name in ["admin", "editor", "viewer"] {
        let rn = allowthem_core::RoleName::new(role_name);
        ath.db().create_role(&rn, None).await.unwrap();
    }

    let auth_client: Arc<dyn allowthem_core::AuthClient> =
        Arc::new(allowthem_core::EmbeddedAuthClient::new(ath.clone(), "/login"));

    let reloader = templates::create_reloader();
    let content_cache = DashMap::new();
    cache::populate(&content_cache, &config.data_dir);

    let metrics_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();

    let state = Arc::new(AppStateInner {
        pool,
        config,
        templates: reloader,
        cache: content_cache,
        etag_cache: DashMap::new(),
        login_limiter: RateLimiter::new(100, std::time::Duration::from_secs(60)),
        api_limiter: RateLimiter::new(1000, std::time::Duration::from_secs(60)),
        metrics_handle,
        audit: audit_logger,
        http_client: reqwest::Client::new(),
        deploy_tasks: DashMap::new(),
        s3_config: None,
        backup_trigger: None,
        backup_running: std::sync::atomic::AtomicBool::new(false),
        backup_cancel: None,
        openapi_cache: std::sync::Arc::new(std::sync::RwLock::new(None)),
        ath,
        auth_client,
        has_users: std::sync::atomic::AtomicBool::new(false),
    });

    let app = routes::build_router(state).layer(session_layer);
    // ... rest unchanged ...
}
```

- [ ] **Step 2: Add allowthem imports to test file**

```rust
use allowthem_core;
use std::sync::Arc;
```

- [ ] **Step 3: Update test assertions for changed behavior**

Key changes:
- Setup creates users in allowthem (same HTTP flow, but backend is different)
- The setup form now needs an email or uses `username@local` pattern
- Cookie name changes from tower-sessions to `allowthem_session`
- User IDs are now UUIDs (affects any test that checks IDs)
- `signup_user_with_role` should still work since it goes through the HTTP layer

Most tests should pass without changes since they test via HTTP. The main failures will be:
- Tests that construct `AppStateInner` directly (TestServer::start)
- Tests in `db/models.rs` that test User CRUD (removed — those are allowthem's responsibility now)
- Tests that check specific session values

- [ ] **Step 4: Run tests and iterate**

Run: `cargo test`
Fix failures one at a time.

- [ ] **Step 5: Commit**

```bash
git add tests/integration.rs
git commit -m "test: update integration tests for allowthem auth"
```

---

### Task 15: Cleanup and final verification

**Files:**
- Modify: `Cargo.toml` (verify no unused deps)
- Modify: `NOTES.md` (update with new architecture)

- [ ] **Step 1: Remove unused dependencies**

Check if these are still needed (may not be after removing old auth code):
- `argon2` — remove (confirmed in Task 2)
- `subtle` — keep (still used for CSRF)
- `hex` — keep (still used for CSRF tokens)

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy`
Fix any warnings.

- [ ] **Step 4: Update NOTES.md**

Add to NOTES.md:

```
- **allowthem integration**: Auth handled by allowthem-core (path dep at ../allowthem/crates/core). Shared SQLite pool (both on sqlx 0.9.0-alpha.1). allowthem's `allowthem_`-prefixed tables live in substrukt's database. `AllowThemBuilder::with_pool(pool)` shares the pool. Cross-references (app_access.user_id, app_tokens.api_token_id) are TEXT UUIDs. tower-sessions kept for flash messages and CSRF only.
- **Role checking**: Roles are in allowthem (admin, editor, viewer). Checked via `auth_client.check_role()`. Role hierarchy (admin > editor > viewer) enforced in `auth::require_role()` and `auth::token::require_api_role()`.
- **Data migration**: One-time Rust migration runs at startup. Reads old `users` table, creates users in allowthem with existing Argon2 hashes via `create_user_with_hash()`. Remaps `app_access.user_id` from INTEGER to TEXT UUID. Drops old auth tables. Idempotent.
```

- [ ] **Step 5: Commit**

```bash
git add NOTES.md Cargo.toml
git commit -m "chore: cleanup deps, update notes for allowthem integration"
```

---

## Dependency graph

```
Task 1  (allowthem-core change)
  ↓
Task 2  (deps + state) → Task 3 (startup) → Task 12 (data migration)
  ↓                                              ↓
Task 4  (auth middleware)                    Task 7 (models cleanup)
  ↓                                              ↓
Task 5  (login/logout/setup)              Task 8 (bearer token)
  ↓                                              ↓
Task 6  (signup/invitations)              Task 9 (AppContext)
  ↓                                              ↓
Task 10 (update all routes) ←─────────────────────┘
  ↓
Task 11 (audit log tabs)
  ↓
Task 13 (settings routes)
  ↓
Task 14 (integration tests)
  ↓
Task 15 (cleanup)
```

Tasks 2-9 can be partially parallelized (left branch and right branch), but Task 10 depends on both branches completing.
