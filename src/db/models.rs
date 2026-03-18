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

pub async fn create_user(pool: &SqlitePool, username: &str, password: &str, role: &str) -> eyre::Result<User> {
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

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct ApiToken {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub token_hash: String,
    pub created_at: String,
}

pub async fn create_api_token(
    pool: &SqlitePool,
    user_id: i64,
    name: &str,
    token_hash: &str,
) -> eyre::Result<ApiToken> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO api_tokens (user_id, name, token_hash, created_at) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(name)
    .bind(token_hash)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    Ok(ApiToken {
        id,
        user_id,
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

pub async fn delete_api_token(pool: &SqlitePool, token_id: i64, user_id: i64) -> eyre::Result<()> {
    sqlx::query("DELETE FROM api_tokens WHERE id = ? AND user_id = ?")
        .bind(token_id)
        .bind(user_id)
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
