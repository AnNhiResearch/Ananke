//! CRUD session: tạo, liệt kê, chi tiết, cập nhật, xoá và snapshot trạng thái.
//!
//! Metadata bền vững nằm ở bảng `session`; registry trong RAM chỉ giữ state sống
//! của session đang chạy turn (AD-08, AD-15).

use ananke_core::event::now_millis;
use ananke_core::Session;
use ananke_protocol::{CreateSessionRequest, SessionDto, SessionStateDto, UpdateSessionRequest};
use ananke_store::SessionMeta;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::dto;
use crate::error::{ApiError, ApiResult};
use crate::routes::{internal, map_store_error, not_found, session_id};
use crate::state::AppState;

/// `POST /api/session` — tạo session mới và ghi row `session` đầy đủ.
///
/// `upsert_session_meta` đã ghi mọi cột (`workspace`, `worktree`, `created_at`),
/// nên không cần gọi thêm `save_session`; title mặc định là basename workspace.
pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> ApiResult<(StatusCode, Json<SessionDto>)> {
    // AD-15: Check admission control before creating session
    let limits = state.limits.read().await;
    let current_resident = state.sessions.resident_session_count().await;
    if current_resident >= limits.max_resident_sessions as usize {
        // Emit resource.throttled event via hub (will be implemented in hub.rs)
        state.hub.emit_resource_throttled("max_resident_sessions".to_owned(), current_resident as u32, "rejected".to_owned());
        
        let detail = format!(
            "max resident sessions exceeded: {}/{} (limit: max_resident_sessions, current: {}, action: rejected)",
            current_resident, limits.max_resident_sessions, current_resident
        );
        let problem = ananke_protocol::ProblemDetails::new(
            StatusCode::TOO_MANY_REQUESTS.as_u16(),
            "Too Many Requests",
        )
        .with_detail(detail)
        .with_code("resource_throttled");
        
        return Err(ApiError::TooManyRequests(problem));
    }
    drop(limits);

    let session = Session::new(body.model.clone());
    let title = title_from_workspace(&body.workspace, &session.title);
    let session = session.with_title(title.clone());

    let meta = SessionMeta {
        id: session.id.clone(),
        parent_id: None,
        workspace: body.workspace,
        worktree: body.worktree,
        title,
        model: body.model,
        created_at: now_millis(),
        archived_at: None,
    };
    state.store.upsert_session_meta(&meta).map_err(internal)?;
    state.sessions.insert(session.clone()).await;

    let dto = dto::session_dto(&meta, Some(&session));
    Ok((StatusCode::CREATED, Json(dto)))
}

/// `GET /api/session` — danh sách session chưa archive, mới nhất trước.
pub async fn list_sessions(State(state): State<AppState>) -> ApiResult<Json<Vec<SessionDto>>> {
    let metas = state.store.list_session_meta().map_err(internal)?;
    let mut sessions = Vec::with_capacity(metas.len());
    for meta in &metas {
        sessions.push(session_dto_with_live(&state, meta).await);
    }
    Ok(Json(sessions))
}

/// `GET /api/session/{id}` — 404 nếu metadata không tồn tại.
pub async fn get_session(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<SessionDto>> {
    let id = session_id(&raw_id);
    let meta = state
        .store
        .get_session_meta(&id)
        .map_err(internal)?
        .ok_or_else(|| not_found(&id))?;

    Ok(Json(session_dto_with_live(&state, &meta).await))
}

/// `PATCH /api/session/{id}` — chỉ `title`/`model`; `permissionMode` chưa hỗ trợ
/// theo từng session ở M0 nên chỉ log và bỏ qua.
pub async fn update_session(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Json(body): Json<UpdateSessionRequest>,
) -> ApiResult<Json<SessionDto>> {
    let id = session_id(&raw_id);
    let mut meta = state
        .store
        .get_session_meta(&id)
        .map_err(internal)?
        .ok_or_else(|| not_found(&id))?;

    if let Some(mode) = body.permission_mode {
        tracing::warn!(
            session = %id,
            ?mode,
            "permissionMode theo session chưa hỗ trợ ở M0, bỏ qua"
        );
    }

    let changed = state
        .store
        .update_session_meta(&id, body.title.as_deref(), body.model.as_deref())
        .map_err(internal)?;
    if !changed {
        return Err(not_found(&id));
    }

    if let Some(title) = &body.title {
        meta.title = title.clone();
    }
    if let Some(model) = &body.model {
        meta.model = model.clone();
    }

    // Session đang chạy turn giữ lock cả lượt: không chặn request, chỉ cập nhật
// RAM khi rảnh vì meta trong DB đã là nguồn đúng cho client.
let dto = match state.sessions.get(&id).await {
    Some(handle) => match handle.try_lock() {
        Ok(mut live) => {
            if let Some(title) = &body.title {
                live.title = title.clone();
            }
            if let Some(model) = &body.model {
                live.model = model.clone();
            }
            dto::session_dto(&meta, Some(&live))
        }
        Err(_) => dto::session_dto(&meta, None),
    },
    None => dto::session_dto(&meta, None),
};

    Ok(Json(dto))
}

/// `DELETE /api/session/{id}` — xoá row cùng dữ liệu con, bỏ khỏi RAM.
pub async fn delete_session(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = session_id(&raw_id);
    let deleted = state.store.delete_session(&id).map_err(internal)?;
    if !deleted {
        return Err(not_found(&id));
    }
    state.sessions.remove(&id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/session/{id}/state` — snapshot để client resync (AD-08).
pub async fn get_session_state(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<SessionStateDto>> {
    let id = session_id(&raw_id);
    let meta = state
        .store
        .get_session_meta(&id)
        .map_err(internal)?
        .ok_or_else(|| not_found(&id))?;

    let handle = state
        .sessions
        .get_or_load(&state.store, &id)
        .await
        .map_err(map_store_error)?
        .ok_or_else(|| not_found(&id))?;

    let last_seq = state.store.max_event_seq(&id).map_err(internal)?;
    let live = handle.lock().await;
    Ok(Json(dto::session_state_dto(&meta, &live, last_seq)))
}

/// Ghép metadata với session trong RAM nếu có. `try_lock` để danh sách không bị
/// treo khi một session đang chạy turn (khi đó dùng giá trị trong DB).
async fn session_dto_with_live(state: &AppState, meta: &SessionMeta) -> SessionDto {
    match state.sessions.get(&meta.id).await {
        Some(handle) => match handle.try_lock() {
            Ok(live) => dto::session_dto(meta, Some(&live)),
            Err(_) => dto::session_dto(meta, None),
        },
        None => dto::session_dto(meta, None),
    }
}

/// Basename của workspace làm title mặc định; workspace rỗng thì giữ title core.
fn title_from_workspace(workspace: &str, fallback: &str) -> String {
    std::path::Path::new(workspace)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| fallback.to_owned(), str::to_owned)
}