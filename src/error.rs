use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::header::WWW_AUTHENTICATE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tokio::task::JoinError;
use tracing::{debug, error};

use crate::mail::BuildError;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("missing or invalid bearer token")]
    Unauthorized,
    #[error("{}", .0.body_text())]
    Json(#[from] JsonRejection),
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error("failed to build message: {0}")]
    Join(#[from] JoinError),
    #[error("smtp delivery failed: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Json(rejection) => rejection.status(),
            Self::Build(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Join(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Smtp(error) if error.is_timeout() => StatusCode::GATEWAY_TIMEOUT,
            Self::Smtp(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = self.to_string();
        if status.is_server_error() {
            error!("{message}");
        } else {
            debug!("{message}");
        }

        let mut response = (status, Json(ErrorBody { error: message })).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}
