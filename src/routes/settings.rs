use axum::{
    Form, Router,
    extract::State,
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

// --- User invitation management (admin only, user_id == 1) ---

async fn require_admin(session: &Session) -> Result<i64, axum::response::Response> {
    let user_id = auth::current_user_id(session)
        .await
        .ok_or_else(|| Redirect::to("/login").into_response())?;
    if user_id != 1 {
        return Err((axum::http::StatusCode::FORBIDDEN, "Admin access required").into_response());
    }
    Ok(user_id)
}

async fn users_page(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<axum::response::Response> {
    let _user_id = require_admin(&session)
        .await
        .map_err(|r| format!("{:?}", r))?;

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
    let user_id = require_admin(&session)
        .await
        .map_err(|r| format!("{:?}", r))?;

    // Basic email validation
    if !form.email.contains('@') || form.email.len() < 3 {
        return render_users_with_error(&state, &session, is_htmx, "Invalid email address").await;
    }

    // Check if email already has an account
    if let Ok(Some(_)) = models::find_user_by_email(&state.pool, &form.email).await {
        return render_users_with_error(&state, &session, is_htmx, "A user with this email already exists").await;
    }

    // Check if already invited
    if let Ok(Some(_)) = models::find_invitation_by_email(&state.pool, &form.email).await {
        return render_users_with_error(&state, &session, is_htmx, "An invitation for this email already exists").await;
    }

    let raw_token = token::generate_token();
    let token_hash = token::hash_token(&raw_token);
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(7)).to_rfc3339();

    let invitation = models::create_invitation(&state.pool, &form.email, &token_hash, user_id, &expires_at)
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
) -> axum::response::Response {
    let user_id = match require_admin(&session).await {
        Ok(id) => id,
        Err(r) => return r,
    };

    let _ = models::delete_invitation(&state.pool, id).await;
    state.audit.log(
        &user_id.to_string(),
        "invite_delete",
        "invitation",
        &id.to_string(),
        None,
    );
    Redirect::to("/settings/users").into_response()
}
