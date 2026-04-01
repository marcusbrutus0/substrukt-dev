use eyre::Result;
use serde::Serialize;

use crate::audit::AuditLogger;
use crate::config::Config;

#[derive(Serialize)]
struct WebhookPayload {
    event_type: &'static str,
    environment: String,
    triggered_at: String,
    triggered_by: &'static str,
}

pub enum TriggerSource {
    Cron,
    Manual,
    Retry,
}

struct AttemptResult {
    success: bool,
    http_status: Option<u16>,
    error_message: Option<String>,
    response_time_ms: i64,
}

async fn attempt_webhook(
    client: &reqwest::Client,
    url: &str,
    auth_token: Option<&str>,
    payload: &WebhookPayload,
) -> AttemptResult {
    let start = std::time::Instant::now();
    let mut req = client.post(url).json(payload);
    if let Some(token) = auth_token {
        req = req.bearer_auth(token);
    }
    match req.send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_millis() as i64;
            if resp.status().is_success() {
                AttemptResult {
                    success: true,
                    http_status: Some(resp.status().as_u16()),
                    error_message: None,
                    response_time_ms: elapsed,
                }
            } else {
                AttemptResult {
                    success: false,
                    http_status: Some(resp.status().as_u16()),
                    error_message: Some(format!("HTTP {}", resp.status())),
                    response_time_ms: elapsed,
                }
            }
        }
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as i64;
            AttemptResult {
                success: false,
                http_status: None,
                error_message: Some(e.to_string()),
                response_time_ms: elapsed,
            }
        }
    }
}

/// Fire a webhook for the given environment. Returns Ok(true) if the first attempt succeeded
/// and timestamp updated, Ok(false) if webhook URL not configured, Err if the first attempt failed
/// (background retries will continue automatically).
pub async fn fire_webhook(
    client: &reqwest::Client,
    audit: &AuditLogger,
    config: &Config,
    environment: &str,
    source: TriggerSource,
) -> Result<bool> {
    let (url, auth_token) = match environment {
        "staging" => (
            config.staging_webhook_url.as_deref(),
            config.staging_webhook_auth_token.as_deref(),
        ),
        "production" => (
            config.production_webhook_url.as_deref(),
            config.production_webhook_auth_token.as_deref(),
        ),
        _ => (None, None),
    };

    let url = match url {
        Some(u) => u,
        None => return Ok(false),
    };

    let triggered_by = match source {
        TriggerSource::Cron => "cron",
        TriggerSource::Manual => "manual",
        TriggerSource::Retry => "retry",
    };

    let payload = WebhookPayload {
        event_type: "substrukt-publish",
        environment: environment.to_string(),
        triggered_at: chrono::Utc::now().to_rfc3339(),
        triggered_by,
    };

    let group_id = uuid::Uuid::new_v4().to_string();

    // First attempt (synchronous)
    let result = attempt_webhook(client, url, auth_token, &payload).await;
    let status = if result.success { "success" } else { "failed" };

    if let Err(e) = audit
        .record_webhook_attempt(
            environment,
            triggered_by,
            status,
            result.http_status,
            result.error_message.as_deref(),
            Some(result.response_time_ms),
            1,
            &group_id,
        )
        .await
    {
        tracing::warn!("Failed to record webhook attempt: {e}");
    }

    if result.success {
        let _ = audit.mark_fired(environment).await;
        return Ok(true);
    }

    // Spawn background retries
    let client = client.clone();
    let audit = audit.clone();
    let url = url.to_string();
    let auth_token = auth_token.map(|s| s.to_string());
    let environment = environment.to_string();
    let group_id_clone = group_id.clone();

    tokio::spawn(async move {
        let delays = [
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(30),
        ];

        for (i, delay) in delays.iter().enumerate() {
            tokio::time::sleep(*delay).await;
            let attempt_num = (i + 2) as i32;

            let retry_result =
                attempt_webhook(&client, &url, auth_token.as_deref(), &payload).await;
            let retry_status = if retry_result.success {
                "success"
            } else {
                "failed"
            };

            if let Err(e) = audit
                .record_webhook_attempt(
                    &environment,
                    "retry",
                    retry_status,
                    retry_result.http_status,
                    retry_result.error_message.as_deref(),
                    Some(retry_result.response_time_ms),
                    attempt_num,
                    &group_id_clone,
                )
                .await
            {
                tracing::warn!("Failed to record webhook retry attempt: {e}");
            }

            if retry_result.success {
                let _ = audit.mark_fired(&environment).await;
                return;
            }
        }

        tracing::warn!(
            "Webhook for {} exhausted all retries (group {})",
            environment,
            group_id_clone
        );
    });

    eyre::bail!(
        "Webhook failed: {}",
        result.error_message.unwrap_or_default()
    )
}

/// Spawn the background cron task that auto-fires the staging webhook when dirty.
pub fn spawn_cron(client: reqwest::Client, audit: AuditLogger, config: Config) {
    let interval = std::time::Duration::from_secs(config.webhook_check_interval);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;

            if config.staging_webhook_url.is_none() {
                continue;
            }

            match audit.is_dirty("staging").await {
                Ok(true) => {
                    tracing::info!("Staging is dirty, firing webhook");
                    if let Err(e) =
                        fire_webhook(&client, &audit, &config, "staging", TriggerSource::Cron).await
                    {
                        tracing::warn!("Staging webhook failed: {e}");
                    }
                }
                Ok(false) => {
                    tracing::debug!("Staging is clean, skipping webhook");
                }
                Err(e) => {
                    tracing::warn!("Failed to check staging dirty state: {e}");
                }
            }
        }
    });
}
