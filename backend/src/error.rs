use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::errors::Error as JwtError;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug, Clone)]
pub enum AppError {
    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Rate limit exceeded")]
    RateLimit,

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("IO error: {0}")]
    Io(String),
}

impl AppError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Auth(_) => StatusCode::UNAUTHORIZED,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::RateLimit => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(serde_json::json!({
            "error": self.to_string(),
            "status": status.as_u16(),
        }));
        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        error!("Database error: {}", err);
        AppError::Database("Internal server error".to_string())
    }
}

impl From<JwtError> for AppError {
    fn from(err: JwtError) -> Self {
        error!("JWT error: {}", err);
        AppError::Auth("Invalid authentication token".to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        error!("IO error: {}", err);
        AppError::Io("Internal server error".to_string())
    }
}
