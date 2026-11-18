//! Turn — một lượt chạy của agent (AD-06, AD-17).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::part::PartDto;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageDto {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TurnStatusDto {
    Running,
    /// Đã persist nhưng đang chờ tool dài; restart phải hoàn tất từ đây (AD-17).
    AwaitingTool,
    Completed,
    Interrupted,
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TurnDto {
    pub id: String,
    pub status: TurnStatusDto,
    pub parts: Vec<PartDto>,
    pub usage: UsageDto,
}

/// Một phần input client gửi lên; client không sinh `id` — server cấp.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputPartDto {
    Text { text: String },
}

/// Body của `POST /api/session/{id}/turn`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateTurnRequest {
    pub input: Vec<InputPartDto>,
}

/// Body của `POST /api/session/{id}/turn/{turnID}/steer`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SteerRequest {
    pub input: Vec<InputPartDto>,
}

/// Response `202` của `POST /api/session/{id}/turn` (§5.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TurnAcceptedResponse {
    #[serde(rename = "turnID")]
    pub turn_id: String,
}

#[cfg(test)]
mod tests {
    use super::{CreateTurnRequest, InputPartDto, TurnAcceptedResponse, TurnStatusDto};

    #[test]
    fn input_part_theo_vi_du_phu_luc_a() {
        let request = CreateTurnRequest {
            input: vec![InputPartDto::Text {
                text: "xin chào".to_owned(),
            }],
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["input"][0]["type"], "text");
        assert_eq!(value["input"][0]["text"], "xin chào");
        assert!(value["input"][0].get("id").is_none());
    }

    #[test]
    fn turn_accepted_dung_turn_id_hoa() {
        let response = TurnAcceptedResponse {
            turn_id: "trn_1".to_owned(),
        };
        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["turnID"], "trn_1");
    }

    #[test]
    fn turn_failed_mang_reason() {
        let status = TurnStatusDto::Failed {
            reason: "provider 429".to_owned(),
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["status"], "failed");
        assert_eq!(value["reason"], "provider 429");
    }
}