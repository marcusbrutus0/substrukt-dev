use std::net::IpAddr;
use std::time::Duration;

use eyre::Result;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::audit::{AuditLogger, Deployment};
use crate::state::AppState;

/// Validate that a webhook URL is safe to call (prevents SSRF attacks).
///
/// When `allow_private` is true, private/reserved hosts and IPs are permitted
/// (useful for testing with local mock servers).
pub fn validate_webhook_url(raw_url: &str, allow_private: bool) -> std::result::Result<(), String> {
    let parsed = reqwest::Url::parse(raw_url).map_err(|e| format!("Invalid URL: {e}"))?;

    // Must be http or https
    match parsed.scheme() {
        "http" | "https" => {}
        s => return Err(format!("Unsupported scheme: {s}")),
    }

    let host = parsed.host_str().ok_or("Missing host")?;

    if !allow_private {
        // Block obviously dangerous hosts
        let blocked = [
            "localhost",
            "127.0.0.1",
            "0.0.0.0",
            "[::1]",
            "metadata.google.internal",
        ];
        if blocked.iter().any(|b| host.eq_ignore_ascii_case(b)) {
            return Err("Private/reserved host not allowed".to_string());
        }

        // Block private IP ranges
        if let Ok(ip) = host.parse::<IpAddr>() {
            if !ip_is_global(&ip) {
                return Err("Private/reserved IP not allowed".to_string());
            }
        }

        // Block 169.254.x.x (link-local / cloud metadata)
        if host.starts_with("169.254.") {
            return Err("Link-local address not allowed".to_string());
        }
    }

    Ok(())
}

fn ip_is_global(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_unspecified()
        }
        IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unspecified(),
    }
}

#[derive(Serialize)]
struct WebhookPayload {
    event_type: &'static str,
    app: String,
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
    app_slug: &str,
) -> Result<bool> {
    let triggered_by = match source {
        TriggerSource::Auto => "auto",
        TriggerSource::Manual => "manual",
        TriggerSource::Retry => "retry",
    };

    let payload = WebhookPayload {
        event_type: "substrukt-publish",
        app: app_slug.to_string(),
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
    let pool = state.pool.clone();
    let poll_interval = Duration::from_secs(30);
    let debounce = Duration::from_secs(deployment.debounce_seconds as u64);

    tokio::spawn(async move {
        // Resolve app_slug from app_id at startup (avoids repeated lookups)
        let app_slug = if let Some(app_id) = deployment.app_id {
            match crate::db::models::find_app_by_id(&pool, app_id).await {
                Ok(Some(app)) => app.slug,
                _ => "unknown".to_string(),
            }
        } else {
            "default".to_string()
        };

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
                    match fire_webhook(&client, &audit, &deployment, TriggerSource::Auto, &app_slug)
                        .await
                    {
                        Ok(_) => {
                            audit.log_with_app(
                                "system",
                                "deployment_auto_fired",
                                "deployment",
                                &deployment.slug,
                                None,
                                deployment.app_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_https_url() {
        assert!(validate_webhook_url("https://example.com/webhook", false).is_ok());
    }

    #[test]
    fn accepts_valid_http_url() {
        assert!(validate_webhook_url("http://example.com/webhook", false).is_ok());
    }

    #[test]
    fn rejects_ftp_scheme() {
        let err = validate_webhook_url("ftp://example.com/file", false).unwrap_err();
        assert!(err.contains("Unsupported scheme"), "got: {err}");
    }

    #[test]
    fn rejects_javascript_scheme() {
        let err = validate_webhook_url("javascript:alert(1)", false).unwrap_err();
        assert!(err.contains("Unsupported scheme"), "got: {err}");
    }

    #[test]
    fn rejects_localhost() {
        let err = validate_webhook_url("http://localhost/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved host"), "got: {err}");
    }

    #[test]
    fn rejects_localhost_case_insensitive() {
        let err = validate_webhook_url("http://LOCALHOST/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved host"), "got: {err}");
    }

    #[test]
    fn rejects_127_0_0_1() {
        let err = validate_webhook_url("http://127.0.0.1/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved"), "got: {err}");
    }

    #[test]
    fn rejects_0_0_0_0() {
        let err = validate_webhook_url("http://0.0.0.0/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved"), "got: {err}");
    }

    #[test]
    fn rejects_ipv6_loopback() {
        let err = validate_webhook_url("http://[::1]/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved"), "got: {err}");
    }

    #[test]
    fn rejects_cloud_metadata_endpoint() {
        let err = validate_webhook_url("http://metadata.google.internal/computeMetadata", false)
            .unwrap_err();
        assert!(err.contains("Private/reserved host"), "got: {err}");
    }

    #[test]
    fn rejects_private_ip_10_x() {
        let err = validate_webhook_url("http://10.0.0.1/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved IP"), "got: {err}");
    }

    #[test]
    fn rejects_private_ip_172_16_x() {
        let err = validate_webhook_url("http://172.16.0.1/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved IP"), "got: {err}");
    }

    #[test]
    fn rejects_private_ip_192_168_x() {
        let err = validate_webhook_url("http://192.168.1.1/webhook", false).unwrap_err();
        assert!(err.contains("Private/reserved IP"), "got: {err}");
    }

    #[test]
    fn rejects_link_local_169_254() {
        let err =
            validate_webhook_url("http://169.254.169.254/latest/meta-data", false).unwrap_err();
        assert!(
            err.contains("Link-local") || err.contains("Private/reserved"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_invalid_url() {
        let err = validate_webhook_url("not a url", false).unwrap_err();
        assert!(err.contains("Invalid URL"), "got: {err}");
    }

    #[test]
    fn rejects_empty_string() {
        let err = validate_webhook_url("", false).unwrap_err();
        assert!(err.contains("Invalid URL"), "got: {err}");
    }

    #[test]
    fn accepts_url_with_port() {
        assert!(validate_webhook_url("https://example.com:8080/webhook", false).is_ok());
    }

    #[test]
    fn accepts_url_with_path_and_query() {
        assert!(validate_webhook_url("https://example.com/api/deploy?token=abc", false).is_ok());
    }

    #[test]
    fn allow_private_permits_localhost() {
        assert!(validate_webhook_url("http://localhost/webhook", true).is_ok());
    }

    #[test]
    fn allow_private_permits_private_ip() {
        assert!(validate_webhook_url("http://127.0.0.1:3000/webhook", true).is_ok());
    }

    #[test]
    fn allow_private_still_rejects_bad_scheme() {
        let err = validate_webhook_url("ftp://localhost/file", true).unwrap_err();
        assert!(err.contains("Unsupported scheme"), "got: {err}");
    }
}
