use std::time::Duration;

use eyre::Result;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::audit::{AuditLogger, Deployment};
use crate::state::AppState;

#[derive(Serialize)]
struct WebhookPayload {
    event_type: &'static str,
    deployment: String,
    include_drafts: bool,
    triggered_at: String,
    triggered_by: &'static str,
}

pub enum TriggerSource {
    Auto,
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

/// Fire a webhook for the given deployment. Returns Ok(true) if the first attempt succeeded
/// and timestamp updated, Err if the first attempt failed (background retries will continue
/// automatically).
pub async fn fire_webhook(
    client: &reqwest::Client,
    audit: &AuditLogger,
    deployment: &Deployment,
    source: TriggerSource,
) -> Result<bool> {
    let triggered_by = match source {
        TriggerSource::Auto => "auto",
        TriggerSource::Manual => "manual",
        TriggerSource::Retry => "retry",
    };

    let payload = WebhookPayload {
        event_type: "substrukt-publish",
        deployment: deployment.slug.clone(),
        include_drafts: deployment.include_drafts,
        triggered_at: chrono::Utc::now().to_rfc3339(),
        triggered_by,
    };

    let group_id = uuid::Uuid::new_v4().to_string();

    // First attempt (synchronous)
    let result = attempt_webhook(
        client,
        &deployment.webhook_url,
        deployment.webhook_auth_token.as_deref(),
        &payload,
    )
    .await;
    let status = if result.success { "success" } else { "failed" };

    if let Err(e) = audit
        .record_webhook_attempt(
            deployment.id,
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
        let _ = audit.mark_deployment_fired(deployment.id).await;
        return Ok(true);
    }

    // Spawn background retries
    let client = client.clone();
    let audit = audit.clone();
    let deployment = deployment.clone();
    let group_id_clone = group_id.clone();

    tokio::spawn(async move {
        let delays = [Duration::from_secs(5), Duration::from_secs(30)];

        for (i, delay) in delays.iter().enumerate() {
            tokio::time::sleep(*delay).await;
            let attempt_num = (i + 2) as i32;

            let retry_result = attempt_webhook(
                &client,
                &deployment.webhook_url,
                deployment.webhook_auth_token.as_deref(),
                &payload,
            )
            .await;
            let retry_status = if retry_result.success {
                "success"
            } else {
                "failed"
            };

            if let Err(e) = audit
                .record_webhook_attempt(
                    deployment.id,
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
                let _ = audit.mark_deployment_fired(deployment.id).await;
                return;
            }
        }

        tracing::warn!(
            "Webhook for {} exhausted all retries (group {})",
            deployment.slug,
            group_id_clone
        );
    });

    eyre::bail!(
        "Webhook failed: {}",
        result.error_message.unwrap_or_default()
    )
}

/// Spawn a background auto-deploy task for a deployment with auto_deploy enabled.
pub fn spawn_auto_deploy_task(state: &AppState, deployment: Deployment) {
    let cancel_token = CancellationToken::new();
    let child_token = cancel_token.child_token();
    state.deploy_tasks.insert(deployment.id, cancel_token);

    let client = state.http_client.clone();
    let audit = state.audit.clone();
    let poll_interval = Duration::from_secs(30);
    let debounce = Duration::from_secs(deployment.debounce_seconds as u64);

    tokio::spawn(async move {
        loop {
            // Check dirty
            let dirty = match audit.is_dirty_for_deployment(deployment.id).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Dirty check failed for deployment {}: {e}", deployment.slug);
                    false
                }
            };

            if dirty {
                // Debounce: wait, then re-check
                tokio::select! {
                    _ = tokio::time::sleep(debounce) => {},
                    _ = child_token.cancelled() => return,
                }

                // Re-check after debounce
                let still_dirty = audit
                    .is_dirty_for_deployment(deployment.id)
                    .await
                    .unwrap_or(false);
                if still_dirty {
                    tracing::info!("Auto-deploying {}", deployment.slug);
                    match fire_webhook(&client, &audit, &deployment, TriggerSource::Auto).await {
                        Ok(_) => {
                            audit.log(
                                "system",
                                "deployment_auto_fired",
                                "deployment",
                                &deployment.slug,
                                None,
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Auto-deploy webhook failed for {}: {e}",
                                deployment.slug
                            );
                        }
                    }
                }
            }

            // Sleep until next poll
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {},
                _ = child_token.cancelled() => return,
            }
        }
    });
}

/// Cancel the auto-deploy task for a deployment (if running).
pub fn cancel_auto_deploy_task(state: &AppState, deployment_id: i64) {
    if let Some((_, token)) = state.deploy_tasks.remove(&deployment_id) {
        token.cancel();
    }
}
