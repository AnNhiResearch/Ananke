//! Mapping thuần từ type nội bộ (core/sandbox/store) sang DTO trên dây (AD-03).
//!
//! Không I/O, không giữ state: mọi hàm ở đây chỉ chuyển đổi dữ liệu để handler
//! HTTP và SSE dùng lại, tránh lặp logic ở từng route.

use ananke_core::{
    Decision, Event, EventKind, Message, Part, PartKind, Role, Session, Turn, TurnStatus, Usage,
};
use ananke_protocol as protocol;
use ananke_store::SessionMeta;

/// Năng lực client được thông báo ở `GET /api/meta`.
const CAPABILITIES: [&str; 4] = ["sessions", "turns", "sse", "permissions"];

/// `SandboxKind` (sandbox) -> `SandboxKindDto` (protocol). Match tay vì protocol
/// không được phép phụ thuộc `ananke-sandbox` (AD-05).
pub fn sandbox_kind_dto(kind: ananke_sandbox::SandboxKind) -> protocol::SandboxKindDto {
    use ananke_sandbox::SandboxKind;
    match kind {
        SandboxKind::Landlock => protocol::SandboxKindDto::Landlock,
        SandboxKind::Seatbelt => protocol::SandboxKindDto::Seatbelt,
        SandboxKind::RestrictedToken => protocol::SandboxKindDto::RestrictedToken,
        SandboxKind::None => protocol::SandboxKindDto::None,
    }
}

/// `DeviceProfile` (sandbox) -> `DeviceProfileDto` (protocol).
pub fn device_profile_dto(p: ananke_sandbox::DeviceProfile) -> protocol::DeviceProfileDto {
    use ananke_sandbox::DeviceProfile;
    match p {
        DeviceProfile::Constrained => protocol::DeviceProfileDto::Constrained,
        DeviceProfile::Small => protocol::DeviceProfileDto::Small,
        DeviceProfile::Default => protocol::DeviceProfileDto::Default,
    }
}

/// Meta đầy đủ: phiên bản protocol/server và năng lực **đo được** lúc start.
pub fn meta_response() -> protocol::MetaResponse {
    protocol::MetaResponse {
        protocol_version: protocol::PROTOCOL_VERSION,
        capabilities: CAPABILITIES.iter().map(|name| (*name).to_owned()).collect(),
        server_version: env!("CARGO_PKG_VERSION").to_owned(),
        device_profile: device_profile_dto(ananke_sandbox::device_profile()),
        sandbox: sandbox_kind_dto(ananke_sandbox::detect().kind()),
    }
}

/// `Role` (core) -> `RoleDto` (protocol).
pub fn role_dto(role: Role) -> protocol::RoleDto {
    match role {
        Role::User => protocol::RoleDto::User,
        Role::Assistant => protocol::RoleDto::Assistant,
        Role::Tool => protocol::RoleDto::Tool,
    }
}

/// `Usage` (core) -> `UsageDto` (protocol).
pub fn usage_dto(u: &Usage) -> protocol::UsageDto {
    protocol::UsageDto {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
    }
}

/// `PartKind` (core) -> `PartKindDto` (protocol).
pub fn part_kind_dto(kind: PartKind) -> protocol::PartKindDto {
    match kind {
        PartKind::Text => protocol::PartKindDto::Text,
        PartKind::Reasoning => protocol::PartKindDto::Reasoning,
        PartKind::ToolCall => protocol::PartKindDto::ToolCall,
        PartKind::ToolResult => protocol::PartKindDto::ToolResult,
    }
}

/// `Part` (core) -> `PartDto` (protocol).
pub fn part_dto(part: &Part) -> protocol::PartDto {
    match part {
        Part::Text { id, text } => protocol::PartDto::Text {
            id: id.to_string(),
            text: text.clone(),
        },
        Part::Reasoning { id, text } => protocol::PartDto::Reasoning {
            id: id.to_string(),
            text: text.clone(),
        },
        Part::ToolCall {
            id,
            call_id,
            tool,
            arguments,
        } => protocol::PartDto::ToolCall {
            id: id.to_string(),
            call_id: call_id.to_string(),
            tool: tool.clone(),
            arguments: arguments.clone(),
        },
        Part::ToolResult {
            id,
            call_id,
            tool,
            ok,
            output,
        } => protocol::PartDto::ToolResult {
            id: id.to_string(),
            call_id: call_id.to_string(),
            tool: tool.clone(),
            ok: *ok,
            output: output.clone(),
        },
    }
}

/// `Message` (core) -> `MessageDto` (protocol).
pub fn message_dto(msg: &Message) -> protocol::MessageDto {
    protocol::MessageDto {
        id: msg.id.to_string(),
        role: role_dto(msg.role),
        parts: msg.parts.iter().map(part_dto).collect(),
    }
}

/// `TurnStatus` (core) -> `TurnStatusDto` (protocol).
pub fn turn_status_dto(status: &TurnStatus) -> protocol::TurnStatusDto {
    match status {
        TurnStatus::Running => protocol::TurnStatusDto::Running,
        TurnStatus::AwaitingTool => protocol::TurnStatusDto::AwaitingTool,
        TurnStatus::Completed => protocol::TurnStatusDto::Completed,
        TurnStatus::Interrupted => protocol::TurnStatusDto::Interrupted,
        TurnStatus::Failed { reason } => protocol::TurnStatusDto::Failed {
            reason: reason.clone(),
        },
    }
}

/// `Turn` (core) -> `TurnDto` (protocol).
///
/// `TurnDto.status` serialize thành object lồng (`{"status":{"status":"..."}}`)
/// theo đúng định nghĩa protocol; không flatten ở đây.
pub fn turn_dto(turn: &Turn) -> protocol::TurnDto {
    protocol::TurnDto {
        id: turn.id.to_string(),
        status: turn_status_dto(&turn.status),
        parts: turn.parts.iter().map(part_dto).collect(),
        usage: usage_dto(&turn.usage),
    }
}

/// Gộp metadata bền vững với state đang sống trong RAM.
///
/// `title`/`model` ưu tiên `live` vì đó là giá trị mới nhất; các field còn lại
/// chỉ có trong [`SessionMeta`].
pub fn session_dto(meta: &SessionMeta, live: Option<&Session>) -> protocol::SessionDto {
    protocol::SessionDto {
        id: meta.id.to_string(),
        parent_id: meta.parent_id.as_ref().map(|parent| parent.to_string()),
        workspace: meta.workspace.clone(),
        worktree: meta.worktree.clone(),
        title: live.map_or_else(|| meta.title.clone(), |session| session.title.clone()),
        model: live.map_or_else(|| meta.model.clone(), |session| session.model.clone()),
        created_at: meta.created_at,
        archived_at: meta.archived_at,
    }
}

/// Snapshot để client resync (`GET /api/session/{id}/state`, AD-08).
///
/// `turns`/`messages` lấy từ `live`; `last_seq` là `seq` lớn nhất đã persist, và
/// cũng là mốc client dùng cho `?since=`.
pub fn session_state_dto(
    meta: &SessionMeta,
    live: &Session,
    last_seq: u64,
) -> protocol::SessionStateDto {
    protocol::SessionStateDto {
        session: session_dto(meta, Some(live)),
        turns: live.turns().iter().map(turn_dto).collect(),
        messages: live.messages().iter().map(message_dto).collect(),
        last_seq,
    }
}

/// Map core [`Event`] -> `EventDto`; `seq` là seq đã gán cho event.
///
/// Core hiện chỉ phát đúng các loại `EventPayloadDto` hỗ trợ, nên hàm luôn trả
/// `Some`. Vẫn giữ `Option` để handler tương thích nếu core thêm loại event
/// chưa có DTO: khi đó client lấy phần còn lại qua `/state`.
pub fn event_to_dto(event: &Event, seq: u64) -> Option<protocol::EventDto> {
    let payload = match &event.kind {
        EventKind::TurnStarted { turn_id } => protocol::event::EventPayloadDto::TurnStarted {
            turn_id: turn_id.to_string(),
        },
        EventKind::TurnAwaitingTool { turn_id } => {
            protocol::event::EventPayloadDto::TurnAwaitingTool {
                turn_id: turn_id.to_string(),
            }
        }
        EventKind::TurnCompleted { turn_id, usage } => {
            protocol::event::EventPayloadDto::TurnCompleted {
                turn_id: turn_id.to_string(),
                usage: usage_dto(usage),
            }
        }
        EventKind::TurnFailed { turn_id, reason } => {
            protocol::event::EventPayloadDto::TurnFailed {
                turn_id: turn_id.to_string(),
                reason: reason.clone(),
            }
        }
        EventKind::TurnInterrupted { turn_id } => {
            protocol::event::EventPayloadDto::TurnInterrupted {
                turn_id: turn_id.to_string(),
            }
        }
        EventKind::PartStarted {
            turn_id,
            part_id,
            kind,
        } => protocol::event::EventPayloadDto::PartStarted {
            turn_id: turn_id.to_string(),
            part_id: part_id.to_string(),
            kind: part_kind_dto(*kind),
        },
        EventKind::PartDelta {
            turn_id,
            part_id,
            kind,
            delta,
        } => protocol::event::EventPayloadDto::PartDelta {
            turn_id: turn_id.to_string(),
            part_id: part_id.to_string(),
            kind: part_kind_dto(*kind),
            delta: delta.clone(),
        },
        EventKind::PartCompleted { turn_id, part } => {
            protocol::event::EventPayloadDto::PartCompleted {
                turn_id: turn_id.to_string(),
                part: part_dto(part),
            }
        }
        EventKind::PermissionRequested {
            turn_id,
            permission_id,
            tool,
            arguments,
        } => protocol::event::EventPayloadDto::PermissionRequested {
            turn_id: turn_id.to_string(),
            permission_id: permission_id.to_string(),
            tool: tool.clone(),
            arguments: arguments.clone(),
        },
        EventKind::PermissionResolved {
            turn_id,
            permission_id,
            decision,
        } => protocol::event::EventPayloadDto::PermissionResolved {
            turn_id: turn_id.to_string(),
            permission_id: permission_id.to_string(),
            decision: permission_decision_dto(*decision),
            scope: None,
        },
        EventKind::Warning { message } => protocol::event::EventPayloadDto::Warning {
            message: message.clone(),
        },
        EventKind::ResourceThrottled { limit, current, action } => {
            protocol::event::EventPayloadDto::ResourceThrottled {
                limit: limit.clone(),
                current: *current,
                action: match action.as_str() {
                    "rejected" => protocol::event::ThrottleAction::Rejected,
                    _ => protocol::event::ThrottleAction::Queued,
                },
            }
        }
    };

    Some(protocol::EventDto {
        seq,
        session_id: event.session_id.to_string(),
        at: event.at,
        payload,
    })
}

/// `Decision` (core) -> `PermissionDecisionDto` (protocol).
///
/// Core gộp `allow` + phạm vi vào `AllowOnce`/`AllowAlways`; `scope` để `None`
/// vì core không mang thông tin phạm vi.
fn permission_decision_dto(decision: Decision) -> protocol::PermissionDecisionDto {
    match decision {
        Decision::AllowOnce => protocol::PermissionDecisionDto::Allow,
        Decision::AllowAlways => protocol::PermissionDecisionDto::AllowAlways,
        Decision::Deny => protocol::PermissionDecisionDto::Deny,
    }
}

pub fn permission_mode_dto_to_core(mode: protocol::PermissionModeDto) -> ananke_core::PermissionMode {
    match mode {
        protocol::PermissionModeDto::ReadOnly => ananke_core::PermissionMode::ReadOnly,
        protocol::PermissionModeDto::WorkspaceWrite => ananke_core::PermissionMode::WorkspaceWrite,
        protocol::PermissionModeDto::DangerFullAccess => ananke_core::PermissionMode::DangerFullAccess,
        protocol::PermissionModeDto::External => ananke_core::PermissionMode::External,
    }
}

/// Chuyển input của client thành parts core. Hiện chỉ hỗ trợ `Text`; server cấp
/// `id` qua generator của core (`PartId::new()`).
pub fn input_parts(input: &[protocol::InputPartDto]) -> Vec<Part> {
    input
        .iter()
        .map(|part| match part {
            protocol::InputPartDto::Text { text } => Part::text(text.clone()),
        })
        .collect()
}