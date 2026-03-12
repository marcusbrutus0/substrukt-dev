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
}

/// Fire a webhook for the given environment. Returns Ok(true) if fired and timestamp updated,
/// Ok(false) if webhook URL not configured, Err if the HTTP call failed.
pub async fn fire_webhook(
    client: &reqwest::Client,
    audit: &AuditLogger,
    config: &Config,
    environment: &str,
    source: TriggerSource,
) -> Result<bool> {
    let url = match environment {
        "staging" => config.staging_webhook_url.as_deref(),
        "production" => config.production_webhook_url.as_deref(),
        _ => None,
    };

    let url = match url {
        Some(u) => u,
        None => return Ok(false),
    };

    let triggered_by = match source {
        TriggerSource::Cron => "cron",
        TriggerSource::Manual => "manual",
    };

    let payload = WebhookPayload {
        event_type: "substrukt-publish",
        environment: environment.to_string(),
        triggered_at: chrono::Utc::now().to_rfc3339(),
        triggered_by,
    };

    let resp = client.post(url).json(&payload).send().await?;

    if resp.status().is_success() {
        audit.mark_fired(environment).await?;
        audit.log(
            "system",
            "webhook_fire",
            "webhook",
            environment,
            Some(
                &serde_json::json!({"status": "success", "triggered_by": triggered_by}).to_string(),
            ),
        );
        Ok(true)
    } else {
        let status = resp.status();
        audit.log(
            "system",
            "webhook_fire",
            "webhook",
            environment,
            Some(
                &serde_json::json!({"status": "failed", "http_status": status.as_u16()})
                    .to_string(),
            ),
        );
        eyre::bail!("Webhook returned HTTP {status}")
    }
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
