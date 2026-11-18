//! `POST /api/session/{id}/permission/{permID}` — trả lời yêu cầu phê duyệt.
//!
//! Broker đang chờ oneshot của engine; resolve thất bại nghĩa là yêu cầu đã hết
//! hạn hoặc không tồn tại nên trả 404 (AD-07).

use ananke_core::{Decision, PermissionId};
use ananke_protocol::{PermissionDecisionDto, PermissionDecisionRequest, PermissionScopeDto};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub async fn resolve_permission(
    State(state): State<AppState>,
    Path((raw_session, raw_permission)): Path<(String, String)>,
    Json(body): Json<PermissionDecisionRequest>,
) -> ApiResult<StatusCode> {
    let permission_id = PermissionId::from(raw_permission.clone());
    let session_id = ananke_core::SessionId::from(raw_session.clone());
    
    let (decision, scope) = match body.decision {
        PermissionDecisionDto::Allow => (Decision::AllowOnce, None),
        PermissionDecisionDto::Deny => (Decision::Deny, None),
        PermissionDecisionDto::AllowAlways => {
            // Convert PermissionScopeDto to internal scope representation
            let scope = body.scope.map(|s| match s {
                PermissionScopeDto::Once => "once".to_owned(),
                PermissionScopeDto::Session => "session".to_owned(),
                PermissionScopeDto::Project => "project".to_owned(),
            });
            (Decision::AllowAlways, scope)
        }
    };

    // Pass scope to broker for AllowAlways persistence
    let resolved = state
        .approval
        .resolve(&permission_id, decision, scope.clone(), &session_id);
    
    if !resolved {
        let problem = ananke_protocol::ProblemDetails::new(
            axum::http::StatusCode::NOT_FOUND.as_u16(),
            "Not Found",
        )
        .with_detail("permission request not found".to_owned())
        .with_code("not_found");
        return Err(ApiError::NotFound(problem));
    }

    tracing::debug!(session = %raw_session, permission = %raw_permission, ?scope, "đã trả lời yêu cầu phê duyệt");
    Ok(StatusCode::NO_CONTENT)
}