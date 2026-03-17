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
    let user_id = auth::current_user_id(&session)
        .await
        .ok_or("Not authenticated")?;

    let raw_token = token::generate_token();
    let token_hash = token::hash_token(&raw_token);

    let api_token = models::create_api_token(&state.pool, user_id, &form.name, &token_hash)
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
) -> impl IntoResponse {
    let user_id = match auth::current_user_id(&session).await {
        Some(id) => id,
        None => return Redirect::to("/login"),
    };

    let _ = models::delete_api_token(&state.pool, token_id, user_id).await;
    state.audit.log(
        &user_id.to_string(),
        "token_delete",
        "api_token",
        &token_id.to_string(),
        None,
    );
    Redirect::to("/settings/tokens")
}

async fn data_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<Html<String>> {
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
) -> impl IntoResponse {
    let user_id = match auth::current_user_id(&session).await {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

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
        return Redirect::to("/settings/data").into_response();
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
            return Redirect::to("/settings/data").into_response();
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

    Redirect::to("/settings/data").into_response()
}

async fn export_data(
    State(state): State<AppState>,
    session: Session,
    Form(_form): Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let user_id = match auth::current_user_id(&session).await {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let tmp =
        std::env::temp_dir().join(format!("substrukt-export-{}.tar.gz", uuid::Uuid::new_v4()));

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
    }
}

// --- User invitation management (admin only, user_id == 1) ---

async fn require_admin(session: &Session) -> axum::response::Result<i64> {
    let user_id =
        auth::current_user_id(session)
            .await
            .ok_or(axum::response::ErrorResponse::from(
                (axum::http::StatusCode::SEE_OTHER, [("location", "/login")]).into_response(),
            ))?;
    if user_id != 1 {
        return Err(axum::response::ErrorResponse::from(
            (axum::http::StatusCode::FORBIDDEN, "Admin access required").into_response(),
        ));
    }
    Ok(user_id)
}

async fn users_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<axum::response::Response> {
    let _user_id = require_admin(&session).await?;

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
                created_at => i.created_at,
                expires_at => i.expires_at,
            }
        })
        .collect();

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
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html).into_response())
}

#[derive(serde::Deserialize)]
pub struct InviteForm {
    email: String,
}

async fn invite_user(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<InviteForm>,
) -> axum::response::Result<axum::response::Response> {
    let user_id = require_admin(&session).await?;

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

    let raw_token = token::generate_token();
    let token_hash = token::hash_token(&raw_token);
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(7)).to_rfc3339();

    let invitation =
        models::create_invitation(&state.pool, &form.email, &token_hash, user_id, &expires_at)
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
    let user_id = require_admin(&session).await?;

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
