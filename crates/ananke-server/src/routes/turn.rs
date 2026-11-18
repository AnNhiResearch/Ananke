//! `POST /api/session/{id}/turn` — nhận input, chạy turn nền và trả 202 khi
//! engine đã phát `turn.started`. Interrupt/steer chưa hỗ trợ ở M0.

use std::sync::Arc;
use std::time::Duration;

use ananke_core::{Message, Role, TurnId, TurnStatus};
use ananke_protocol::event::EventPayloadDto;
use ananke_protocol::{CreateTurnRequest, SteerRequest, TurnAcceptedResponse};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use tokio::sync::broadcast::error::RecvError;

use crate::dto;
use crate::error::{ApiError, ApiResult};
use crate::routes::{internal, map_store_error, not_found, session_id};
use crate::state::AppState;

/// Chờ `turn.started` tối đa 10 giây; quá hạn coi như engine không khởi động.
const TURN_START_TIMEOUT: Duration = Duration::from_secs(10);

/// `POST /api/session/{id}/turn` — 202 + `turnID` ngay khi turn bắt đầu.
pub async fn create_turn(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Json(body): Json<CreateTurnRequest>,
) -> ApiResult<(StatusCode, Json<TurnAcceptedResponse>)> {
    let id = session_id(&raw_id);

    let exists = state.store.session_exists(&id).map_err(internal)?;
    if !exists {
        return Err(not_found(&id));
    }

    // AD-15: Check max_parallel_turns admission control (global limit across all sessions)
    // This check is deferred until after acquiring the session lock to ensure
    // we can see active turns in other sessions (which hold their locks).

    let handle = state
        .sessions
        .get_or_load(&state.store, &id)
        .await
        .map_err(map_store_error)?
        .ok_or_else(|| not_found(&id))?;

    // Giữ lock của session suốt turn: hai turn song song trên cùng session là 409.
    let guard = match handle.try_lock_owned() {
        Ok(guard) => guard,
        Err(_) => {
            let problem = ananke_protocol::ProblemDetails::new(
                StatusCode::CONFLICT.as_u16(),
                "Conflict",
            )
            .with_detail("turn already running")
            .with_code("conflict");
            return Err(ApiError::Conflict(problem));
        }
    };

    // AD-15: Check max_parallel_turns admission control (global limit across all sessions)
    // We do this AFTER acquiring the session lock so we can see active turns in other sessions
    // that are also holding their locks.
    {
        let limits = state.limits.read().await;
        let global_active_turns = state.sessions.global_active_turn_count();
        if global_active_turns >= limits.max_parallel_turns as usize {
            state.hub.emit_resource_throttled("max_parallel_turns".to_owned(), global_active_turns as u32, "rejected".to_owned());
            
            let detail = format!(
                "max parallel turns exceeded: {}/{} (limit: max_parallel_turns, current: {}, action: rejected)",
                global_active_turns, limits.max_parallel_turns, global_active_turns
            );
            let problem = ananke_protocol::ProblemDetails::new(
                StatusCode::TOO_MANY_REQUESTS.as_u16(),
                "Too Many Requests",
            )
            .with_detail(detail)
            .with_code("resource_throttled");
            
            return Err(ApiError::TooManyRequests(problem));
        }
    }

    let messages = vec![Message::new(Role::User, dto::input_parts(&body.input))];

    // Subscribe trước khi spawn để không lỡ `turn.started` phát ra quá sớm.
    let mut rx = state.hub.subscribe(&id);

    let engine = Arc::clone(&state.engine);
    let task_session = id.clone();
    let sessions = state.sessions.clone();
    // Increment active turns counter when turn starts
    state.sessions.increment_active_turns();
    tokio::spawn(async move {
        let mut guard = guard;
        let result = engine.run_turn(&mut guard, messages).await;
        if let Err(err) = &result {
            tracing::error!(session = %task_session, error = %err, "turn kết thúc với lỗi");
        }
        drop(guard);
        // Decrement active turns counter when turn ends
        sessions.decrement_active_turns();
    });

    let started = tokio::time::timeout(TURN_START_TIMEOUT, async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let EventPayloadDto::TurnStarted { turn_id } = event.payload {
                        return Some(turn_id);
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "bỏ qua event khi chờ turn.started");
                }
                Err(RecvError::Closed) => return None,
            }
        }
    })
    .await;

    match started {
        Ok(Some(turn_id)) => Ok((
            StatusCode::ACCEPTED,
            Json(TurnAcceptedResponse { turn_id }),
        )),
        Ok(None) => {
            let problem = ananke_protocol::ProblemDetails::new(
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "Internal Server Error",
            )
            .with_detail("turn event channel closed")
            .with_code("internal");
            Err(ApiError::Internal(problem))
        }
        Err(_) => {
            let problem = ananke_protocol::ProblemDetails::new(
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "Internal Server Error",
            )
            .with_detail("turn did not start")
            .with_code("internal");
            Err(ApiError::Internal(problem))
        }
    }
}

/// `POST /api/session/{id}/turn/{turnID}/interrupt` — cancel turn đang chạy.
pub async fn interrupt_turn(
    State(state): State<AppState>,
    Path((raw_session, raw_turn)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let session_id = session_id(&raw_session);
    let turn_id = TurnId::from(raw_turn);

    let handle = state
        .sessions
        .get_or_load(&state.store, &session_id)
        .await
        .map_err(map_store_error)?
        .ok_or_else(|| not_found(&session_id))?;

    let mut session = handle.lock().await;
    
    // Check turn status before attempting to cancel
    let can_cancel = {
        let turn = session.turn(&turn_id).ok_or_else(|| not_found(&session_id))?;
        matches!(turn.status, TurnStatus::Running | TurnStatus::AwaitingTool)
    };
    
    if !can_cancel {
        let detail = "turn không ở trạng thái cho phép interrupt (chỉ Running hoặc AwaitingTool) - action: interrupt, allowed_states: [running, awaiting_tool]".to_owned();
        let problem = ananke_protocol::ProblemDetails::new(
            StatusCode::BAD_REQUEST.as_u16(),
            "Bad Request",
        )
        .with_detail(detail)
        .with_code("bad_state");
        return Err(ApiError::BadRequest(problem));
    }

    state
        .engine
        .cancel_turn(&mut session, &turn_id)
        .await
        .map_err(|e| {
            let problem = ananke_protocol::ProblemDetails::new(
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "Internal Server Error",
            )
            .with_detail(e.to_string())
            .with_code("internal");
            ApiError::Internal(problem)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/session/{id}/turn/{turnID}/steer` — điều hướng turn đang chạy.
pub async fn steer_turn(
    State(state): State<AppState>,
    Path((raw_session, raw_turn)): Path<(String, String)>,
    Json(body): Json<SteerRequest>,
) -> ApiResult<StatusCode> {
    let session_id = session_id(&raw_session);
    let turn_id = TurnId::from(raw_turn);

    let handle = state
        .sessions
        .get_or_load(&state.store, &session_id)
        .await
        .map_err(map_store_error)?
        .ok_or_else(|| not_found(&session_id))?;

    let mut session = handle.lock().await;
    
    // Check turn status before attempting to steer
    let can_steer = {
        let turn = session.turn(&turn_id).ok_or_else(|| not_found(&session_id))?;
        matches!(turn.status, TurnStatus::Running | TurnStatus::AwaitingTool)
    };
    
    if !can_steer {
        let detail = "turn không ở trạng thái cho phép steer (chỉ Running hoặc AwaitingTool) - action: steer, allowed_states: [running, awaiting_tool]".to_owned();
        let problem = ananke_protocol::ProblemDetails::new(
            StatusCode::BAD_REQUEST.as_u16(),
            "Bad Request",
        )
        .with_detail(detail)
        .with_code("bad_state");
        return Err(ApiError::BadRequest(problem));
    }

    let input_messages = vec![Message::new(Role::User, dto::input_parts(&body.input))];

    state
        .engine
        .steer_turn(&mut session, &turn_id, input_messages)
        .await
        .map_err(|e| {
            let problem = ananke_protocol::ProblemDetails::new(
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "Internal Server Error",
            )
            .with_detail(e.to_string())
            .with_code("internal");
            ApiError::Internal(problem)
        })?;

    Ok(StatusCode::NO_CONTENT)
}