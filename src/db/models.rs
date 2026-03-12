use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub created_at: String,
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

pub async fn create_user(pool: &SqlitePool, username: &str, password: &str) -> eyre::Result<User> {
    let password_hash = User::hash_password(password)?;
    let now = chrono::Utc::now().to_rfc3339();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(username)
    .bind(&password_hash)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    Ok(User {
        id,
        username: username.to_string(),
        password_hash,
        created_at: now,
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
