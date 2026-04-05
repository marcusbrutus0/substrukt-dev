use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Json, Response};
use axum_htmx::HxRequest;
use tower_sessions::Session;

use crate::config::Config;
use crate::db::models::{self, App};
use crate::routes::render_error;
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

        // Extract app_slug from path params
        let params: HashMap<String, String> =
            match Path::<HashMap<String, String>>::from_request_parts(parts, state).await {
                Ok(Path(params)) => params,
                Err(_) => {
                    let html = render_error(state, 404, "Not found", is_htmx);
                    return Err((StatusCode::NOT_FOUND, Html(html)).into_response());
                }
            };

        let slug = params.get("app_slug").ok_or_else(|| {
            let html = render_error(state, 404, "Not found", is_htmx);
            (StatusCode::NOT_FOUND, Html(html)).into_response()
        })?;

        // Look up app
        let app = models::find_app_by_slug(&state.pool, slug)
            .await
            .map_err(|_| {
                let html = render_error(state, 500, "Internal error", is_htmx);
                (StatusCode::INTERNAL_SERVER_ERROR, Html(html)).into_response()
            })?
            .ok_or_else(|| {
                let html = render_error(state, 404, "App not found", is_htmx);
                (StatusCode::NOT_FOUND, Html(html)).into_response()
            })?;

        // Check access: get session from extensions (set by require_auth middleware)
        let session = parts.extensions.get::<Session>().cloned().ok_or_else(|| {
            let html = render_error(state, 500, "Session not available", is_htmx);
            (StatusCode::INTERNAL_SERVER_ERROR, Html(html)).into_response()
        })?;

        let user_id = crate::auth::current_user_id(&session)
            .await
            .ok_or_else(|| {
                let html = render_error(state, 403, "Not authenticated", is_htmx);
                (StatusCode::FORBIDDEN, Html(html)).into_response()
            })?;

        let user_role = crate::auth::current_user_role(&session)
            .await
            .unwrap_or_default();

        // Admins have access to all apps; others need explicit access
        if user_role != "admin" {
            let has_access = models::user_has_app_access(&state.pool, app.id, user_id)
                .await
                .map_err(|_| {
                    let html = render_error(state, 500, "Internal error", is_htmx);
                    (StatusCode::INTERNAL_SERVER_ERROR, Html(html)).into_response()
                })?;
            if !has_access {
                let html = render_error(state, 403, "You do not have access to this app", is_htmx);
                return Err((StatusCode::FORBIDDEN, Html(html)).into_response());
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
