use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use campusunbloat_shared::ApiError;

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Bitte melde dich erneut an.")
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        log::error!("Campus Dual request failed: {error:#}");
        Self::new(
            StatusCode::BAD_GATEWAY,
            "Campus Dual konnte die Daten nicht liefern. Bitte versuche es erneut.",
        )
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiError {
                error: self.message,
            }),
        )
            .into_response()
    }
}
