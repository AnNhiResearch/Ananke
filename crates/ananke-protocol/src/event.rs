//! Event trên dây (§6, AD-08).
//!
//! Envelope phẳng: `seq`, `sessionID`, `at` và các field của payload nằm cùng
//! một object JSON, khớp ví dụ frame SSE trong §6. SSE frame còn đặt `seq` vào
//! dòng `id:` để client replay bằng `Last-Event-ID`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::part::{PartDto, PartKindDto};
use crate::permission::{PermissionDecisionDto, PermissionScopeDto};
use crate::turn::UsageDto;

/// Toàn bộ tên event của §6, đúng thứ tự trong tài liệu.
///
/// Client gặp tên ngoài danh sách này (hoặc chưa biết) thì **bỏ qua**, không lỗi.
pub const EVENT_TYPES: &[&str] = &[
    "service.ready",
    "service.health",
    "service.stopping",
    "session.created",
    "session.updated",
    "session.archived",
    "session.unloaded",
    "session.compacted",
    "turn.started",
    "turn.completed",
    "turn.failed",
    "turn.interrupted",
    "turn.usage",
    "turn.awaiting_tool",
    "part.started",
    "part.delta",
    "part.completed",
    "permission.requested",
    "permission.resolved",
    "todo.updated",
    "plan.updated",
    "file.changed",
    "lsp.diagnostic",
    "mcp.status_changed",
    "model.catalog_updated",
    "subagent.queued",
    "subagent.started",
    "subagent.completed",
    "plugin.started",
    "plugin.stopped",
    "plugin.disabled",
    "plugin.error",
    "resource.throttled",
    "event.resync_required",
    "warning",
    "log",
];

/// Hành động khi resource bị throttle (AD-15).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThrottleAction {
    Queued,
    Rejected,
}

/// Payload của event, gắn loại bằng field `type` (§6).
///
/// Hiện chỉ khai báo các loại engine phát ra; loại khác rơi vào [`Self::Unknown`]
/// để client forward-compatible và vẫn đọc được `seq` của envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type")]
pub enum EventPayloadDto {
    #[serde(rename = "turn.started")]
    TurnStarted {
        #[serde(rename = "turnID")]
        turn_id: String,
    },
    #[serde(rename = "turn.awaiting_tool")]
    TurnAwaitingTool {
        #[serde(rename = "turnID")]
        turn_id: String,
    },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        #[serde(rename = "turnID")]
        turn_id: String,
        usage: UsageDto,
    },
    #[serde(rename = "turn.failed")]
    TurnFailed {
        #[serde(rename = "turnID")]
        turn_id: String,
        reason: String,
    },
    #[serde(rename = "part.started")]
    PartStarted {
        #[serde(rename = "turnID")]
        turn_id: String,
        #[serde(rename = "partID")]
        part_id: String,
        kind: PartKindDto,
    },
    /// Batch 16–33 ms và **không** persist (AD-08); client không được giả định
    /// mỗi token là một frame.
    #[serde(rename = "part.delta")]
    PartDelta {
        #[serde(rename = "turnID")]
        turn_id: String,
        #[serde(rename = "partID")]
        part_id: String,
        kind: PartKindDto,
        delta: String,
    },
    #[serde(rename = "part.completed")]
    PartCompleted {
        #[serde(rename = "turnID")]
        turn_id: String,
        part: PartDto,
    },
    #[serde(rename = "permission.requested")]
    PermissionRequested {
        #[serde(rename = "turnID")]
        turn_id: String,
        #[serde(rename = "permissionID")]
        permission_id: String,
        tool: String,
        #[schema(value_type = Object)]
        arguments: Value,
    },
    #[serde(rename = "permission.resolved")]
    PermissionResolved {
        #[serde(rename = "turnID")]
        turn_id: String,
        #[serde(rename = "permissionID")]
        permission_id: String,
        decision: PermissionDecisionDto,
        #[serde(skip_serializing_if = "Option::is_none")]
        scope: Option<PermissionScopeDto>,
    },
    #[serde(rename = "warning")]
    Warning { message: String },
    /// Turn bị ngắt bởi user/system (M1).
    #[serde(rename = "turn.interrupted")]
    TurnInterrupted {
        #[serde(rename = "turnID")]
        turn_id: String,
    },
    /// Resource limit exceeded (AD-15).
    #[serde(rename = "resource.throttled")]
    ResourceThrottled {
        limit: String,
        current: u32,
        action: ThrottleAction,
    },
    /// Client nên resync từ server state (SSE race recovery).
    #[serde(rename = "event.resync_required")]
    EventResyncRequired {
        #[serde(rename = "lastServerSeq")]
        last_server_seq: u64,
        reason: String,
    },
    /// Loại chưa biết: giữ envelope, payload bị bỏ (§6).
    #[serde(other)]
    Unknown,
}

/// Event đã gắn `seq` toàn phần trong một session (AD-08).
///
/// `seq` tăng đơn điệu trong một session; giữa các session không đảm bảo (§6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventDto {
    /// Dùng cho `?since=` và `Last-Event-ID` khi replay (AD-08).
    pub seq: u64,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Mili-giây Unix.
    pub at: u64,
    #[serde(flatten)]
    pub payload: EventPayloadDto,
}

#[cfg(test)]
mod tests {
    use super::{EventDto, EventPayloadDto, EVENT_TYPES};
    use crate::part::PartKindDto;

    #[test]
    fn danh_sach_event_khop_muc_6_va_khong_trung() {
        assert_eq!(EVENT_TYPES.len(), 36);
        let mut sorted = EVENT_TYPES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), EVENT_TYPES.len(), "có tên event trùng");
        assert!(EVENT_TYPES.contains(&"part.delta"));
        assert!(EVENT_TYPES.contains(&"event.resync_required"));
    }

    #[test]
    fn part_delta_phang_dung_vi_du_sse() {
        let event = EventDto {
            seq: 1043,
            session_id: "ses_01H".to_owned(),
            at: 1_700_000_000_000,
            payload: EventPayloadDto::PartDelta {
                turn_id: "trn_01H".to_owned(),
                part_id: "prt_01H".to_owned(),
                kind: PartKindDto::Text,
                delta: "Đang chạy ".to_owned(),
            },
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["seq"], 1043);
        assert_eq!(value["sessionID"], "ses_01H");
        assert_eq!(value["type"], "part.delta");
        assert_eq!(value["turnID"], "trn_01H");
        assert_eq!(value["partID"], "prt_01H");
        assert_eq!(value["kind"], "text");
        assert_eq!(value["delta"], "Đang chạy ");

        let back: EventDto = serde_json::from_value(value).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn loai_la_van_giu_seq_va_envelope() {
        let value = serde_json::json!({
            "seq": 7,
            "sessionID": "ses_1",
            "at": 1_700_000_000_000u64,
            "type": "session.created",
            "title": "Mới"
        });
        let event: EventDto = serde_json::from_value(value).unwrap();
        assert_eq!(event.seq, 7);
        assert_eq!(event.session_id, "ses_1");
        assert_eq!(event.payload, EventPayloadDto::Unknown);
    }

    #[test]
    fn permission_resolved_bo_scope_khi_vang() {
        let event = EventDto {
            seq: 9,
            session_id: "ses_1".to_owned(),
            at: 1,
            payload: EventPayloadDto::PermissionResolved {
                turn_id: "trn_1".to_owned(),
                permission_id: "prm_1".to_owned(),
                decision: crate::permission::PermissionDecisionDto::Deny,
                scope: None,
            },
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["permissionID"], "prm_1");
        assert_eq!(value["decision"], "deny");
        assert!(value.get("scope").is_none());
    }
}