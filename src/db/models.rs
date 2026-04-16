use sqlx::SqlitePool;

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

pub async fn user_has_app_access(
    pool: &SqlitePool,
    app_id: i64,
    user_id: &str,
) -> sqlx::Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM app_access WHERE app_id = ? AND user_id = ?")
            .bind(app_id)
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

pub async fn list_apps_for_user(pool: &SqlitePool, user_id: &str) -> sqlx::Result<Vec<App>> {
    let apps = sqlx::query_as::<_, App>(
        "SELECT a.* FROM apps a INNER JOIN app_access aa ON a.id = aa.app_id WHERE aa.user_id = ? ORDER BY a.name",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(apps)
}

// --- App Tokens ---

pub async fn create_app_token(
    pool: &SqlitePool,
    api_token_id: &str,
    app_id: i64,
    token_hash: &str,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO app_tokens (api_token_id, app_id, token_hash) VALUES (?, ?, ?)")
        .bind(api_token_id)
        .bind(app_id)
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_app_for_token_hash(
    pool: &SqlitePool,
    token_hash: &str,
) -> sqlx::Result<Option<(String, i64)>> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT api_token_id, app_id FROM app_tokens WHERE token_hash = ?")
            .bind(token_hash)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

pub async fn list_app_tokens(pool: &SqlitePool, app_id: i64) -> sqlx::Result<Vec<String>> {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT api_token_id FROM app_tokens WHERE app_id = ?")
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
        // The migration creates app_access with INTEGER user_id.
        // Recreate it with TEXT user_id for new tests.
        sqlx::query("DROP TABLE IF EXISTS app_access")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE app_access (app_id INTEGER NOT NULL, user_id TEXT NOT NULL, PRIMARY KEY (app_id, user_id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Create app_tokens table too
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS app_tokens (api_token_id TEXT NOT NULL, app_id INTEGER NOT NULL, token_hash TEXT NOT NULL, PRIMARY KEY (api_token_id, app_id))",
        )
        .execute(&pool)
        .await
        .unwrap();
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
        let user_id = "test-user-uuid-1";

        assert!(!user_has_app_access(&pool, app.id, user_id).await.unwrap());

        grant_app_access(&pool, app.id, user_id).await.unwrap();
        assert!(user_has_app_access(&pool, app.id, user_id).await.unwrap());

        // Grant again (idempotent)
        grant_app_access(&pool, app.id, user_id).await.unwrap();

        revoke_app_access(&pool, app.id, user_id).await.unwrap();
        assert!(!user_has_app_access(&pool, app.id, user_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_app_access_multiple_apps() {
        let pool = test_pool().await;
        let app1 = create_app(&pool, "blog", "Blog").await.unwrap();
        let app2 = create_app(&pool, "docs", "Docs").await.unwrap();
        let user_id = "test-user-uuid-2";

        grant_app_access(&pool, app1.id, user_id).await.unwrap();
        grant_app_access(&pool, app2.id, user_id).await.unwrap();

        let user_apps = list_apps_for_user(&pool, user_id).await.unwrap();
        assert_eq!(user_apps.len(), 2);

        revoke_app_access(&pool, app1.id, user_id).await.unwrap();
        let user_apps = list_apps_for_user(&pool, user_id).await.unwrap();
        assert_eq!(user_apps.len(), 1);
        assert_eq!(user_apps[0].slug, "docs");
    }
}
