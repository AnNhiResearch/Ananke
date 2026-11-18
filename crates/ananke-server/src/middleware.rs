//! Middleware kiểm tra header giao thức (`X-Ananke-Protocol`).

use ananke_protocol::{ProblemDetails, PROTOCOL_HEADER, PROTOCOL_VERSION};
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// Các path không yêu cầu header giao thức (probe + tài liệu OpenAPI).
pub const EXEMPT_PATHS: [&str; 3] = ["/healthz", "/readyz", "/api/openapi.json"];

/// Từ chối request thiếu/sai header giao thức với ProblemDetails 426.
pub async fn protocol_guard(req: Request, next: Next) -> Response {
    if EXEMPT_PATHS.contains(&req.uri().path()) {
        return next.run(req).await;
    }

    let expected = PROTOCOL_VERSION.to_string();
    let matches = req
        .headers()
        .get(PROTOCOL_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected);

    if matches {
        return next.run(req).await;
    }

    let problem = ProblemDetails::new(
        StatusCode::UPGRADE_REQUIRED.as_u16(),
        "Unsupported protocol",
    )
    .with_detail(format!(
        "cần header {PROTOCOL_HEADER}: {PROTOCOL_VERSION}"
    ))
    .with_code("protocol_mismatch");

    (
        StatusCode::UPGRADE_REQUIRED,
        [(header::CONTENT_TYPE, ananke_protocol::PROBLEM_CONTENT_TYPE)],
        Json(problem),
    )
        .into_response()
}