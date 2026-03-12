pub mod token;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use tower_sessions::Session;

use crate::db::models;
use crate::state::AppState;

const USER_ID_KEY: &str = "user_id";

pub async fn login_user(session: &Session, user_id: i64) -> eyre::Result<()> {
    session
        .insert(USER_ID_KEY, user_id)
        .await
        .map_err(|e| eyre::eyre!("Session insert error: {e}"))?;
    Ok(())
}

pub async fn logout_user(session: &Session) -> eyre::Result<()> {
    session
        .flush()
        .await
        .map_err(|e| eyre::eyre!("Session flush error: {e}"))?;
    Ok(())
}

pub async fn current_user_id(session: &Session) -> Option<i64> {
    session.get::<i64>(USER_ID_KEY).await.ok().flatten()
}

/// Middleware: redirect to /setup if no users exist, or to /login if not authenticated.
/// Session is extracted from request extensions (set by the session layer below this).
pub async fn require_auth(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();

    // Allow public paths
    if path.starts_with("/login")
        || path.starts_with("/setup")
        || path.starts_with("/api/")
        || path.starts_with("/uploads/file/")
    {
        return next.run(request).await;
    }

    // Check if any users exist
    let user_count = models::user_count(&state.pool).await.unwrap_or(0);
    if user_count == 0 {
        return Redirect::to("/setup").into_response();
    }

    // Check session - get from request extensions
    let session = request.extensions().get::<Session>().cloned();
    match session {
        Some(session) => {
            if current_user_id(&session).await.is_none() {
                return Redirect::to("/login").into_response();
            }
        }
        None => {
            return Redirect::to("/login").into_response();
        }
    }

    next.run(request).await
}
