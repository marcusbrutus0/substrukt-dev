use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub created_at: String,
    pub role: String,
}

impl User {
    pub fn hash_password(password: &str) -> eyre::Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| eyre::eyre!("Failed to hash password: {e}"))?;
        Ok(hash.to_string())
    }

    pub fn verify_password(&self, password: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(&self.password_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }
}

pub async fn create_user(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: &str,
) -> eyre::Result<User> {
    let password_hash = User::hash_password(password)?;
    let now = chrono::Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (username, password_hash, created_at, role) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(username)
    .bind(&password_hash)
    .bind(&now)
    .bind(role)
    .fetch_one(pool)
    .await?;

    Ok(User {
        id,
        username: username.to_string(),
        password_hash,
        created_at: now,
        role: role.to_string(),
    })
}

pub async fn find_user_by_username(
    pool: &SqlitePool,
    username: &str,
) -> eyre::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn user_count(pool: &SqlitePool) -> eyre::Result<i64> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

// --- Apps ---

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct App {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

pub async fn create_app(pool: &SqlitePool, slug: &str, name: &str) -> eyre::Result<App> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO apps (slug, name, created_at) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(slug)
    .bind(name)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    Ok(App {
        id,
        slug: slug.to_string(),
        name: name.to_string(),
        created_at: now,
    })
}

pub async fn find_app_by_slug(pool: &SqlitePool, slug: &str) -> eyre::Result<Option<App>> {
    let app = sqlx::query_as::<_, App>("SELECT * FROM apps WHERE slug = ?")
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    Ok(app)
}

pub async fn find_app_by_id(pool: &SqlitePool, id: i64) -> eyre::Result<Option<App>> {
    let app = sqlx::query_as::<_, App>("SELECT * FROM apps WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(app)
}

pub async fn list_apps(pool: &SqlitePool) -> eyre::Result<Vec<App>> {
    let apps = sqlx::query_as::<_, App>("SELECT * FROM apps ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(apps)
}

pub async fn update_app_name(pool: &SqlitePool, id: i64, name: &str) -> eyre::Result<()> {
    sqlx::query("UPDATE apps SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_app(pool: &SqlitePool, id: i64) -> eyre::Result<()> {
    sqlx::query("DELETE FROM apps WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

const RESERVED_APP_SLUGS: &[&str] = &[
    "api", "settings", "login", "logout", "signup", "setup", "healthz", "metrics", "new",
];

pub fn validate_app_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.len() > 64 {
        return Err("Slug must be 1-64 characters".to_string());
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err("Slug must not start or end with a hyphen".to_string());
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(
            "Slug must be lowercase alphanumeric with hyphens, no leading/trailing hyphens"
                .to_string(),
        );
    }
    if RESERVED_APP_SLUGS.contains(&slug) {
        return Err(format!("'{slug}' is a reserved slug"));
    }
    Ok(())
}

// --- App Access ---

pub async fn grant_app_access(
    pool: &SqlitePool,
    app_id: i64,
    user_id: i64,
) -> eyre::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO app_access (app_id, user_id) VALUES (?, ?)")
        .bind(app_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn revoke_app_access(
    pool: &SqlitePool,
    app_id: i64,
    user_id: i64,
) -> eyre::Result<()> {
    sqlx::query("DELETE FROM app_access WHERE app_id = ? AND user_id = ?")
        .bind(app_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn user_has_app_access(
    pool: &SqlitePool,
    app_id: i64,
    user_id: i64,
) -> eyre::Result<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM app_access WHERE app_id = ? AND user_id = ?",
    )
    .bind(app_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn list_apps_for_user(pool: &SqlitePool, user_id: i64) -> eyre::Result<Vec<App>> {
    let apps = sqlx::query_as::<_, App>(
        "SELECT a.* FROM apps a INNER JOIN app_access aa ON a.id = aa.app_id WHERE aa.user_id = ? ORDER BY a.name",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(apps)
}

pub async fn list_app_users(
    pool: &SqlitePool,
    app_id: i64,
) -> eyre::Result<Vec<(User, bool)>> {
    // Get all non-admin users with a flag indicating if they have access to this app
    let users: Vec<User> = sqlx::query_as::<_, User>(
        "SELECT id, username, password_hash, created_at, role FROM users WHERE role != 'admin' ORDER BY username",
    )
    .fetch_all(pool)
    .await?;

    let mut result = Vec::new();
    for user in users {
        let has_access = user_has_app_access(pool, app_id, user.id).await?;
        result.push((user, has_access));
    }
    Ok(result)
}

// --- API Tokens ---

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct ApiToken {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub token_hash: String,
    pub created_at: String,
    pub app_id: Option<i64>,
}

pub async fn create_api_token(
    pool: &SqlitePool,
    user_id: i64,
    app_id: i64,
    name: &str,
    token_hash: &str,
) -> eyre::Result<ApiToken> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO api_tokens (user_id, app_id, name, token_hash, created_at) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(app_id)
    .bind(name)
    .bind(token_hash)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    Ok(ApiToken {
        id,
        user_id,
        app_id: Some(app_id),
        name: name.to_string(),
        token_hash: token_hash.to_string(),
        created_at: now,
    })
}

pub async fn list_api_tokens(pool: &SqlitePool, user_id: i64) -> eyre::Result<Vec<ApiToken>> {
    let tokens = sqlx::query_as::<_, ApiToken>(
        "SELECT * FROM api_tokens WHERE user_id = ? ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(tokens)
}

pub async fn list_api_tokens_for_app(
    pool: &SqlitePool,
    app_id: i64,
) -> eyre::Result<Vec<ApiToken>> {
    let tokens = sqlx::query_as::<_, ApiToken>(
        "SELECT * FROM api_tokens WHERE app_id = ? ORDER BY created_at DESC",
    )
    .bind(app_id)
    .fetch_all(pool)
    .await?;
    Ok(tokens)
}

pub async fn delete_api_token(
    pool: &SqlitePool,
    token_id: i64,
    app_id: i64,
) -> eyre::Result<()> {
    sqlx::query("DELETE FROM api_tokens WHERE id = ? AND app_id = ?")
        .bind(token_id)
        .bind(app_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_token_by_hash(
    pool: &SqlitePool,
    token_hash: &str,
) -> eyre::Result<Option<ApiToken>> {
    let token = sqlx::query_as::<_, ApiToken>("SELECT * FROM api_tokens WHERE token_hash = ?")
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;
    Ok(token)
}

// --- Invitations ---

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct Invitation {
    pub id: i64,
    pub email: String,
    pub token_hash: String,
    pub invited_by: i64,
    pub created_at: String,
    pub expires_at: String,
    pub role: String,
}

pub async fn create_invitation(
    pool: &SqlitePool,
    email: &str,
    token_hash: &str,
    invited_by: i64,
    expires_at: &str,
    role: &str,
) -> eyre::Result<Invitation> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO invitations (email, token_hash, invited_by, created_at, expires_at, role) VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(email)
    .bind(token_hash)
    .bind(invited_by)
    .bind(&now)
    .bind(expires_at)
    .bind(role)
    .fetch_one(pool)
    .await?;

    Ok(Invitation {
        id,
        email: email.to_string(),
        token_hash: token_hash.to_string(),
        invited_by,
        created_at: now,
        expires_at: expires_at.to_string(),
        role: role.to_string(),
    })
}

pub async fn find_invitation_by_token_hash(
    pool: &SqlitePool,
    token_hash: &str,
) -> eyre::Result<Option<Invitation>> {
    let inv = sqlx::query_as::<_, Invitation>(
        "SELECT * FROM invitations WHERE token_hash = ? AND expires_at > datetime('now')",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    Ok(inv)
}

pub async fn find_invitation_by_email(
    pool: &SqlitePool,
    email: &str,
) -> eyre::Result<Option<Invitation>> {
    let inv = sqlx::query_as::<_, Invitation>("SELECT * FROM invitations WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(inv)
}

pub async fn list_pending_invitations(pool: &SqlitePool) -> eyre::Result<Vec<Invitation>> {
    let invitations = sqlx::query_as::<_, Invitation>(
        "SELECT * FROM invitations WHERE expires_at > datetime('now') ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(invitations)
}

pub async fn delete_invitation(pool: &SqlitePool, id: i64) -> eyre::Result<()> {
    sqlx::query("DELETE FROM invitations WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn create_user_with_email(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    email: &str,
    role: &str,
) -> eyre::Result<User> {
    let password_hash = User::hash_password(password)?;
    let now = chrono::Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (username, password_hash, email, created_at, role) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(username)
    .bind(&password_hash)
    .bind(email)
    .bind(&now)
    .bind(role)
    .fetch_one(pool)
    .await?;

    Ok(User {
        id,
        username: username.to_string(),
        password_hash,
        created_at: now,
        role: role.to_string(),
    })
}

pub async fn find_user_by_email(pool: &SqlitePool, email: &str) -> eyre::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn find_user_role(pool: &SqlitePool, user_id: i64) -> eyre::Result<Option<String>> {
    let role = sqlx::query_scalar::<_, String>("SELECT role FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(role)
}
