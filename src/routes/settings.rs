use std::sync::atomic::Ordering;

use axum::{
    Form, Router,
    body::Body,
    extract::{Multipart, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use axum_htmx::HxRequest;
use tower_sessions::Session;

use crate::auth;
use crate::auth::token;
use crate::backup;
use crate::db::models;
use crate::state::AppState;
use crate::templates::base_for_htmx;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/tokens", get(tokens_page).post(create_token))
        .route(
            "/tokens/{token_id}/delete",
            axum::routing::post(delete_token),
        )
        .route("/data", get(data_page))
        .route("/data/import", axum::routing::post(import_data))
        .route("/data/export", axum::routing::post(export_data))
        .route("/users", get(users_page))
        .route("/users/invite", axum::routing::post(invite_user))
        .route(
            "/users/invitations/{id}/delete",
            axum::routing::post(delete_invitation),
        )
        .route("/audit-log", get(audit_log_page))
        .route("/backups", get(backups_page).post(update_backup_config))
        .route("/backups/trigger", axum::routing::post(trigger_backup))
}

async fn tokens_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<Html<String>> {
    let user_id = auth::current_user_id(&session)
        .await
        .ok_or("Not authenticated")?;

    let tokens = models::list_api_tokens(&state.pool, user_id)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let csrf_token = auth::ensure_csrf_token(&session).await;

    let token_data: Vec<minijinja::Value> = tokens
        .iter()
        .map(|t| {
            minijinja::context! {
                id => t.id,
                name => t.name,
                created_at => t.created_at,
            }
        })
        .collect();

    let user_role = auth::current_user_role(&session).await.unwrap_or_default();
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/tokens.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            user_role => user_role,
            tokens => token_data,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}

#[derive(serde::Deserialize)]
pub struct CreateTokenForm {
    name: String,
}

async fn create_token(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<CreateTokenForm>,
) -> axum::response::Result<Html<String>> {
    let user_id = auth::require_role(&session, "editor").await?;

    let raw_token = token::generate_token();
    let token_hash = token::hash_token(&raw_token);

    // Temporary: use app_id=1 (default) until tokens move to per-app settings
    let api_token = models::create_api_token(&state.pool, user_id, 1, &form.name, &token_hash)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    state.audit.log(
        &user_id.to_string(),
        "token_create",
        "api_token",
        &api_token.id.to_string(),
        None,
    );

    let tokens = models::list_api_tokens(&state.pool, user_id)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let token_data: Vec<minijinja::Value> = tokens
        .iter()
        .map(|t| {
            minijinja::context! {
                id => t.id,
                name => t.name,
                created_at => t.created_at,
            }
        })
        .collect();

    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/tokens.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            tokens => token_data,
            new_token => raw_token,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}

async fn delete_token(
    State(state): State<AppState>,
    session: Session,
    axum::extract::Path(token_id): axum::extract::Path<i64>,
) -> axum::response::Result<Redirect> {
    let user_id = auth::require_role(&session, "editor").await?;

    // Temporary: use app_id=1 (default) until tokens move to per-app settings
    let _ = models::delete_api_token(&state.pool, token_id, 1).await;
    state.audit.log(
        &user_id.to_string(),
        "token_delete",
        "api_token",
        &token_id.to_string(),
        None,
    );
    Ok(Redirect::to("/settings/tokens"))
}

async fn data_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<Html<String>> {
    auth::require_role(&session, "admin").await?;
    let csrf_token = auth::ensure_csrf_token(&session).await;

    // Consume flash message if present
    let mut import_status = String::new();
    let mut import_message = String::new();
    let mut import_warnings: Vec<String> = Vec::new();

    if let Some((kind, value)) = auth::take_flash(&session).await {
        if kind == "data_result" {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value) {
                import_status = parsed["status"].as_str().unwrap_or("").to_string();
                import_message = parsed["message"].as_str().unwrap_or("").to_string();
                if let Some(warnings) = parsed["warnings"].as_array() {
                    import_warnings = warnings
                        .iter()
                        .filter_map(|w| w.as_str().map(String::from))
                        .collect();
                }
            }
        }
    }

    let user_role = auth::current_user_role(&session).await.unwrap_or_default();
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/data.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            user_role => user_role,
            import_status => import_status,
            import_message => import_message,
            import_warnings => import_warnings,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}

async fn import_data(
    State(state): State<AppState>,
    session: Session,
    mut multipart: Multipart,
) -> axum::response::Result<axum::response::Response> {
    let user_id = auth::require_role(&session, "admin").await?;

    // Extract CSRF token and bundle from multipart fields
    let mut csrf_token = None;
    let mut bundle_data = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name().unwrap_or("") {
            "_csrf" => {
                if let Ok(text) = field.text().await {
                    csrf_token = Some(text);
                }
            }
            "bundle" => {
                if let Ok(bytes) = field.bytes().await {
                    if !bytes.is_empty() {
                        bundle_data = Some(bytes);
                    }
                }
            }
            _ => {}
        }
    }

    // Verify CSRF
    let csrf_valid = match &csrf_token {
        Some(token) => auth::verify_csrf_token(&session, token).await,
        None => false,
    };
    if !csrf_valid {
        auth::set_flash(
            &session,
            "data_result",
            &serde_json::json!({"status": "error", "message": "Invalid CSRF token", "warnings": []}).to_string(),
        ).await;
        return Ok(Redirect::to("/settings/data").into_response());
    }

    // Validate bundle present
    let data = match bundle_data {
        Some(d) => d,
        None => {
            auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({"status": "error", "message": "No file provided", "warnings": []}).to_string(),
            ).await;
            return Ok(Redirect::to("/settings/data").into_response());
        }
    };

    // Import
    match crate::sync::import_bundle_from_bytes(&state.config.data_dir, &state.pool, &data).await {
        Ok(warnings) => {
            crate::cache::rebuild(
                &state.cache,
                &state.config.schemas_dir(),
                &state.config.content_dir(),
            );
            state
                .audit
                .log(&user_id.to_string(), "import", "bundle", "", None);

            let (status, message) = if warnings.is_empty() {
                (
                    "success".to_string(),
                    "Bundle imported successfully".to_string(),
                )
            } else {
                (
                    "warning".to_string(),
                    format!("Bundle imported with {} warnings", warnings.len()),
                )
            };

            auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({
                    "status": status,
                    "message": message,
                    "warnings": warnings,
                })
                .to_string(),
            )
            .await;
        }
        Err(e) => {
            auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({"status": "error", "message": e.to_string(), "warnings": []})
                    .to_string(),
            )
            .await;
        }
    }

    Ok(Redirect::to("/settings/data").into_response())
}

async fn export_data(
    State(state): State<AppState>,
    session: Session,
    Form(_form): Form<std::collections::HashMap<String, String>>,
) -> axum::response::Result<axum::response::Response> {
    let user_id = auth::require_role(&session, "admin").await?;

    let tmp =
        std::env::temp_dir().join(format!("substrukt-export-{}.tar.gz", uuid::Uuid::new_v4()));

    Ok(
        match crate::sync::export_bundle(&state.config.data_dir, &state.pool, &tmp).await {
            Ok(()) => match std::fs::read(&tmp) {
                Ok(data) => {
                    let _ = std::fs::remove_file(&tmp);
                    state
                        .audit
                        .log(&user_id.to_string(), "export", "bundle", "", None);

                    let date = chrono::Utc::now().format("%Y-%m-%d");
                    let filename = format!("substrukt-export-{date}.tar.gz");
                    let disposition = format!("attachment; filename=\"{filename}\"");

                    let mut response = Body::from(data).into_response();
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/gzip"),
                    );
                    if let Ok(val) = HeaderValue::from_str(&disposition) {
                        response
                            .headers_mut()
                            .insert(header::CONTENT_DISPOSITION, val);
                    }
                    response
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    auth::set_flash(
                    &session,
                    "data_result",
                    &serde_json::json!({"status": "error", "message": format!("Export failed: {e}"), "warnings": []}).to_string(),
                ).await;
                    Redirect::to("/settings/data").into_response()
                }
            },
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                auth::set_flash(
                &session,
                "data_result",
                &serde_json::json!({"status": "error", "message": format!("Export failed: {e}"), "warnings": []}).to_string(),
            ).await;
                Redirect::to("/settings/data").into_response()
            }
        },
    )
}

async fn users_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<axum::response::Response> {
    let _user_id = auth::require_role(&session, "admin").await?;

    let invitations = models::list_pending_invitations(&state.pool)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let csrf_token = auth::ensure_csrf_token(&session).await;

    let inv_data: Vec<minijinja::Value> = invitations
        .iter()
        .map(|i| {
            minijinja::context! {
                id => i.id,
                email => i.email,
                role => i.role,
                created_at => i.created_at,
                expires_at => i.expires_at,
            }
        })
        .collect();

    let user_role = auth::current_user_role(&session).await.unwrap_or_default();
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/users.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            user_role => user_role,
            invitations => inv_data,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html).into_response())
}

#[derive(serde::Deserialize)]
pub struct InviteForm {
    email: String,
    role: String,
}

async fn invite_user(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<InviteForm>,
) -> axum::response::Result<axum::response::Response> {
    let user_id = auth::require_role(&session, "admin").await?;

    // Basic email validation
    if !form.email.contains('@') || form.email.len() < 3 {
        return render_users_with_error(&state, &session, is_htmx, "Invalid email address").await;
    }

    // Check if email already has an account
    if let Ok(Some(_)) = models::find_user_by_email(&state.pool, &form.email).await {
        return render_users_with_error(
            &state,
            &session,
            is_htmx,
            "A user with this email already exists",
        )
        .await;
    }

    // Check if already invited
    if let Ok(Some(_)) = models::find_invitation_by_email(&state.pool, &form.email).await {
        return render_users_with_error(
            &state,
            &session,
            is_htmx,
            "An invitation for this email already exists",
        )
        .await;
    }

    // Validate role
    let role = match form.role.as_str() {
        "admin" | "editor" | "viewer" => &form.role,
        _ => return render_users_with_error(&state, &session, is_htmx, "Invalid role").await,
    };

    let raw_token = token::generate_token();
    let token_hash = token::hash_token(&raw_token);
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(7)).to_rfc3339();

    let invitation = models::create_invitation(
        &state.pool,
        &form.email,
        &token_hash,
        user_id,
        &expires_at,
        role,
    )
    .await
    .map_err(|e| format!("DB error: {e}"))?;

    state.audit.log(
        &user_id.to_string(),
        "invite_create",
        "invitation",
        &invitation.id.to_string(),
        Some(&serde_json::json!({"email": form.email}).to_string()),
    );

    let invite_url = format!("/signup?token={raw_token}");

    let invitations = models::list_pending_invitations(&state.pool)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let inv_data: Vec<minijinja::Value> = invitations
        .iter()
        .map(|i| {
            minijinja::context! {
                id => i.id,
                email => i.email,
                role => i.role,
                created_at => i.created_at,
                expires_at => i.expires_at,
            }
        })
        .collect();

    let csrf_token = auth::ensure_csrf_token(&session).await;
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/users.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            invitations => inv_data,
            invite_url => invite_url,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html).into_response())
}

async fn render_users_with_error(
    state: &AppState,
    session: &Session,
    is_htmx: bool,
    error: &str,
) -> axum::response::Result<axum::response::Response> {
    let invitations = models::list_pending_invitations(&state.pool)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let inv_data: Vec<minijinja::Value> = invitations
        .iter()
        .map(|i| {
            minijinja::context! {
                id => i.id,
                email => i.email,
                role => i.role,
                created_at => i.created_at,
                expires_at => i.expires_at,
            }
        })
        .collect();

    let csrf_token = auth::ensure_csrf_token(session).await;
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/users.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            invitations => inv_data,
            error => error,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html).into_response())
}

async fn delete_invitation(
    State(state): State<AppState>,
    session: Session,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> axum::response::Result<axum::response::Response> {
    let user_id = auth::require_role(&session, "admin").await?;

    let _ = models::delete_invitation(&state.pool, id).await;
    state.audit.log(
        &user_id.to_string(),
        "invite_delete",
        "invitation",
        &id.to_string(),
        None,
    );
    Ok(Redirect::to("/settings/users").into_response())
}

#[derive(serde::Deserialize, Default)]
pub struct AuditLogFilter {
    #[serde(default)]
    action: String,
    #[serde(default)]
    actor: String,
    #[serde(default)]
    page: String,
}

async fn audit_log_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    axum::extract::Query(filter): axum::extract::Query<AuditLogFilter>,
) -> axum::response::Result<Html<String>> {
    auth::require_role(&session, "admin").await?;

    let page: u32 = filter.page.parse().unwrap_or(1).max(1);

    let action_filter = if filter.action.is_empty() {
        None
    } else {
        Some(filter.action.as_str())
    };
    let actor_filter = if filter.actor.is_empty() {
        None
    } else {
        Some(filter.actor.as_str())
    };

    let (entries, has_next) = state
        .audit
        .list_audit_log(action_filter, actor_filter, None, page)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let actors = state
        .audit
        .list_audit_actors()
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let entry_data: Vec<minijinja::Value> = entries
        .iter()
        .map(|e| {
            minijinja::context! {
                id => e.id,
                timestamp => e.timestamp,
                actor => e.actor,
                action => e.action,
                resource_type => e.resource_type,
                resource_id => e.resource_id,
                details => e.details,
            }
        })
        .collect();

    let mut pagination_params = Vec::new();
    if !filter.action.is_empty() {
        pagination_params.push(format!("action={}", filter.action));
    }
    if !filter.actor.is_empty() {
        pagination_params.push(format!("actor={}", filter.actor));
    }
    let pagination_qs = if pagination_params.is_empty() {
        String::new()
    } else {
        format!("{}&", pagination_params.join("&"))
    };

    let user_role = auth::current_user_role(&session).await.unwrap_or_default();
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/audit_log.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            user_role => user_role,
            entries => entry_data,
            actors => actors,
            filter_action => filter.action,
            filter_actor => filter.actor,
            pagination_qs => pagination_qs,
            page => page,
            has_next => has_next,
            has_prev => page > 1,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}

// ── Backups ─────────────────────────────────────────────────

async fn backups_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<Html<String>> {
    auth::require_role(&session, "admin").await?;

    let config = state
        .audit
        .get_backup_config()
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let latest_backup = state
        .audit
        .latest_backup()
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let history = state
        .audit
        .list_backup_history(10)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let s3_configured = state.s3_config.is_some();
    let backup_running = state.backup_running.load(Ordering::SeqCst);

    // Next backup info
    let next_backup_info = if config.enabled && s3_configured {
        let last_success = state.audit.last_successful_backup().await.ok().flatten();
        let delay =
            backup::calculate_next_backup_delay(last_success.as_ref(), config.frequency_hours);
        if delay.is_zero() {
            "Imminent".to_string()
        } else {
            let hours = delay.as_secs() / 3600;
            let mins = (delay.as_secs() % 3600) / 60;
            if hours > 0 {
                format!("In {} hours {} minutes", hours, mins)
            } else {
                format!("In {} minutes", mins)
            }
        }
    } else {
        String::new()
    };

    // Credential status
    let credential_status: Vec<minijinja::Value> = [
        (
            "SUBSTRUKT_S3_ENDPOINT",
            std::env::var("SUBSTRUKT_S3_ENDPOINT").is_ok(),
        ),
        (
            "SUBSTRUKT_S3_BUCKET",
            std::env::var("SUBSTRUKT_S3_BUCKET").is_ok(),
        ),
        (
            "SUBSTRUKT_S3_ACCESS_KEY",
            std::env::var("SUBSTRUKT_S3_ACCESS_KEY").is_ok(),
        ),
        (
            "SUBSTRUKT_S3_SECRET_KEY",
            std::env::var("SUBSTRUKT_S3_SECRET_KEY").is_ok(),
        ),
        (
            "SUBSTRUKT_S3_REGION",
            std::env::var("SUBSTRUKT_S3_REGION").is_ok(),
        ),
        (
            "SUBSTRUKT_S3_PATH_STYLE",
            std::env::var("SUBSTRUKT_S3_PATH_STYLE").is_ok(),
        ),
    ]
    .iter()
    .map(|(name, present)| {
        minijinja::context! {
            name => *name,
            present => *present,
        }
    })
    .collect();

    // Flash message
    let (flash_kind, flash_message) = match auth::take_flash(&session).await {
        Some((kind, msg)) => (kind, msg),
        None => (String::new(), String::new()),
    };

    let csrf_token = auth::ensure_csrf_token(&session).await;
    let user_role = auth::current_user_role(&session).await.unwrap_or_default();

    let latest_ctx = latest_backup.as_ref().map(|b| {
        minijinja::context! {
            status => b.status,
            started_at => b.started_at,
            error_message => b.error_message,
            size_bytes => b.size_bytes,
        }
    });

    let history_ctx: Vec<minijinja::Value> = history
        .iter()
        .map(|b| {
            minijinja::context! {
                started_at => b.started_at,
                status => b.status,
                trigger_source => b.trigger_source,
                size_bytes => b.size_bytes,
                s3_key => b.s3_key,
                error_message => b.error_message,
            }
        })
        .collect();

    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template("settings/backups.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            csrf_token => csrf_token,
            user_role => user_role,
            config => minijinja::context! {
                frequency_hours => config.frequency_hours,
                retention_count => config.retention_count,
                enabled => config.enabled,
            },
            latest_backup => latest_ctx,
            history => history_ctx,
            s3_configured => s3_configured,
            backup_running => backup_running,
            next_backup_info => next_backup_info,
            credential_status => credential_status,
            flash_kind => flash_kind,
            flash_message => flash_message,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}

#[derive(serde::Deserialize)]
struct BackupConfigForm {
    frequency_hours: i64,
    retention_count: i64,
    enabled: Option<String>,
}

async fn update_backup_config(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<BackupConfigForm>,
) -> axum::response::Result<Redirect> {
    let user_id = auth::require_role(&session, "admin").await?;

    // Validate
    let valid_frequencies = [1, 6, 12, 24, 48, 168];
    if !valid_frequencies.contains(&form.frequency_hours) {
        auth::set_flash(&session, "error", "Invalid frequency").await;
        return Ok(Redirect::to("/settings/backups"));
    }
    if form.retention_count < 1 || form.retention_count > 100 {
        auth::set_flash(&session, "error", "Retention count must be 1-100").await;
        return Ok(Redirect::to("/settings/backups"));
    }

    let enabled = form.enabled.is_some();

    state
        .audit
        .update_backup_config(form.frequency_hours, form.retention_count, enabled)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    state.audit.log(
        &user_id.to_string(),
        "backup_config_changed",
        "backup_config",
        "1",
        Some(
            &serde_json::json!({
                "frequency_hours": form.frequency_hours,
                "retention_count": form.retention_count,
                "enabled": enabled,
            })
            .to_string(),
        ),
    );

    auth::set_flash(&session, "success", "Backup configuration updated").await;
    Ok(Redirect::to("/settings/backups"))
}

async fn trigger_backup(
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<Redirect> {
    let user_id = auth::require_role(&session, "admin").await?;

    if state.s3_config.is_none() {
        auth::set_flash(&session, "error", "S3 not configured").await;
        return Ok(Redirect::to("/settings/backups"));
    }

    if state.backup_running.load(Ordering::SeqCst) {
        auth::set_flash(&session, "error", "Backup already in progress").await;
        return Ok(Redirect::to("/settings/backups"));
    }

    if let Some(tx) = &state.backup_trigger {
        if tx.try_send(()).is_err() {
            auth::set_flash(&session, "error", "Backup trigger channel full").await;
            return Ok(Redirect::to("/settings/backups"));
        }
    } else {
        auth::set_flash(&session, "error", "Backup not available").await;
        return Ok(Redirect::to("/settings/backups"));
    }

    state.audit.log(
        &user_id.to_string(),
        "backup_triggered",
        "backup",
        "",
        Some(&serde_json::json!({"trigger": "manual"}).to_string()),
    );

    auth::set_flash(&session, "success", "Backup triggered").await;
    Ok(Redirect::to("/settings/backups"))
}
