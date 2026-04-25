use crate::state::AppState;

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
}

pub fn client_ip(headers: &axum::http::HeaderMap, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = xff.split(',').next() {
                return first.trim().to_string();
            }
        }
    }
    "direct".to_string()
}
