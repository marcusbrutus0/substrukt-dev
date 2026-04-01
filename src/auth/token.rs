use axum::{extract::FromRequestParts, http::request::Parts};
use sha2::{Digest, Sha256};

use crate::db::models::{self, ApiToken};
use crate::state::AppState;

pub fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: [u8; 32] = rng.random();
    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct BearerToken {
    pub token: ApiToken,
    pub role: String,
}

impl FromRequestParts<AppState> for BearerToken {
    type Rejection = axum::http::StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(axum::http::StatusCode::UNAUTHORIZED)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(axum::http::StatusCode::UNAUTHORIZED)?;

        let token_hash = hash_token(token);
        let api_token = models::find_token_by_hash(&state.pool, &token_hash)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(axum::http::StatusCode::UNAUTHORIZED)?;

        let role = models::find_user_role(&state.pool, api_token.user_id)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(axum::http::StatusCode::UNAUTHORIZED)?;

        Ok(BearerToken {
            token: api_token,
            role,
        })
    }
}
