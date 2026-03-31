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

pub async fn grant_app_access(pool: &SqlitePool, app_id: i64, user_id: i64) -> eyre::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO app_access (app_id, user_id) VALUES (?, ?)")
        .bind(app_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn revoke_app_access(pool: &SqlitePool, app_id: i64, user_id: i64) -> eyre::Result<()> {
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

pub async fn list_app_users(pool: &SqlitePool, app_id: i64) -> eyre::Result<Vec<(User, bool)>> {
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

pub async fn delete_api_token(pool: &SqlitePool, token_id: i64, app_id: i64) -> eyre::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;

    async fn test_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .pragma("foreign_keys", "ON");
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    // ── App CRUD tests ──────────────────────────────────────────

    #[tokio::test]
    async fn test_app_crud() {
        let pool = test_pool().await;

        // Default app from migration already exists
        let default = find_app_by_slug(&pool, "default").await.unwrap().unwrap();
        assert_eq!(default.slug, "default");
        assert_eq!(default.name, "Default");
        assert_eq!(default.id, 1);

        // Create a new app
        let blog = create_app(&pool, "blog", "Blog").await.unwrap();
        assert_eq!(blog.slug, "blog");
        assert_eq!(blog.name, "Blog");
        assert!(blog.id > 1);

        // find_by_slug
        let found = find_app_by_slug(&pool, "blog").await.unwrap().unwrap();
        assert_eq!(found.id, blog.id);

        // find_by_id
        let found = find_app_by_id(&pool, blog.id).await.unwrap().unwrap();
        assert_eq!(found.slug, "blog");

        // find_by_slug returns None for nonexistent
        assert!(find_app_by_slug(&pool, "nope").await.unwrap().is_none());

        // find_by_id returns None for nonexistent
        assert!(find_app_by_id(&pool, 9999).await.unwrap().is_none());

        // list_apps includes both
        let apps = list_apps(&pool).await.unwrap();
        assert_eq!(apps.len(), 2);
        let slugs: Vec<&str> = apps.iter().map(|a| a.slug.as_str()).collect();
        assert!(slugs.contains(&"default"));
        assert!(slugs.contains(&"blog"));

        // update_app_name
        update_app_name(&pool, blog.id, "My Blog").await.unwrap();
        let updated = find_app_by_id(&pool, blog.id).await.unwrap().unwrap();
        assert_eq!(updated.name, "My Blog");

        // delete_app
        delete_app(&pool, blog.id).await.unwrap();
        assert!(find_app_by_slug(&pool, "blog").await.unwrap().is_none());
        let apps = list_apps(&pool).await.unwrap();
        assert_eq!(apps.len(), 1);
    }

    #[tokio::test]
    async fn test_app_duplicate_slug_fails() {
        let pool = test_pool().await;
        create_app(&pool, "blog", "Blog").await.unwrap();
        let result = create_app(&pool, "blog", "Another Blog").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UNIQUE"));
    }

    // ── Slug validation tests ───────────────────────────────────

    #[test]
    fn test_app_slug_validation_valid() {
        assert!(validate_app_slug("blog").is_ok());
        assert!(validate_app_slug("my-app").is_ok());
        assert!(validate_app_slug("app123").is_ok());
        assert!(validate_app_slug("a").is_ok());
        assert!(validate_app_slug("a-b-c").is_ok());
        assert!(validate_app_slug("x".repeat(64).as_str()).is_ok());
    }

    #[test]
    fn test_app_slug_validation_invalid() {
        // Empty
        assert!(validate_app_slug("").is_err());

        // Too long (65 chars)
        assert!(validate_app_slug(&"x".repeat(65)).is_err());

        // Leading/trailing hyphens
        assert!(validate_app_slug("-blog").is_err());
        assert!(validate_app_slug("blog-").is_err());
        assert!(validate_app_slug("-").is_err());

        // Uppercase
        assert!(validate_app_slug("Blog").is_err());
        assert!(validate_app_slug("BLOG").is_err());

        // Spaces
        assert!(validate_app_slug("my blog").is_err());

        // Underscores
        assert!(validate_app_slug("my_blog").is_err());

        // Special chars
        assert!(validate_app_slug("blog!").is_err());
        assert!(validate_app_slug("blog/path").is_err());
    }

    #[test]
    fn test_app_slug_validation_reserved() {
        let reserved = vec![
            "api", "settings", "login", "logout", "signup", "setup", "healthz", "metrics", "new",
        ];
        for slug in reserved {
            let result = validate_app_slug(slug);
            assert!(result.is_err(), "'{slug}' should be reserved");
            assert!(
                result.unwrap_err().contains("reserved"),
                "Error for '{slug}' should mention 'reserved'"
            );
        }
    }

    // ── App access tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_app_access() {
        let pool = test_pool().await;
        let app = create_app(&pool, "blog", "Blog").await.unwrap();
        let user = create_user(&pool, "editor1", "pass", "editor")
            .await
            .unwrap();

        // Initially no access
        assert!(!user_has_app_access(&pool, app.id, user.id).await.unwrap());
        let user_apps = list_apps_for_user(&pool, user.id).await.unwrap();
        assert!(user_apps.is_empty());

        // Grant access
        grant_app_access(&pool, app.id, user.id).await.unwrap();
        assert!(user_has_app_access(&pool, app.id, user.id).await.unwrap());
        let user_apps = list_apps_for_user(&pool, user.id).await.unwrap();
        assert_eq!(user_apps.len(), 1);
        assert_eq!(user_apps[0].slug, "blog");

        // Grant again (idempotent via INSERT OR IGNORE)
        grant_app_access(&pool, app.id, user.id).await.unwrap();
        assert!(user_has_app_access(&pool, app.id, user.id).await.unwrap());

        // Revoke access
        revoke_app_access(&pool, app.id, user.id).await.unwrap();
        assert!(!user_has_app_access(&pool, app.id, user.id).await.unwrap());
        let user_apps = list_apps_for_user(&pool, user.id).await.unwrap();
        assert!(user_apps.is_empty());

        // Revoke again (no error)
        revoke_app_access(&pool, app.id, user.id).await.unwrap();
    }

    #[tokio::test]
    async fn test_app_access_multiple_apps() {
        let pool = test_pool().await;
        let app1 = create_app(&pool, "blog", "Blog").await.unwrap();
        let app2 = create_app(&pool, "docs", "Docs").await.unwrap();
        let user = create_user(&pool, "editor1", "pass", "editor")
            .await
            .unwrap();

        grant_app_access(&pool, app1.id, user.id).await.unwrap();
        grant_app_access(&pool, app2.id, user.id).await.unwrap();

        let user_apps = list_apps_for_user(&pool, user.id).await.unwrap();
        assert_eq!(user_apps.len(), 2);
        let slugs: Vec<&str> = user_apps.iter().map(|a| a.slug.as_str()).collect();
        assert!(slugs.contains(&"blog"));
        assert!(slugs.contains(&"docs"));

        // Revoking one doesn't affect the other
        revoke_app_access(&pool, app1.id, user.id).await.unwrap();
        let user_apps = list_apps_for_user(&pool, user.id).await.unwrap();
        assert_eq!(user_apps.len(), 1);
        assert_eq!(user_apps[0].slug, "docs");
    }

    #[tokio::test]
    async fn test_list_app_users() {
        let pool = test_pool().await;
        let app = create_app(&pool, "blog", "Blog").await.unwrap();
        let editor = create_user(&pool, "editor1", "pass", "editor")
            .await
            .unwrap();
        let _viewer = create_user(&pool, "viewer1", "pass", "viewer")
            .await
            .unwrap();
        // Admin users are not listed by list_app_users
        let _admin = create_user(&pool, "admin2", "pass", "admin").await.unwrap();

        grant_app_access(&pool, app.id, editor.id).await.unwrap();

        let users = list_app_users(&pool, app.id).await.unwrap();
        assert_eq!(users.len(), 2); // editor and viewer (non-admins)
        let editor_entry = users.iter().find(|(u, _)| u.username == "editor1").unwrap();
        assert!(editor_entry.1, "editor1 should have access");
        let viewer_entry = users.iter().find(|(u, _)| u.username == "viewer1").unwrap();
        assert!(!viewer_entry.1, "viewer1 should not have access");
    }

    // ── API Token app scoping tests ─────────────────────────────

    #[tokio::test]
    async fn test_api_token_app_scoping() {
        let pool = test_pool().await;
        let app1 = create_app(&pool, "blog", "Blog").await.unwrap();
        let app2 = create_app(&pool, "docs", "Docs").await.unwrap();
        let user = create_user(&pool, "admin1", "pass", "admin").await.unwrap();

        // Create tokens for different apps
        let tok1 = create_api_token(&pool, user.id, app1.id, "Blog Token", "hash1")
            .await
            .unwrap();
        assert_eq!(tok1.app_id, Some(app1.id));

        let tok2 = create_api_token(&pool, user.id, app2.id, "Docs Token", "hash2")
            .await
            .unwrap();
        assert_eq!(tok2.app_id, Some(app2.id));

        // find_token_by_hash returns correct app_id
        let found = find_token_by_hash(&pool, "hash1").await.unwrap().unwrap();
        assert_eq!(found.app_id, Some(app1.id));
        assert_eq!(found.name, "Blog Token");

        let found = find_token_by_hash(&pool, "hash2").await.unwrap().unwrap();
        assert_eq!(found.app_id, Some(app2.id));

        // list_api_tokens_for_app returns only that app's tokens
        let app1_tokens = list_api_tokens_for_app(&pool, app1.id).await.unwrap();
        assert_eq!(app1_tokens.len(), 1);
        assert_eq!(app1_tokens[0].name, "Blog Token");

        let app2_tokens = list_api_tokens_for_app(&pool, app2.id).await.unwrap();
        assert_eq!(app2_tokens.len(), 1);
        assert_eq!(app2_tokens[0].name, "Docs Token");

        // list_api_tokens (by user) returns all
        let all_tokens = list_api_tokens(&pool, user.id).await.unwrap();
        assert_eq!(all_tokens.len(), 2);

        // delete_api_token scoped by app
        delete_api_token(&pool, tok1.id, app1.id).await.unwrap();
        assert!(find_token_by_hash(&pool, "hash1").await.unwrap().is_none());
        // tok2 should be unaffected
        assert!(find_token_by_hash(&pool, "hash2").await.unwrap().is_some());

        // delete_api_token with wrong app_id doesn't delete
        delete_api_token(&pool, tok2.id, app1.id).await.unwrap(); // no error, but no effect
        assert!(
            find_token_by_hash(&pool, "hash2").await.unwrap().is_some(),
            "Token should not be deleted when app_id doesn't match"
        );
    }

    // ── Cascade delete tests ────────────────────────────────────

    #[tokio::test]
    async fn test_delete_app_cascade() {
        let pool = test_pool().await;
        let app = create_app(&pool, "blog", "Blog").await.unwrap();
        let user = create_user(&pool, "editor1", "pass", "editor")
            .await
            .unwrap();

        // Create associated records
        grant_app_access(&pool, app.id, user.id).await.unwrap();
        create_api_token(&pool, user.id, app.id, "Test Token", "tokenhash")
            .await
            .unwrap();

        // Insert upload and upload_reference directly
        sqlx::query(
            "INSERT INTO uploads (app_id, hash, filename, mime, size) VALUES (?, 'abc123', 'test.png', 'image/png', 100)",
        )
        .bind(app.id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO upload_references (app_id, upload_hash, schema_slug, entry_id) VALUES (?, 'abc123', 'posts', 'entry1')",
        )
        .bind(app.id)
        .execute(&pool)
        .await
        .unwrap();

        // Verify everything exists
        assert!(user_has_app_access(&pool, app.id, user.id).await.unwrap());
        assert!(
            find_token_by_hash(&pool, "tokenhash")
                .await
                .unwrap()
                .is_some()
        );
        let upload_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM uploads WHERE app_id = ?")
                .bind(app.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(upload_count, 1);
        let ref_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM upload_references WHERE app_id = ?")
                .bind(app.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ref_count, 1);

        // Delete the app
        delete_app(&pool, app.id).await.unwrap();

        // All associated records should be cascade-deleted
        assert!(!user_has_app_access(&pool, app.id, user.id).await.unwrap());
        assert!(
            find_token_by_hash(&pool, "tokenhash")
                .await
                .unwrap()
                .is_none()
        );
        let upload_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM uploads WHERE app_id = ?")
                .bind(app.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(upload_count, 0);
        let ref_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM upload_references WHERE app_id = ?")
                .bind(app.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ref_count, 0);

        // User still exists (not cascade-deleted)
        assert!(
            find_user_by_username(&pool, "editor1")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_delete_user_cascades_app_access() {
        let pool = test_pool().await;
        let app = create_app(&pool, "blog", "Blog").await.unwrap();
        let user = create_user(&pool, "editor1", "pass", "editor")
            .await
            .unwrap();

        grant_app_access(&pool, app.id, user.id).await.unwrap();
        assert!(user_has_app_access(&pool, app.id, user.id).await.unwrap());

        // Delete user
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();

        // app_access should be cascade-deleted
        assert!(!user_has_app_access(&pool, app.id, user.id).await.unwrap());

        // App still exists
        assert!(find_app_by_slug(&pool, "blog").await.unwrap().is_some());
    }
}
