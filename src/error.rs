use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

// One error type for the whole API. Each handler returns `Result<_, AppError>`,
// and Axum calls `into_response()` on the `Err` for us — so every error path
// produces the same JSON shape (`{"error": "..."}`) with the right status.
#[derive(Debug)]
pub enum AppError {
    /// The requested todo id doesn't exist.
    NotFound,
    /// The client sent something invalid (e.g. an empty title).
    Validation(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        // Centralized JSON error body. Change it once here, everywhere benefits.
        (status, Json(json!({ "error": message }))).into_response()
    }
}
