use std::path::Path;
use std::sync::Arc;

use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use std::str::FromStr;

pub async fn init_pool(db_path: &Path) -> eyre::Result<SqlitePool> {
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let options = SqliteConnectOptions::from_str(&url)?
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await?;
    sqlx::migrate!("./audit_migrations").run(&pool).await?;
    Ok(pool)
}

#[derive(Clone)]
pub struct AuditLogger {
    pool: Arc<SqlitePool>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebhookHistoryGroup {
    pub id: i64,
    pub environment: String,
    pub trigger_source: String,
    pub status: String,
    pub http_status: Option<i32>,
    pub error_message: Option<String>,
    pub response_time_ms: Option<i64>,
    pub attempt_count: i32,
    pub group_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub timestamp: String,
    pub actor: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub details: Option<String>,
}

impl AuditLogger {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    #[cfg(test)]
    pub async fn execute_raw(&self, query: &str) -> eyre::Result<()> {
        sqlx::query(query).execute(self.pool.as_ref()).await?;
        Ok(())
    }

    pub async fn is_dirty(&self, environment: &str) -> eyre::Result<bool> {
        let last_fired: Option<(Option<String>,)> =
            sqlx::query_as("SELECT last_fired_at FROM webhook_state WHERE environment = ?")
                .bind(environment)
                .fetch_optional(self.pool.as_ref())
                .await?;

        let last_fired_at = match last_fired {
            Some((Some(ts),)) => ts,
            _ => return Ok(true),
        };

        let latest_mutation: (Option<String>,) = sqlx::query_as(
            "SELECT MAX(timestamp) FROM audit_log WHERE action IN ('content_create', 'content_update', 'content_delete', 'schema_create', 'schema_update', 'schema_delete')",
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        match latest_mutation {
            (Some(ts),) => Ok(ts > last_fired_at),
            _ => Ok(false),
        }
    }

    pub async fn mark_fired(&self, environment: &str) -> eyre::Result<String> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE webhook_state SET last_fired_at = ? WHERE environment = ?")
            .bind(&now)
            .bind(environment)
            .execute(self.pool.as_ref())
            .await?;
        Ok(now)
    }

    pub async fn record_webhook_attempt(
        &self,
        environment: &str,
        trigger_source: &str,
        status: &str,
        http_status: Option<u16>,
        error_message: Option<&str>,
        response_time_ms: Option<i64>,
        attempt: i32,
        group_id: &str,
    ) -> eyre::Result<i64> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "INSERT INTO webhook_history (environment, trigger_source, status, http_status, error_message, response_time_ms, attempt, group_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(environment)
        .bind(trigger_source)
        .bind(status)
        .bind(http_status.map(|s| s as i32))
        .bind(error_message)
        .bind(response_time_ms)
        .bind(attempt)
        .bind(group_id)
        .bind(&now)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn list_webhook_history(
        &self,
        environment_filter: Option<&str>,
        status_filter: Option<&str>,
    ) -> eyre::Result<Vec<WebhookHistoryGroup>> {
        let base = "SELECT h.id, h.environment, h.trigger_source, h.status, h.http_status, h.error_message, h.response_time_ms, g.attempt_count, h.group_id, h.created_at
            FROM webhook_history h
            INNER JOIN (
                SELECT group_id, MAX(id) AS max_id, COUNT(*) AS attempt_count
                FROM webhook_history
                GROUP BY group_id
            ) g ON h.id = g.max_id";

        let mut conditions = Vec::new();
        if environment_filter.is_some() {
            conditions.push("h.environment = ?");
        }
        if status_filter.is_some() {
            conditions.push("h.status = ?");
        }

        let query = if conditions.is_empty() {
            format!("{base} ORDER BY h.created_at DESC LIMIT 100")
        } else {
            format!(
                "{base} WHERE {} ORDER BY h.created_at DESC LIMIT 100",
                conditions.join(" AND ")
            )
        };

        let mut q = sqlx::query_as::<
            _,
            (
                i64,
                String,
                String,
                String,
                Option<i32>,
                Option<String>,
                Option<i64>,
                i32,
                String,
                String,
            ),
        >(&query);

        if let Some(env) = environment_filter {
            q = q.bind(env);
        }
        if let Some(status) = status_filter {
            q = q.bind(status);
        }

        let rows = q.fetch_all(self.pool.as_ref()).await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    environment,
                    trigger_source,
                    status,
                    http_status,
                    error_message,
                    response_time_ms,
                    attempt_count,
                    group_id,
                    created_at,
                )| {
                    WebhookHistoryGroup {
                        id,
                        environment,
                        trigger_source,
                        status,
                        http_status,
                        error_message,
                        response_time_ms,
                        attempt_count,
                        group_id,
                        created_at,
                    }
                },
            )
            .collect())
    }

    pub async fn list_audit_log(
        &self,
        action_filter: Option<&str>,
        actor_filter: Option<&str>,
        page: u32,
    ) -> eyre::Result<(Vec<AuditLogEntry>, bool)> {
        let page = page.max(1);
        let offset = (page - 1) as i64 * 100;
        let base = "SELECT id, timestamp, actor, action, resource_type, resource_id, details FROM audit_log";

        let mut conditions = Vec::new();
        if action_filter.is_some() {
            conditions.push("action = ?");
        }
        if actor_filter.is_some() {
            conditions.push("actor = ?");
        }

        let query = if conditions.is_empty() {
            format!("{base} ORDER BY timestamp DESC, id DESC LIMIT 101 OFFSET ?")
        } else {
            format!(
                "{base} WHERE {} ORDER BY timestamp DESC, id DESC LIMIT 101 OFFSET ?",
                conditions.join(" AND ")
            )
        };

        let mut q = sqlx::query_as::<_, (i64, String, String, String, String, String, Option<String>)>(&query);

        if let Some(action) = action_filter {
            q = q.bind(action);
        }
        if let Some(actor) = actor_filter {
            q = q.bind(actor);
        }
        q = q.bind(offset);

        let rows = q.fetch_all(self.pool.as_ref()).await?;
        let has_next = rows.len() > 100;
        let entries: Vec<AuditLogEntry> = rows
            .into_iter()
            .take(100)
            .map(|(id, timestamp, actor, action, resource_type, resource_id, details)| {
                AuditLogEntry { id, timestamp, actor, action, resource_type, resource_id, details }
            })
            .collect();

        Ok((entries, has_next))
    }

    pub async fn list_audit_actors(&self) -> eyre::Result<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT DISTINCT actor FROM audit_log ORDER BY actor")
                .fetch_all(self.pool.as_ref())
                .await?;
        Ok(rows.into_iter().map(|(actor,)| actor).collect())
    }

    pub fn log(
        &self,
        actor: &str,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        details: Option<&str>,
    ) {
        let pool = self.pool.clone();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let actor = actor.to_string();
        let action = action.to_string();
        let resource_type = resource_type.to_string();
        let resource_id = resource_id.to_string();
        let details = details.map(|s| s.to_string());

        tokio::spawn(async move {
            let result = sqlx::query(
                "INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id, details) VALUES (?, ?, ?, ?, ?, ?)"
            )
            .bind(&timestamp)
            .bind(&actor)
            .bind(&action)
            .bind(&resource_type)
            .bind(&resource_id)
            .bind(&details)
            .execute(pool.as_ref())
            .await;

            if let Err(e) = result {
                tracing::warn!("Failed to write audit log: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./audit_migrations")
            .run(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_is_dirty_when_no_mutations() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        assert!(logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_is_dirty_after_mutation() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        logger.mark_fired("staging").await.unwrap();
        // Insert a mutation with a timestamp in the future (RFC3339 format to match mark_fired)
        let future_ts = (chrono::Utc::now() + chrono::Duration::seconds(10)).to_rfc3339();
        let query = format!(
            "INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('{future_ts}', 'test', 'content_create', 'content', 'test/1')"
        );
        logger.execute_raw(&query).await.unwrap();
        assert!(logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_not_dirty_after_mark_fired() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES (datetime('now'), 'test', 'content_create', 'content', 'test/1')")
            .await
            .unwrap();
        logger.mark_fired("staging").await.unwrap();
        assert!(!logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_dirty_ignores_non_mutation_events() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        logger.mark_fired("staging").await.unwrap();
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES (datetime('now', '+1 second'), 'test', 'login', 'session', '')")
            .await
            .unwrap();
        assert!(!logger.is_dirty("staging").await.unwrap());
    }

    #[tokio::test]
    async fn test_record_webhook_attempt() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        let id = logger
            .record_webhook_attempt(
                "staging",
                "manual",
                "success",
                Some(200),
                None,
                Some(150),
                1,
                "test-group-1",
            )
            .await
            .unwrap();
        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_list_webhook_history_grouped() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);

        // Two attempts in same group
        logger
            .record_webhook_attempt(
                "staging",
                "manual",
                "failed",
                Some(500),
                Some("Server error"),
                Some(200),
                1,
                "group-a",
            )
            .await
            .unwrap();
        logger
            .record_webhook_attempt(
                "staging",
                "retry",
                "success",
                Some(200),
                None,
                Some(100),
                2,
                "group-a",
            )
            .await
            .unwrap();

        // One attempt in different group
        logger
            .record_webhook_attempt(
                "production",
                "manual",
                "success",
                Some(200),
                None,
                Some(50),
                1,
                "group-b",
            )
            .await
            .unwrap();

        let all = logger.list_webhook_history(None, None).await.unwrap();
        assert_eq!(all.len(), 2); // two groups

        // Most recent first (group-b then group-a)
        assert_eq!(all[0].group_id, "group-b");
        assert_eq!(all[0].attempt_count, 1);
        assert_eq!(all[1].group_id, "group-a");
        assert_eq!(all[1].attempt_count, 2);
        assert_eq!(all[1].status, "success"); // latest attempt

        // Filter by environment
        let staging = logger
            .list_webhook_history(Some("staging"), None)
            .await
            .unwrap();
        assert_eq!(staging.len(), 1);
        assert_eq!(staging[0].environment, "staging");

        // Filter by status
        let successful = logger
            .list_webhook_history(None, Some("success"))
            .await
            .unwrap();
        assert_eq!(successful.len(), 2); // both groups ended in success
    }

    #[tokio::test]
    async fn test_staging_and_production_independent() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES (datetime('now'), 'test', 'content_create', 'content', 'test/1')")
            .await
            .unwrap();
        logger.mark_fired("staging").await.unwrap();
        assert!(!logger.is_dirty("staging").await.unwrap());
        assert!(logger.is_dirty("production").await.unwrap());
    }

    #[tokio::test]
    async fn test_list_audit_log_order_and_basic() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);

        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id, details) VALUES ('2026-01-01T00:00:00Z', 'user1', 'content_create', 'content', 'posts/1', '{\"title\":\"Hello\"}')")
            .await
            .unwrap();
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id, details) VALUES ('2026-01-02T00:00:00Z', 'user2', 'login', 'session', '', NULL)")
            .await
            .unwrap();

        let (entries, has_next) = logger.list_audit_log(None, None, 1).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert!(!has_next);
        assert_eq!(entries[0].action, "login");
        assert_eq!(entries[0].actor, "user2");
        assert_eq!(entries[1].action, "content_create");
        assert_eq!(entries[1].actor, "user1");
        assert_eq!(entries[1].details, Some("{\"title\":\"Hello\"}".to_string()));
        assert_eq!(entries[0].details, None);
    }

    #[tokio::test]
    async fn test_list_audit_log_action_filter() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);

        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-01T00:00:00Z', 'user1', 'content_create', 'content', 'posts/1')")
            .await
            .unwrap();
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-02T00:00:00Z', 'user1', 'login', 'session', '')")
            .await
            .unwrap();

        let (entries, _) = logger.list_audit_log(Some("login"), None, 1).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "login");
    }

    #[tokio::test]
    async fn test_list_audit_log_actor_filter() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);

        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-01T00:00:00Z', 'user1', 'content_create', 'content', 'posts/1')")
            .await
            .unwrap();
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-02T00:00:00Z', 'user2', 'login', 'session', '')")
            .await
            .unwrap();

        let (entries, _) = logger.list_audit_log(None, Some("user1"), 1).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].actor, "user1");
    }

    #[tokio::test]
    async fn test_list_audit_log_pagination() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);

        for i in 0..105 {
            let ts = format!("2026-01-01T{:02}:{:02}:00Z", i / 60, i % 60);
            let query = format!(
                "INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('{ts}', 'user1', 'login', 'session', '')"
            );
            logger.execute_raw(&query).await.unwrap();
        }

        let (page1, has_next1) = logger.list_audit_log(None, None, 1).await.unwrap();
        assert_eq!(page1.len(), 100);
        assert!(has_next1);

        let (page2, has_next2) = logger.list_audit_log(None, None, 2).await.unwrap();
        assert_eq!(page2.len(), 5);
        assert!(!has_next2);
    }

    #[tokio::test]
    async fn test_list_audit_actors() {
        let pool = test_pool().await;
        let logger = AuditLogger::new(pool);

        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-01T00:00:00Z', 'zara', 'login', 'session', '')")
            .await
            .unwrap();
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-02T00:00:00Z', 'alice', 'login', 'session', '')")
            .await
            .unwrap();
        logger
            .execute_raw("INSERT INTO audit_log (timestamp, actor, action, resource_type, resource_id) VALUES ('2026-01-03T00:00:00Z', 'alice', 'logout', 'session', '')")
            .await
            .unwrap();

        let actors = logger.list_audit_actors().await.unwrap();
        assert_eq!(actors, vec!["alice", "zara"]);
    }
}
