use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Json, Response};
use axum_htmx::HxRequest;
use tower_sessions::Session;

use crate::config::Config;
use crate::db::models::{self, App};
use crate::routes::render_error_with_nav;
use crate::schema;
use crate::state::AppState;

/// Extractor for UI routes: resolves `{app_slug}` from the path, verifies
/// the app exists and the current user has access.
pub struct AppContext {
    pub app: App,
}

impl AppContext {
    /// List schemas for this app's schemas directory, returning minijinja-compatible values.
    pub fn nav_schemas(&self, config: &Config) -> Vec<minijinja::Value> {
        let schemas_dir = config.app_schemas_dir(&self.app.slug);
        match schema::list_schemas(&schemas_dir) {
            Ok(schemas) => schemas
                .iter()
                .map(|s| {
                    minijinja::context! {
                        title => s.meta.title,
                        slug => s.meta.slug,
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Return a minijinja context value representing this app.
    pub fn template_context(&self) -> minijinja::Value {
        minijinja::context! {
            id => self.app.id,
            slug => self.app.slug,
            name => self.app.name,
        }
    }
}

impl FromRequestParts<AppState> for AppContext {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Extract HxRequest to decide partial vs full template rendering
        let HxRequest(is_htmx) = HxRequest::from_request_parts(parts, state)
            .await
            .unwrap_or(HxRequest(false));

        // Extract session early so all error paths can include user nav context
        let session = parts.extensions.get::<Session>().cloned();
        let (user_role, current_username, csrf_token) = if let Some(ref s) = session {
            let role = crate::auth::current_user_role(s).await.unwrap_or_default();
            let username = crate::auth::current_username(s).await.unwrap_or_default();
            let csrf = crate::auth::ensure_csrf_token(s).await;
            (role, username, csrf)
        } else {
            (String::new(), String::new(), String::new())
        };

        let err_nav = |status: u16, msg: &str| {
            let html =
                render_error_with_nav(state, status, msg, is_htmx, &user_role, &current_username, &csrf_token);
            (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Html(html))
                .into_response()
        };

        // Extract app_slug from path params
        let params: HashMap<String, String> =
            match Path::<HashMap<String, String>>::from_request_parts(parts, state).await {
                Ok(Path(params)) => params,
                Err(_) => return Err(err_nav(404, "Not found")),
            };

        let slug = params
            .get("app_slug")
            .ok_or_else(|| err_nav(404, "Not found"))?;

        // Look up app
        let app = models::find_app_by_slug(&state.pool, slug)
            .await
            .map_err(|_| err_nav(500, "Internal error"))?
            .ok_or_else(|| err_nav(404, "App not found"))?;

        // Require authenticated session
        let session = session.ok_or_else(|| err_nav(500, "Session not available"))?;

        let user_id = crate::auth::current_user_id(&session)
            .await
            .ok_or_else(|| err_nav(403, "Not authenticated"))?;

        // Admins have access to all apps; others need explicit access
        if user_role != "admin" {
            let has_access = models::user_has_app_access(&state.pool, app.id, user_id)
                .await
                .map_err(|_| err_nav(500, "Internal error"))?;
            if !has_access {
                return Err(err_nav(403, "You do not have access to this app"));
            }
        }

        Ok(AppContext { app })
    }
}

/// Extractor for API routes: resolves `{app_slug}` from the path, verifies
/// the app exists. Does NOT check session/access (API auth is via bearer token).
pub struct ApiAppContext {
    pub app: App,
}

impl FromRequestParts<AppState> for ApiAppContext {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Extract app_slug from path params
        let params: HashMap<String, String> =
            match Path::<HashMap<String, String>>::from_request_parts(parts, state).await {
                Ok(Path(params)) => params,
                Err(_) => {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({"error": "Not found"})),
                    ));
                }
            };

        let slug = params.get("app_slug").ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Not found"})),
            )
        })?;

        // Look up app
        let app = models::find_app_by_slug(&state.pool, slug)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Internal error"})),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "App not found"})),
                )
            })?;

        Ok(ApiAppContext { app })
    }
}
