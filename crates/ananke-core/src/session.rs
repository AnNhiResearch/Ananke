use crate::approval::PermissionMode;
use crate::ids::{SessionId, TurnId};
use crate::message::Message;
use crate::turn::Turn;

/// Session là đơn vị lưu trữ và replay (AD-08, AD-09): mọi turn và message của
/// một cuộc hội thoại nằm trong đây, không có state ẩn ở client.
#[derive(Clone, Debug)]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    pub model: String,
    pub permission_mode: PermissionMode,
    messages: Vec<Message>,
    turns: Vec<Turn>,
}

impl Session {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            id: SessionId::new(),
            title: String::new(),
            model: model.into(),
            permission_mode: PermissionMode::WorkspaceWrite,
            messages: Vec::new(),
            turns: Vec::new(),
        }
    }

    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn push_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Pop the last message (used for steer_turn to replace assistant message).
    pub fn pop_message(&mut self) -> Option<Message> {
        self.messages.pop()
    }

    /// Get mutable reference to last message if it's an assistant message.
    pub fn last_message_mut(&mut self) -> Option<&mut Message> {
        self.messages.last_mut()
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    pub fn push_turn(&mut self, turn: Turn) {
        self.turns.push(turn);
    }

    pub fn turn(&self, id: &TurnId) -> Option<&Turn> {
        self.turns.iter().find(|turn| &turn.id == id)
    }

    pub fn turn_mut(&mut self, id: &TurnId) -> Option<&mut Turn> {
        self.turns.iter_mut().find(|turn| &turn.id == id)
    }

    /// Turn đang chờ tool dở dang; engine phải hoàn tất chúng khi khởi động lại (AD-17).
    pub fn turns_awaiting_tool(&self) -> impl Iterator<Item = &Turn> {
        self.turns
            .iter()
            .filter(|turn| matches!(turn.status, crate::turn::TurnStatus::AwaitingTool))
    }

    /// Ước lượng số token của context trong session (AD-15/AD-24).
    /// Heuristic chuẩn: 1 token ~ 4 ký tự cho văn bản text/reasoning/tool arguments.
    pub fn estimated_context_tokens(&self) -> u32 {
        let mut char_count = 0;
        for msg in &self.messages {
            for part in &msg.parts {
                match part {
                    crate::Part::Text { text, .. } => char_count += text.len(),
                    crate::Part::Reasoning { text, .. } => char_count += text.len(),
                    crate::Part::ToolCall { tool, arguments, .. } => {
                        char_count += tool.len() + arguments.to_string().len();
                    }
                    crate::Part::ToolResult { output, .. } => {
                        char_count += output.len();
                    }
                }
            }
        }
        (char_count.div_ceil(4)) as u32
    }
}