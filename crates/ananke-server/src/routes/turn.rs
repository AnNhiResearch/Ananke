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

    // AD-15: Check admission control (max_parallel_turns & max_part_bytes)
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

        // Validate max_subagents (AD-15)
        let active_subagents = state.sessions.global_active_subagent_count();
        if active_subagents >= limits.max_subagents as usize {
            state.hub.emit_resource_throttled("max_subagents".to_owned(), active_subagents as u32, "rejected".to_owned());
            let detail = format!(
                "max subagents exceeded: {}/{} (limit: max_subagents, current: {}, action: rejected)",
                active_subagents, limits.max_subagents, active_subagents
            );
            let problem = ananke_protocol::ProblemDetails::new(
                StatusCode::TOO_MANY_REQUESTS.as_u16(),
                "Too Many Requests",
            )
            .with_detail(detail)
            .with_code("resource_throttled");
            return Err(ApiError::TooManyRequests(problem));
        }

        // Validate max_part_bytes cho từng input part
        for part in &body.input {
            let part_len = match part {
                ananke_protocol::InputPartDto::Text { text } => text.len(),
            };
            if part_len > limits.max_part_bytes as usize {
                state.hub.emit_resource_throttled("max_part_bytes".to_owned(), part_len as u32, "rejected".to_owned());
                let detail = format!(
                    "input part bytes ({}) vượt quá max_part_bytes ({})",
                    part_len, limits.max_part_bytes
                );
                let problem = ananke_protocol::ProblemDetails::new(
                    StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
                    "Payload Too Large",
                )
                .with_detail(detail)
                .with_code("limit_exceeded");
                return Err(ApiError::PayloadTooLarge(problem));
            }
        }

        // Validate max_context_tokens đối với context hiện tại của session
        let current_tokens = guard.estimated_context_tokens();
        if current_tokens > limits.max_context_tokens {
            state.hub.emit_resource_throttled("max_context_tokens".to_owned(), current_tokens, "rejected".to_owned());
            let detail = format!(
                "estimated context tokens ({}) vượt quá max_context_tokens ({})",
                current_tokens, limits.max_context_tokens
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
    let running_tasks = Arc::clone(&state.running_tasks);
    // Increment active turns counter when turn starts
    state.sessions.increment_active_turns();
    let join_handle = tokio::spawn(async move {
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
        Ok(Some(turn_id)) => {
            let core_turn_id = TurnId::from(turn_id.clone());
            running_tasks.lock().await.insert(core_turn_id, join_handle);
            Ok((
                StatusCode::ACCEPTED,
                Json(TurnAcceptedResponse { turn_id }),
            ))
        }
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

    // Abort running task if active to release session lock and stop waiting
    let running_task = state.running_tasks.lock().await.remove(&turn_id);
    if let Some(handle) = running_task {
        handle.abort();
    }

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

    // Abort running task if active so steer can take over driving the turn
    let running_task = state.running_tasks.lock().await.remove(&turn_id);
    if let Some(handle) = running_task {
        handle.abort();
    }

    let handle = state
        .sessions
        .get_or_load(&state.store, &session_id)
        .await
        .map_err(map_store_error)?
        .ok_or_else(|| not_found(&session_id))?;

    let session = handle.lock().await;
    
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

    // Steer drive loop should run asynchronously in the background so the HTTP steer request returns 204 promptly!
    let engine = Arc::clone(&state.engine);
    let task_session = session_id.clone();
    let steer_turn_id = turn_id.clone();
    let sessions = state.sessions.clone();
    let running_tasks = Arc::clone(&state.running_tasks);
    
    // Drop session lock before spawning new driver task
    drop(session);

    let handle_clone = Arc::clone(&handle);
    let join_handle = tokio::spawn(async move {
        let session = handle_clone.lock().await;
        let mut session = session;
        let result = engine.steer_turn(&mut session, &steer_turn_id, input_messages).await;
        if let Err(err) = &result {
            tracing::error!(session = %task_session, error = %err, "steer turn kết thúc với lỗi");
        }
        drop(session);
        sessions.decrement_active_turns();
    });

    running_tasks.lock().await.insert(turn_id, join_handle);

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/session/{id}/turn/{turnID}/resume` — tiếp tục turn đang AwaitingTool (AD-17).
pub async fn resume_turn(
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

    let session = handle.lock().await;

    let can_resume = {
        let turn = session.turn(&turn_id).ok_or_else(|| not_found(&session_id))?;
        matches!(turn.status, TurnStatus::AwaitingTool)
    };

    if !can_resume {
        let detail = "turn không ở trạng thái AwaitingTool - action: resume, allowed_states: [awaiting_tool]".to_owned();
        let problem = ananke_protocol::ProblemDetails::new(
            StatusCode::BAD_REQUEST.as_u16(),
            "Bad Request",
        )
        .with_detail(detail)
        .with_code("bad_state");
        return Err(ApiError::BadRequest(problem));
    }

    let engine = Arc::clone(&state.engine);
    let task_session = session_id.clone();
    let resume_turn_id = turn_id.clone();
    let sessions = state.sessions.clone();
    let running_tasks = Arc::clone(&state.running_tasks);

    // Increment active turns for admission control tracking
    sessions.increment_active_turns();

    drop(session);

    let handle_clone = Arc::clone(&handle);
    let join_handle = tokio::spawn(async move {
        let session = handle_clone.lock().await;
        let mut session = session;
        let result = engine.resume_turn(&mut session, &resume_turn_id).await;
        if let Err(err) = &result {
            tracing::error!(session = %task_session, error = %err, "resume turn kết thúc với lỗi");
        }
        drop(session);
        sessions.decrement_active_turns();
    });

    running_tasks.lock().await.insert(turn_id, join_handle);

    Ok(StatusCode::NO_CONTENT)
}
