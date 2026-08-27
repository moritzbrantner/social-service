use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

use crate::features::Feature;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("forbidden")]
    Forbidden,
    #[error("feature `{0}` is disabled")]
    FeatureDisabled(Feature),
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::FeatureDisabled(_) => (StatusCode::NOT_FOUND, "feature_disabled"),
            Self::Database(_) => {
                tracing::error!(error = ?self, "database request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            }
        };
        let message = self.to_string();
        (
            status,
            Json(ErrorBody {
                error: code,
                message,
            }),
        )
            .into_response()
    }
}
