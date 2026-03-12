use axum::{
    Router,
    body::Body,
    extract::{Multipart, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Json},
    routing::{get, post},
};

use crate::state::AppState;
use crate::uploads;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(upload_file))
        .route("/file/{hash}/{filename}", get(serve_upload))
        .route("/file/{hash}", get(serve_upload_no_name))
}

async fn upload_file(State(state): State<AppState>, mut multipart: Multipart) -> impl IntoResponse {
    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field.file_name().unwrap_or("file").to_string();
        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response();
            }
        };

        if data.is_empty() {
            continue;
        }

        match uploads::store_upload(&state.config.uploads_dir(), &filename, &content_type, &data) {
            Ok(meta) => {
                return Json(serde_json::json!({
                    "hash": meta.hash,
                    "filename": meta.filename,
                    "mime": meta.mime,
                    "size": meta.size,
                }))
                .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response();
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "No file provided"})),
    )
        .into_response()
}

async fn serve_upload(
    State(state): State<AppState>,
    Path((hash, _filename)): Path<(String, String)>,
) -> impl IntoResponse {
    serve_file(&state, &hash)
}

async fn serve_upload_no_name(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> impl IntoResponse {
    serve_file(&state, &hash)
}

pub fn serve_upload_by_hash(state: &AppState, hash: &str) -> axum::response::Response {
    serve_file(state, hash)
}

fn serve_file(state: &AppState, hash: &str) -> axum::response::Response {
    let path = match uploads::get_upload_path(&state.config.uploads_dir(), hash) {
        Some(p) => p,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let meta = uploads::get_upload_meta(&state.config.uploads_dir(), hash);
    let content_type = meta
        .as_ref()
        .map(|m| m.mime.clone())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    match std::fs::read(&path) {
        Ok(data) => {
            let mut response = Body::from(data).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&content_type)
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            );
            if let Some(meta) = &meta {
                let disposition = format!("inline; filename=\"{}\"", meta.filename);
                if let Ok(val) = HeaderValue::from_str(&disposition) {
                    response
                        .headers_mut()
                        .insert(header::CONTENT_DISPOSITION, val);
                }
            }
            response
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
