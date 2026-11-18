use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{PartId, ToolCallId};

/// Loại part, dùng cho event `part.started` / `part.delta` (client render theo kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Text,
    Reasoning,
    ToolCall,
    ToolResult,
}

/// Đơn vị nội dung nhỏ nhất mà model nhìn thấy và client render được (AD-06).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        id: PartId,
        text: String,
    },
    Reasoning {
        id: PartId,
        text: String,
    },
    ToolCall {
        id: PartId,
        call_id: ToolCallId,
        tool: String,
        arguments: Value,
    },
    ToolResult {
        id: PartId,
        call_id: ToolCallId,
        tool: String,
        ok: bool,
        output: String,
    },
}

impl Part {
    pub fn id(&self) -> &PartId {
        match self {
            Part::Text { id, .. }
            | Part::Reasoning { id, .. }
            | Part::ToolCall { id, .. }
            | Part::ToolResult { id, .. } => id,
        }
    }

    pub fn kind(&self) -> PartKind {
        match self {
            Part::Text { .. } => PartKind::Text,
            Part::Reasoning { .. } => PartKind::Reasoning,
            Part::ToolCall { .. } => PartKind::ToolCall,
            Part::ToolResult { .. } => PartKind::ToolResult,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Part::Text {
            id: PartId::new(),
            text: text.into(),
        }
    }

    pub fn reasoning(text: impl Into<String>) -> Self {
        Part::Reasoning {
            id: PartId::new(),
            text: text.into(),
        }
    }

    pub fn tool_call(call_id: ToolCallId, tool: impl Into<String>, arguments: Value) -> Self {
        Part::ToolCall {
            id: PartId::new(),
            call_id,
            tool: tool.into(),
            arguments,
        }
    }

    pub fn tool_result(
        call_id: ToolCallId,
        tool: impl Into<String>,
        ok: bool,
        output: impl Into<String>,
    ) -> Self {
        Part::ToolResult {
            id: PartId::new(),
            call_id,
            tool: tool.into(),
            ok,
            output: output.into(),
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Part::Text { text, .. } | Part::Reasoning { text, .. } => Some(text),
            _ => None,
        }
    }
}