pub mod api;
pub mod apps;
pub mod auth;
pub mod content;
pub mod deployments;
pub mod schemas;
pub mod settings;
pub mod uploads;

use axum::{
    Router,
    extract::State,
    middleware,
    response::{Html, IntoResponse, Redirect},
};
use axum_htmx::HxRequest;
use tower_http::catch_panic::CatchPanicLayer;

use crate::auth::{require_auth, verify_csrf};
use crate::metrics;
use crate::state::AppState;
use crate::templates::base_for_htmx;

pub fn build_router(state: AppState) -> Router {
    let auth_routes = auth::routes();
    let settings_routes = settings::routes();
    let apps_management = apps::routes();
    let app_content = Router::new()
        .nest("/schemas", schemas::routes())
        .nest("/content", content::routes())
        .nest("/uploads", uploads::routes())
        .nest("/deployments", deployments::routes());

    let api_global = api::api_global_routes();
    let api_app_scoped = api::api_app_routes();
    let api_routes = Router::new()
        .merge(api_global)
        .nest("/apps/{app_slug}", api_app_scoped)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api::api_rate_limit,
        ));

    Router::new()
        .merge(auth_routes)
        .nest("/apps", apps_management)
        .nest("/apps/{app_slug}", app_content)
        .nest("/settings", settings_routes)
        .route("/", axum::routing::get(|| async { Redirect::to("/apps") }))
        .layer(middleware::from_fn(verify_csrf))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .nest("/api/v1", api_routes)
        .route("/healthz", axum::routing::get(healthz))
        .route("/metrics", axum::routing::get(metrics::metrics_handler))
        .fallback(not_found)
        .layer(middleware::from_fn(metrics::track_metrics))
        .layer(CatchPanicLayer::custom(handle_panic))
        .with_state(state)
}

fn handle_panic(_err: Box<dyn std::any::Any + Send + 'static>) -> axum::response::Response {
    let html = "<h1>500</h1><p>Internal server error</p>";
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Html(html.to_string()),
    )
        .into_response()
}

async fn not_found(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
) -> (axum::http::StatusCode, Html<String>) {
    let html = render_error(&state, 404, "Page not found", is_htmx);
    (axum::http::StatusCode::NOT_FOUND, Html(html))
}

pub fn render_error(state: &AppState, status: u16, message: &str, is_htmx: bool) -> String {
    let Ok(tmpl) = state.templates.acquire_env() else {
        return format!("<h1>{status}</h1><p>{message}</p>");
    };
    if let Ok(template) = tmpl.get_template("error.html")
        && let Ok(html) = template.render(minijinja::context! {
            base_template => base_for_htmx(is_htmx),
            status => status,
            message => message,
        })
    {
        return html;
    }
    format!("<h1>{status}</h1><p>{message}</p>")
}

async fn healthz() -> &'static str {
    "ok"
}
