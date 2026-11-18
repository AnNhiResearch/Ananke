use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::approval::Decision;
use crate::ids::{PartId, PermissionId, SessionId, TurnId};
use crate::part::{Part, PartKind};
use crate::turn::Usage;

/// Mốc thời gian dạng millis, đủ cho event log và không kéo thêm dependency.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Tên event trùng §6 của `ARCHITECTURE.md` để client và plugin dùng chung chuỗi.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventKind {
    #[serde(rename = "turn.started")]
    TurnStarted { turn_id: TurnId },
    #[serde(rename = "turn.awaiting_tool")]
    TurnAwaitingTool { turn_id: TurnId },
    #[serde(rename = "turn.completed")]
    TurnCompleted { turn_id: TurnId, usage: Usage },
    #[serde(rename = "turn.failed")]
    TurnFailed { turn_id: TurnId, reason: String },
    #[serde(rename = "turn.interrupted")]
    TurnInterrupted { turn_id: TurnId },
    #[serde(rename = "part.started")]
    PartStarted {
        turn_id: TurnId,
        part_id: PartId,
        kind: PartKind,
    },
    #[serde(rename = "part.delta")]
    PartDelta {
        turn_id: TurnId,
        part_id: PartId,
        kind: PartKind,
        delta: String,
    },
    #[serde(rename = "part.completed")]
    PartCompleted { turn_id: TurnId, part: Part },
    #[serde(rename = "permission.requested")]
    PermissionRequested {
        turn_id: TurnId,
        permission_id: PermissionId,
        tool: String,
        arguments: Value,
    },
    #[serde(rename = "permission.resolved")]
    PermissionResolved {
        turn_id: TurnId,
        permission_id: PermissionId,
        decision: Decision,
    },
    #[serde(rename = "resource.throttled")]
    ResourceThrottled {
        limit: String,
        current: u32,
        action: String,
    },
    #[serde(rename = "warning")]
    Warning { message: String },
}

impl EventKind {
    /// AD-08: `part.delta` chỉ broadcast cho subscriber đang sống, không ghi vào event log.
    pub fn is_persistent(&self) -> bool {
        !matches!(self, EventKind::PartDelta { .. })
    }
}

/// Event đã gắn `seq` toàn phần trong một session (AD-08).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub session_id: SessionId,
    pub at: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// Điểm phát event. Transport (SSE/stdio) cài đặt trait này; core không biết transport.
#[async_trait]
pub trait EventSink: Send + Sync {
    async fn emit(&self, event: Event);
}

/// Sink trong bộ nhớ, dùng cho test và cho client nối muộn.
#[derive(Default)]
pub struct MemoryEventSink {
    events: Mutex<Vec<Event>>,
}

impl MemoryEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn len(&self) -> usize {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl EventSink for MemoryEventSink {
    async fn emit(&self, event: Event) {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event);
    }
}