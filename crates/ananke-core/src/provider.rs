use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use serde_json::Value;

use crate::ids::ToolCallId;
use crate::message::Message;
use crate::turn::Usage;

/// Mô tả tool gửi cho model. Provider adapter map sang định dạng của từng hãng.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
}

/// Một sự kiện trên stream của model. `Failed` là terminal.
#[derive(Clone, Debug)]
pub enum ProviderEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCall {
        call_id: ToolCallId,
        tool: String,
        arguments: Value,
    },
    Done {
        usage: Usage,
    },
    Failed {
        reason: String,
    },
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = ProviderEvent> + Send>>;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider error: {0}")]
    Failed(String),
}

/// Model provider chỉ lo stream token; retry, fallback, compaction nằm ở tầng khác.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn stream(&self, request: ProviderRequest) -> Result<ProviderStream, ProviderError>;
}