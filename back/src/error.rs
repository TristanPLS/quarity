//! Erreur applicative unique → réponse HTTP JSON structurée.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(&'static str),
    Forbidden(&'static str),
    NotFound(&'static str),
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request".to_string(), m),
            AppError::Unauthorized(c) => (StatusCode::UNAUTHORIZED, c.to_string(), c.to_string()),
            AppError::Forbidden(c) => (StatusCode::FORBIDDEN, c.to_string(), c.to_string()),
            AppError::NotFound(c) => (StatusCode::NOT_FOUND, c.to_string(), c.to_string()),
            AppError::Internal(e) => {
                tracing::error!(error = ?e, "erreur interne");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error".to_string(),
                    "internal error".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Internal(e.into())
    }
}
impl From<redis::RedisError> for AppError {
    fn from(e: redis::RedisError) -> Self {
        AppError::Internal(e.into())
    }
}
impl From<crate::ch::ClickhouseError> for AppError {
    fn from(e: crate::ch::ClickhouseError) -> Self {
        AppError::Internal(e.into())
    }
}
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e)
    }
}
