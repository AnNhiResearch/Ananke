//! Lỗi API theo RFC 9457 (`application/problem+json`), AD-03/AD-15.

use ananke_protocol::{ProblemDetails, PROBLEM_CONTENT_TYPE};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

/// Lỗi ở đường biên HTTP; mọi biến thể map sang một status + `code` ổn định.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    NotFound(ProblemDetails),
    BadRequest(ProblemDetails),
    Conflict(ProblemDetails),
    UnsupportedProtocol(ProblemDetails),
    NotImplemented(ProblemDetails),
    Internal(ProblemDetails),
    TooManyRequests(ProblemDetails),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str, &'static str, &ProblemDetails) {
        match self {
            Self::NotFound(problem) => (StatusCode::NOT_FOUND, "Not Found", "not_found", problem),
            Self::BadRequest(problem) => {
                (StatusCode::BAD_REQUEST, "Bad Request", "bad_request", problem)
            }
            Self::Conflict(problem) => (StatusCode::CONFLICT, "Conflict", "conflict", problem),
            Self::UnsupportedProtocol(problem) => (
                StatusCode::UPGRADE_REQUIRED,
                "Unsupported protocol",
                "protocol_mismatch",
                problem,
            ),
            Self::NotImplemented(problem) => (
                StatusCode::NOT_IMPLEMENTED,
                "Not Implemented",
                "not_implemented",
                problem,
            ),
            Self::Internal(problem) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "internal",
                problem,
            ),
            Self::TooManyRequests(problem) => (
                StatusCode::TOO_MANY_REQUESTS,
                "Too Many Requests",
                "resource_throttled",
                problem,
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, title, code, problem) = self.parts();
        let mut problem = problem.clone();
        // Extract values before moving
        let detail = problem.detail.clone().unwrap_or_else(|| title.to_owned());
        let code = problem.code.clone().unwrap_or_else(|| code.to_owned());
        problem = problem.with_detail(detail).with_code(code);
        (
            status,
            [(header::CONTENT_TYPE, PROBLEM_CONTENT_TYPE)],
            Json(problem),
        )
            .into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        tracing::error!(error = %err, "lỗi nội bộ khi xử lý request");
        let problem = ProblemDetails::new(
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            "Internal Server Error",
        )
        .with_detail(err.to_string())
        .with_code("internal");
        Self::Internal(problem)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        let problem = ProblemDetails::new(
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            "Internal Server Error",
        )
        .with_detail(err.to_string())
        .with_code("internal");
        Self::Internal(problem)
    }
}

/// Kết quả trả về của handler HTTP.
pub type ApiResult<T> = Result<T, ApiError>;