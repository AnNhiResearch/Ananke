use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{PartId, ToolCallId, TurnId};
use crate::part::Part;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub fn merge(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    /// Turn đã persist nhưng đang chờ tool dài; khởi động lại phải hoàn tất từ đây (AD-17).
    AwaitingTool,
    Completed,
    Interrupted,
    Failed { reason: String },
}

impl TurnStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TurnStatus::Completed | TurnStatus::Interrupted | TurnStatus::Failed { .. }
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Turn {
    pub id: TurnId,
    pub status: TurnStatus,
    pub parts: Vec<Part>,
    pub usage: Usage,
    /// Flag hủy turn; được set bởi Engine::cancel_turn.
    #[serde(skip)]
    pub cancelled: AtomicBool,
}

impl Turn {
    pub fn new() -> Self {
        Self {
            id: TurnId::new(),
            status: TurnStatus::Running,
            parts: Vec::new(),
            usage: Usage::default(),
            cancelled: AtomicBool::new(false),
        }
    }

    /// Kiểm tra xem turn đã bị hủy chưa.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Đánh dấu turn bị hủy.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn part(&self, id: &PartId) -> Option<&Part> {
        self.parts.iter().find(|part| part.id() == id)
    }

    /// Các lời gọi tool chưa có kết quả. Turn `AwaitingTool` khôi phục từ đây (AD-17).
    pub fn pending_tool_calls(&self) -> Vec<(&ToolCallId, &str, &Value)> {
        let resolved: HashSet<&ToolCallId> = self
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::ToolResult { call_id, .. } => Some(call_id),
                _ => None,
            })
            .collect();

        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::ToolCall {
                    call_id,
                    tool,
                    arguments,
                    ..
                } if !resolved.contains(call_id) => {
                    Some((call_id, tool.as_str(), arguments))
                }
                _ => None,
            })
            .collect()
    }
}

impl Default for Turn {
    fn default() -> Self {
        Self::new()
    }
}

/// Cần implement Clone thủ công vì AtomicBool không implement Clone.
impl Clone for Turn {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            status: self.status.clone(),
            parts: self.parts.clone(),
            usage: self.usage.clone(),
            cancelled: AtomicBool::new(self.cancelled.load(Ordering::Acquire)),
        }
    }
}