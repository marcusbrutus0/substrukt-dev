use axum::{
    Form, Router,
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use tower_sessions::Session;

use crate::auth::{self, ensure_csrf_token, token::hash_token};
use crate::db::models;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", axum::routing::post(logout))
        .route("/setup", get(setup_page).post(setup_submit))
        .route("/signup", get(signup_page).post(signup_submit))
}

#[derive(serde::Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

#[derive(serde::Deserialize)]
pub struct SetupForm {
    username: String,
    password: String,
    confirm_password: String,
}

async fn login_page(
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<Html<String>> {
    let csrf_token = ensure_csrf_token(&session).await;
    render_template(
        &state,
        "login.html",
        minijinja::context! { csrf_token => csrf_token },
    )
    .await
}

async fn login_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    session: Session,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let ip = client_ip(&headers, state.config.trust_proxy_headers);
    if !state.login_limiter.check(&ip) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "Too many login attempts. Please try again later.",
        )
            .into_response();
    }

    let user = models::find_user_by_username(&state.pool, &form.username).await;
    match user {
        Ok(Some(user)) if user.verify_password(&form.password) => {
            if let Err(e) = auth::login_user(&session, user.id, &user.role).await {
                tracing::error!("Failed to create session: {e}");
                return Redirect::to("/login").into_response();
            }
            state
                .audit
                .log(&user.id.to_string(), "login", "session", "", None);
            Redirect::to("/").into_response()
        }
        _ => {
            let csrf_token = ensure_csrf_token(&session).await;
            let html = render_template(
                &state,
                "login.html",
                minijinja::context! {
                    csrf_token => csrf_token,
                    error => "Invalid username or password",
                },
            )
            .await;
            match html {
                Ok(h) => h.into_response(),
                Err(_) => Redirect::to("/login").into_response(),
            }
        }
    }
}

async fn logout(State(state): State<AppState>, session: Session) -> Redirect {
    let user_id = auth::current_user_id(&session).await;
    let _ = auth::logout_user(&session).await;
    let actor = user_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    state.audit.log(&actor, "logout", "session", "", None);
    Redirect::to("/login")
}

async fn setup_page(
    State(state): State<AppState>,
    session: Session,
) -> axum::response::Result<impl IntoResponse> {
    // If users already exist, redirect to login
    let count = models::user_count(&state.pool)
        .await
        .map_err(|e| format!("DB error: {e}"))?;
    if count > 0 {
        return Ok(Redirect::to("/login").into_response());
    }
    let csrf_token = ensure_csrf_token(&session).await;
    let html = render_template(
        &state,
        "setup.html",
        minijinja::context! { csrf_token => csrf_token },
    )
    .await?;
    Ok(html.into_response())
}

async fn setup_submit(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<SetupForm>,
) -> impl IntoResponse {
    // Check no users exist
    let count = models::user_count(&state.pool).await.unwrap_or(1);
    if count > 0 {
        return Redirect::to("/login").into_response();
    }

    if form.password != form.confirm_password {
        let csrf_token = ensure_csrf_token(&session).await;
        let html = render_template(
            &state,
            "setup.html",
            minijinja::context! {
                csrf_token => csrf_token,
                error => "Passwords do not match",
            },
        )
        .await;
        return match html {
            Ok(h) => h.into_response(),
            Err(_) => Redirect::to("/setup").into_response(),
        };
    }

    if form.username.is_empty() || form.password.len() < 8 {
        let csrf_token = ensure_csrf_token(&session).await;
        let html = render_template(
            &state,
            "setup.html",
            minijinja::context! {
                csrf_token => csrf_token,
                error => "Username required, password must be at least 8 characters",
            },
        )
        .await;
        return match html {
            Ok(h) => h.into_response(),
            Err(_) => Redirect::to("/setup").into_response(),
        };
    }

    match models::create_user(&state.pool, &form.username, &form.password, "admin").await {
        Ok(user) => {
            let _ = auth::login_user(&session, user.id, &user.role).await;
            state.audit.log(
                &user.id.to_string(),
                "user_create",
                "user",
                &user.id.to_string(),
                None,
            );
            Redirect::to("/").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create user: {e}");
            Redirect::to("/setup").into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct SignupForm {
    token: String,
    username: String,
    password: String,
    confirm_password: String,
}

async fn signup_page(
    State(state): State<AppState>,
    session: Session,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Result<impl IntoResponse> {
    let token = params.get("token").cloned().unwrap_or_default();
    if token.is_empty() {
        return Ok(Redirect::to("/login").into_response());
    }

    let token_hash = hash_token(&token);
    let invitation = models::find_invitation_by_token_hash(&state.pool, &token_hash)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let Some(invitation) = invitation else {
        return Ok(render_template(
            &state,
            "error.html",
            minijinja::context! {
                status => 400,
                message => "Invalid or expired invitation link",
            },
        )
        .await?
        .into_response());
    };

    let csrf_token = ensure_csrf_token(&session).await;
    let html = render_template(
        &state,
        "signup.html",
        minijinja::context! {
            csrf_token => csrf_token,
            token => token,
            email => invitation.email,
        },
    )
    .await?;
    Ok(html.into_response())
}

async fn signup_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    session: Session,
    Form(form): Form<SignupForm>,
) -> impl IntoResponse {
    let ip = client_ip(&headers, state.config.trust_proxy_headers);
    if !state.login_limiter.check(&ip) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Please try again later.",
        )
            .into_response();
    }

    let token_hash = hash_token(&form.token);
    let invitation = models::find_invitation_by_token_hash(&state.pool, &token_hash).await;
    let invitation = match invitation {
        Ok(Some(inv)) => inv,
        _ => {
            return render_signup_error(
                &state,
                &session,
                &form.token,
                "",
                "Invalid or expired invitation link",
            )
            .await;
        }
    };

    if form.username.is_empty() {
        return render_signup_error(
            &state,
            &session,
            &form.token,
            &invitation.email,
            "Username is required",
        )
        .await;
    }

    if form.password.len() < 8 {
        return render_signup_error(
            &state,
            &session,
            &form.token,
            &invitation.email,
            "Password must be at least 8 characters",
        )
        .await;
    }

    if form.password != form.confirm_password {
        return render_signup_error(
            &state,
            &session,
            &form.token,
            &invitation.email,
            "Passwords do not match",
        )
        .await;
    }

    // Check username uniqueness
    if let Ok(Some(_)) = models::find_user_by_username(&state.pool, &form.username).await {
        return render_signup_error(
            &state,
            &session,
            &form.token,
            &invitation.email,
            "Username is already taken",
        )
        .await;
    }

    // Create user with email
    match models::create_user_with_email(
        &state.pool,
        &form.username,
        &form.password,
        &invitation.email,
        &invitation.role,
    )
    .await
    {
        Ok(user) => {
            // Delete the invitation
            let _ = models::delete_invitation(&state.pool, invitation.id).await;
            // Grant access to all apps (non-admins need explicit app_access rows)
            if user.role != "admin"
                && let Ok(apps) = models::list_apps(&state.pool).await
            {
                for app in &apps {
                    let _ = models::grant_app_access(&state.pool, app.id, user.id).await;
                }
            }
            // Auto-login
            let _ = auth::login_user(&session, user.id, &user.role).await;
            state.audit.log(
                &user.id.to_string(),
                "user_create",
                "user",
                &user.id.to_string(),
                None,
            );
            Redirect::to("/").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create user: {e}");
            render_signup_error(
                &state,
                &session,
                &form.token,
                &invitation.email,
                "Failed to create account",
            )
            .await
        }
    }
}

async fn render_signup_error(
    state: &AppState,
    session: &Session,
    token: &str,
    email: &str,
    error: &str,
) -> axum::response::Response {
    let csrf_token = ensure_csrf_token(session).await;
    let html = render_template(
        state,
        "signup.html",
        minijinja::context! {
            csrf_token => csrf_token,
            token => token,
            email => email,
            error => error,
        },
    )
    .await;
    match html {
        Ok(h) => h.into_response(),
        Err(_) => Redirect::to("/login").into_response(),
    }
}

async fn render_template(
    state: &AppState,
    name: &str,
    ctx: minijinja::Value,
) -> axum::response::Result<Html<String>> {
    let tmpl = state
        .templates
        .acquire_env()
        .map_err(|e| format!("Template env error: {e}"))?;
    let template = tmpl
        .get_template(name)
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(ctx)
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}

fn client_ip(headers: &HeaderMap, trust_proxy_headers: bool) -> String {
    if trust_proxy_headers {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
            && let Some(first_ip) = xff.split(',').next()
        {
            return first_ip.trim().to_string();
        }
    }
    "direct".to_string()
}
