//! Handler HTTP cho toàn bộ `/api` (§5). Mỗi file con là một nhóm endpoint;
//! phần dùng chung chỉ là map id/lỗi để các handler không lặp code.

pub mod limits;
pub mod meta;
pub mod models;
pub mod permission;
pub mod session;
pub mod sse;
pub mod turn;

use std::fmt::Display;

use ananke_core::{SessionId, StoreError};

use crate::error::ApiError;

/// Path param của axum là `String`; id nội bộ là newtype nên đổi ở một chỗ.
pub(crate) fn session_id(raw: &str) -> SessionId {
    SessionId::from(raw)
}

/// 404 thống nhất cho session không có trong store.
pub(crate) fn not_found(id: &SessionId) -> ApiError {
    let problem = ananke_protocol::ProblemDetails::new(
        axum::http::StatusCode::NOT_FOUND.as_u16(),
        "Not Found",
    )
    .with_detail(format!("session {id} không tồn tại"))
    .with_code("not_found");
    ApiError::NotFound(problem)
}

/// Lỗi hạ tầng (rusqlite, DB hỏng...) thành 500; chi tiết giữ nguyên để log.
pub(crate) fn internal(err: impl Display) -> ApiError {
    let problem = ananke_protocol::ProblemDetails::new(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        "Internal Server Error",
    )
    .with_detail(err.to_string())
    .with_code("internal");
    ApiError::Internal(problem)
}

/// `StoreError::NotFound` vẫn là 404; còn lại là lỗi nội bộ 500.
pub(crate) fn map_store_error(err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound(detail) => {
            let problem = ananke_protocol::ProblemDetails::new(
                axum::http::StatusCode::NOT_FOUND.as_u16(),
                "Not Found",
            )
            .with_detail(detail)
            .with_code("not_found");
            ApiError::NotFound(problem)
        }
        StoreError::Failed(detail) => {
            let problem = ananke_protocol::ProblemDetails::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "Internal Server Error",
            )
            .with_detail(detail)
            .with_code("internal");
            ApiError::Internal(problem)
        }
    }
}