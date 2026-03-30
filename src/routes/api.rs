use axum::{
    Router,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Json},
    routing::{get, post},
};

use crate::auth::token::BearerToken;
use crate::content;
use crate::schema;
use crate::state::AppState;
use crate::uploads;

#[derive(serde::Deserialize, Default)]
pub struct ListParams {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub status: String,
}

pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/schemas", get(list_schemas))
        .route("/schemas/{slug}", get(get_schema))
        .route(
            "/content/{schema_slug}",
            get(list_entries).post(create_entry),
        )
        .route(
            "/content/{schema_slug}/single",
            get(get_single).put(upsert_single).delete(delete_single),
        )
        .route(
            "/content/{schema_slug}/{entry_id}/publish",
            post(api_publish_entry),
        )
        .route(
            "/content/{schema_slug}/{entry_id}/unpublish",
            post(api_unpublish_entry),
        )
        .route(
            "/content/{schema_slug}/{entry_id}",
            get(get_entry).put(update_entry).delete(delete_entry),
        )
        .route("/uploads", post(upload_file))
        .route("/uploads/{hash}", get(get_upload))
        .route("/export", post(export_bundle))
        .route("/import", post(import_bundle))
        .route("/publish/{environment}", post(publish))
        .layer(middleware::from_fn_with_state(state, api_rate_limit))
}

async fn api_rate_limit(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|xff| xff.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    if !state.api_limiter.check(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "Rate limit exceeded"})),
        )
            .into_response();
    }

    next.run(request).await
}

fn require_api_role(
    bearer: &BearerToken,
    min_role: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let role_level = |r: &str| -> u8 {
        match r {
            "admin" => 3,
            "editor" => 2,
            "viewer" => 1,
            _ => 0,
        }
    };
    if role_level(&bearer.role) >= role_level(min_role) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Insufficient permissions"})),
        ))
    }
}

fn resolve_references(
    data: &mut serde_json::Value,
    schema: &serde_json::Value,
    cache: &crate::state::ContentCache,
) {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return;
    };
    let Some(obj) = data.as_object_mut() else {
        return;
    };
    for (key, prop) in props {
        let is_ref = prop.get("type").and_then(|t| t.as_str()) == Some("string")
            && prop.get("format").and_then(|f| f.as_str()) == Some("reference");
        if !is_ref {
            continue;
        }
        let target_slug = prop
            .get("x-substrukt-reference")
            .and_then(|r| r.get("schema"))
            .and_then(|s| s.as_str());
        let Some(target_slug) = target_slug else {
            continue;
        };
        if let Some(serde_json::Value::String(ref_id)) = obj.get(key).cloned() {
            let cache_key = format!("{target_slug}/{ref_id}");
            if let Some(entry) = cache.get(&cache_key) {
                obj.insert(key.clone(), entry.value().clone());
            }
        }
    }
}

async fn list_schemas(State(state): State<AppState>, _token: BearerToken) -> impl IntoResponse {
    match schema::list_schemas(&state.config.schemas_dir()) {
        Ok(schemas) => {
            let data: Vec<serde_json::Value> = schemas
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "title": s.meta.title,
                        "slug": s.meta.slug,
                        "storage": s.meta.storage.to_string(),
                        "kind": s.meta.kind.to_string(),
                        "schema": s.schema,
                    })
                })
                .collect();
            Json(serde_json::json!(data)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_schema(
    State(state): State<AppState>,
    _token: BearerToken,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match schema::get_schema(&state.config.schemas_dir(), &slug) {
        Ok(Some(s)) => Json(s.schema).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_entries(
    State(state): State<AppState>,
    _token: BearerToken,
    Path(schema_slug): Path<String>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    match content::list_entries(&state.config.content_dir(), &schema_file) {
        Ok(entries) => {
            // Filter by status (default: published only)
            let status = if params.status.is_empty() {
                "published"
            } else {
                &params.status
            };
            let entries = content::filter_by_status(entries, status);

            let q = params.q.trim().to_string();
            let entries = if q.is_empty() {
                entries
            } else {
                content::filter_entries(entries, &q)
            };
            let data: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let mut d = content::strip_internal_status(&e.data);
                    resolve_references(&mut d, &schema_file.schema, &state.cache);
                    d
                })
                .collect();
            Json(serde_json::json!(data)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_entry(
    State(state): State<AppState>,
    _token: BearerToken,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    match content::get_entry(&state.config.content_dir(), &schema_file, &entry_id) {
        Ok(Some(entry)) => {
            let mut data = content::strip_internal_status(&entry.data);
            resolve_references(&mut data, &schema_file.schema, &state.cache);
            Json(data).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn create_entry(
    State(state): State<AppState>,
    token: BearerToken,
    Path(schema_slug): Path<String>,
    Json(data): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if schema_file.meta.kind == crate::schema::models::Kind::Single {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "This schema is a single. Use PUT /content/{slug}/single instead."})),
        )
            .into_response();
    }

    if let Err(errors) = content::validate_content(&schema_file, &data) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"errors": errors})),
        )
            .into_response();
    }

    let hashes = uploads::extract_upload_hashes(&data);
    match content::save_entry(&state.config.content_dir(), &schema_file, None, data) {
        Ok(id) => {
            crate::cache::reload_entry(
                &state.cache,
                &state.config.content_dir(),
                &schema_file,
                &id,
            );
            let _ = uploads::db_update_references(&state.pool, &schema_slug, &id, &hashes).await;
            state.audit.log(
                "api",
                "content_create",
                "content",
                &format!("{schema_slug}/{id}"),
                None,
            );
            (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn update_entry(
    State(state): State<AppState>,
    token: BearerToken,
    Path((schema_slug, entry_id)): Path<(String, String)>,
    Json(data): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if let Err(errors) = content::validate_content(&schema_file, &data) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"errors": errors})),
        )
            .into_response();
    }

    // Snapshot current version for history
    if let Ok(Some(current)) =
        content::get_entry(&state.config.content_dir(), &schema_file, &entry_id)
    {
        if let Err(e) = crate::history::snapshot_entry(
            &state.config.data_dir,
            &schema_slug,
            &entry_id,
            &current.data,
            state.config.version_history_count,
        ) {
            tracing::warn!("Failed to snapshot version: {e}");
        }
    }

    let hashes = uploads::extract_upload_hashes(&data);
    match content::save_entry(
        &state.config.content_dir(),
        &schema_file,
        Some(&entry_id),
        data,
    ) {
        Ok(_) => {
            crate::cache::reload_entry(
                &state.cache,
                &state.config.content_dir(),
                &schema_file,
                &entry_id,
            );
            let _ =
                uploads::db_update_references(&state.pool, &schema_slug, &entry_id, &hashes).await;
            state.audit.log(
                "api",
                "content_update",
                "content",
                &format!("{schema_slug}/{entry_id}"),
                None,
            );
            StatusCode::OK.into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_entry(
    State(state): State<AppState>,
    token: BearerToken,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    let _ = uploads::db_delete_references(&state.pool, &schema_slug, &entry_id).await;
    match content::delete_entry(&state.config.content_dir(), &schema_file, &entry_id) {
        Ok(()) => {
            crate::history::delete_history(&state.config.data_dir, &schema_slug, &entry_id);
            let key = format!("{schema_slug}/{entry_id}");
            state.cache.remove(&key);
            state.audit.log(
                "api",
                "content_delete",
                "content",
                &format!("{schema_slug}/{entry_id}"),
                None,
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_single(
    State(state): State<AppState>,
    _token: BearerToken,
    Path(schema_slug): Path<String>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    match content::get_entry(&state.config.content_dir(), &schema_file, "_single") {
        Ok(Some(entry)) => {
            // Check status filter
            let status = if params.status.is_empty() {
                "published"
            } else {
                &params.status
            };
            if status != "all" && content::get_entry_status(&entry.data) != status {
                return StatusCode::NOT_FOUND.into_response();
            }

            let mut data = content::strip_internal_status(&entry.data);
            resolve_references(&mut data, &schema_file.schema, &state.cache);
            Json(data).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn upsert_single(
    State(state): State<AppState>,
    token: BearerToken,
    Path(schema_slug): Path<String>,
    Json(data): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if let Err(errors) = content::validate_content(&schema_file, &data) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"errors": errors})),
        )
            .into_response();
    }

    // Snapshot current version for history
    if let Ok(Some(current)) =
        content::get_entry(&state.config.content_dir(), &schema_file, "_single")
    {
        if let Err(e) = crate::history::snapshot_entry(
            &state.config.data_dir,
            &schema_slug,
            "_single",
            &current.data,
            state.config.version_history_count,
        ) {
            tracing::warn!("Failed to snapshot version: {e}");
        }
    }

    let hashes = uploads::extract_upload_hashes(&data);
    match content::save_entry(
        &state.config.content_dir(),
        &schema_file,
        Some("_single"),
        data,
    ) {
        Ok(_) => {
            crate::cache::reload_entry(
                &state.cache,
                &state.config.content_dir(),
                &schema_file,
                "_single",
            );
            let _ =
                uploads::db_update_references(&state.pool, &schema_slug, "_single", &hashes).await;
            state.audit.log(
                "api",
                "content_update",
                "content",
                &format!("{schema_slug}/_single"),
                None,
            );
            StatusCode::OK.into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_single(
    State(state): State<AppState>,
    token: BearerToken,
    Path(schema_slug): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    let _ = uploads::db_delete_references(&state.pool, &schema_slug, "_single").await;
    match content::delete_entry(&state.config.content_dir(), &schema_file, "_single") {
        Ok(()) => {
            crate::history::delete_history(&state.config.data_dir, &schema_slug, "_single");
            let key = format!("{schema_slug}/_single");
            state.cache.remove(&key);
            state.audit.log(
                "api",
                "content_delete",
                "content",
                &format!("{schema_slug}/_single"),
                None,
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn api_publish_entry(
    State(state): State<AppState>,
    token: BearerToken,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if let Err(e) = content::set_entry_status(
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
        "published",
    ) {
        let msg = e.to_string();
        if msg.contains("not found") {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response();
    }

    crate::cache::reload_entry(
        &state.cache,
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
    );

    state.audit.log(
        "api",
        "entry_published",
        "content",
        &format!("{schema_slug}/{entry_id}"),
        None,
    );

    Json(serde_json::json!({"status": "published", "entry_id": entry_id})).into_response()
}

async fn api_unpublish_entry(
    State(state): State<AppState>,
    token: BearerToken,
    Path((schema_slug, entry_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    let schema_file = match schema::get_schema(&state.config.schemas_dir(), &schema_slug) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if let Err(e) = content::set_entry_status(
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
        "draft",
    ) {
        let msg = e.to_string();
        if msg.contains("not found") {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response();
    }

    crate::cache::reload_entry(
        &state.cache,
        &state.config.content_dir(),
        &schema_file,
        &entry_id,
    );

    state.audit.log(
        "api",
        "entry_unpublished",
        "content",
        &format!("{schema_slug}/{entry_id}"),
        None,
    );

    Json(serde_json::json!({"status": "draft", "entry_id": entry_id})).into_response()
}

async fn upload_file(
    State(state): State<AppState>,
    token: BearerToken,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
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

        match uploads::store_upload(
            &state.config.uploads_dir(),
            &state.pool,
            &filename,
            &content_type,
            &data,
        )
        .await
        {
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

async fn get_upload(
    State(state): State<AppState>,
    _token: BearerToken,
    Path(hash): Path<String>,
) -> impl IntoResponse {
    crate::routes::uploads::serve_upload_by_hash(&state, &hash).await
}

async fn export_bundle(State(state): State<AppState>, token: BearerToken) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "admin") {
        return e.into_response();
    }
    let tmp =
        std::env::temp_dir().join(format!("substrukt-export-{}.tar.gz", uuid::Uuid::new_v4()));
    match crate::sync::export_bundle(&state.config.data_dir, &state.pool, &tmp).await {
        Ok(()) => match std::fs::read(&tmp) {
            Ok(data) => {
                let _ = std::fs::remove_file(&tmp);
                state.audit.log("api", "export", "bundle", "", None);
                let mut response = axum::body::Body::from(data).into_response();
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/gzip"),
                );
                response.headers_mut().insert(
                    axum::http::header::CONTENT_DISPOSITION,
                    axum::http::HeaderValue::from_static("attachment; filename=\"bundle.tar.gz\""),
                );
                response
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response()
            }
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn import_bundle(
    State(state): State<AppState>,
    token: BearerToken,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "admin") {
        return e.into_response();
    }
    while let Ok(Some(field)) = multipart.next_field().await {
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

        match crate::sync::import_bundle_from_bytes(&state.config.data_dir, &state.pool, &data)
            .await
        {
            Ok(warnings) => {
                // Rebuild cache after import
                crate::cache::rebuild(
                    &state.cache,
                    &state.config.schemas_dir(),
                    &state.config.content_dir(),
                );
                state.audit.log("api", "import", "bundle", "", None);
                return Json(serde_json::json!({
                    "status": "ok",
                    "warnings": warnings,
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

async fn publish(
    State(state): State<AppState>,
    token: BearerToken,
    Path(environment): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_api_role(&token, "editor") {
        return e.into_response();
    }
    if !matches!(environment.as_str(), "staging" | "production") {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Unknown environment"})),
        )
            .into_response();
    }

    // For production: flip all drafts to published before firing webhook
    if environment == "production" {
        match content::publish_all_drafts(&state.config.schemas_dir(), &state.config.content_dir())
        {
            Ok(count) => {
                if count > 0 {
                    // Rebuild cache to pick up status changes
                    crate::cache::rebuild(
                        &state.cache,
                        &state.config.schemas_dir(),
                        &state.config.content_dir(),
                    );
                    tracing::info!("Published {count} draft entries");
                }
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Failed to publish drafts: {e}")})),
                )
                    .into_response();
            }
        }
    }

    match crate::webhooks::fire_webhook(
        &state.http_client,
        &state.audit,
        &state.config,
        &environment,
        crate::webhooks::TriggerSource::Manual,
    )
    .await
    {
        Ok(true) => Json(serde_json::json!({"status": "triggered"})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Webhook URL not configured"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
