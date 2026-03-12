pub mod api;
pub mod auth;
pub mod content;
pub mod schemas;
pub mod settings;
pub mod uploads;

use axum::{Router, extract::State, middleware, response::Html};

use crate::auth::require_auth;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let api_routes = api::routes();
    let auth_routes = auth::routes();
    let schema_routes = schemas::routes();
    let content_routes = content::routes();
    let upload_routes = uploads::routes();
    let settings_routes = settings::routes();

    Router::new()
        .merge(auth_routes)
        .nest("/schemas", schema_routes)
        .nest("/content", content_routes)
        .nest("/uploads", upload_routes)
        .nest("/settings", settings_routes)
        .route("/", axum::routing::get(dashboard))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .nest("/api/v1", api_routes)
        .with_state(state)
}

async fn dashboard(State(state): State<AppState>) -> axum::response::Result<Html<String>> {
    let schemas = crate::schema::list_schemas(&state.config.schemas_dir()).unwrap_or_default();
    let entry_count: usize = schemas
        .iter()
        .filter_map(|s| crate::content::list_entries(&state.config.content_dir(), s).ok())
        .map(|entries| entries.len())
        .sum();

    let tmpl = state.templates.read().await;
    let template = tmpl
        .get_template("dashboard.html")
        .map_err(|e| format!("Template error: {e}"))?;
    let html = template
        .render(minijinja::context! {
            schema_count => schemas.len(),
            entry_count => entry_count,
        })
        .map_err(|e| format!("Render error: {e}"))?;
    Ok(Html(html))
}
