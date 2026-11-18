//! `GET/PATCH /api/limits` — ngân sách tài nguyên hiệu lực (AD-15).
//!
//! PATCH chỉ áp các field có mặt; phần còn lại giữ nguyên giá trị đang chạy.

use ananke_protocol::{LimitsDto, PatchLimitsRequest};
use axum::extract::State;
use axum::Json;

use crate::error::ApiResult;
use crate::state::AppState;

pub async fn get_limits(State(state): State<AppState>) -> ApiResult<Json<LimitsDto>> {
    let limits = state.limits.read().await;
    Ok(Json(*limits))
}

pub async fn patch_limits(
    State(state): State<AppState>,
    Json(body): Json<PatchLimitsRequest>,
) -> ApiResult<Json<LimitsDto>> {
    let mut limits = state.limits.write().await;

    if let Some(value) = body.max_resident_sessions {
        limits.max_resident_sessions = value;
    }
    if let Some(value) = body.max_parallel_turns {
        limits.max_parallel_turns = value;
    }
    if let Some(value) = body.max_subagents {
        limits.max_subagents = value;
    }
    if let Some(value) = body.max_context_tokens {
        limits.max_context_tokens = value;
    }
    if let Some(value) = body.max_part_bytes {
        limits.max_part_bytes = value;
    }
    if let Some(value) = body.max_event_bytes {
        limits.max_event_bytes = value;
    }

    Ok(Json(*limits))
}