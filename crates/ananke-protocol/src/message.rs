//! Message — lịch sử hội thoại trả về từ `GET /api/session/{id}/message`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::part::PartDto;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleDto {
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageDto {
    pub id: String,
    pub role: RoleDto,
    pub parts: Vec<PartDto>,
}

#[cfg(test)]
mod tests {
    use super::{MessageDto, RoleDto};

    #[test]
    fn message_serialize_dung_role() {
        let message = MessageDto {
            id: "msg_1".to_owned(),
            role: RoleDto::Assistant,
            parts: vec![],
        };
        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value["role"], "assistant");
    }
}