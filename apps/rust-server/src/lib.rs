pub mod api;
pub mod control;
pub mod media;
pub mod recordings;
pub mod storage;
pub mod topics;
pub mod whisper;
pub mod youtube;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::fmt;
pub use storage::Store;

pub type Result<T> = std::result::Result<T, AppError>;
#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}
impl AppError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
}
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for AppError {}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self {
            status: if e.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            },
            message: format!(
                "Local file operation failed ({}). Check that the project folder is available and writable.",
                e.kind()
            ),
        }
    }
}
impl From<serde_json::Error> for AppError {
    fn from(_: serde_json::Error) -> Self {
        Self::bad("Project data or input JSON is invalid.")
    }
}
impl From<reqwest::Error> for AppError {
    fn from(_: reqwest::Error) -> Self {
        Self::bad(
            "The provider request could not finish. Check connectivity and resume processing.",
        )
    }
}
impl From<String> for AppError {
    fn from(s: String) -> Self {
        Self::bad(s)
    }
}
impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        Self::bad(s)
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({"error":self.message}))).into_response()
    }
}
