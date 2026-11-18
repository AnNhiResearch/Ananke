//! Session và snapshot trạng thái (§5.2, AD-08).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::message::MessageDto;
use crate::permission::PermissionModeDto;
use crate::turn::TurnDto;

/// Body của `POST /api/session`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub workspace: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
}

/// Body của `PATCH /api/session/{id}`; field vắng mặt nghĩa là giữ nguyên.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionModeDto>,
}

/// Session trên dây. `parentID` chỉ có với sub-agent (AD-16).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    pub title: String,
    pub model: String,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<u64>,
}

/// Snapshot để resync (`GET /api/session/{id}/state`, AD-08).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateDto {
    pub session: SessionDto,
    pub turns: Vec<TurnDto>,
    pub messages: Vec<MessageDto>,
    /// `seq` cuối cùng đã gộp; client stream tiếp từ `since = lastSeq`.
    pub last_seq: u64,
}

#[cfg(test)]
mod tests {
    use super::{CreateSessionRequest, SessionDto};

    #[test]
    fn create_session_theo_muc_5_2() {
        let request = CreateSessionRequest {
            workspace: "d:/project/Ananke".to_owned(),
            model: "default".to_owned(),
            worktree: None,
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["workspace"], "d:/project/Ananke");
        assert_eq!(value["model"], "default");
        assert!(value.get("worktree").is_none());
    }

    #[test]
    fn session_parent_id_hoa_va_camel_case() {
        let session = SessionDto {
            id: "ses_1".to_owned(),
            parent_id: Some("ses_0".to_owned()),
            workspace: ".".to_owned(),
            worktree: None,
            title: "Thử".to_owned(),
            model: "default".to_owned(),
            created_at: 1_700_000_000_000,
            archived_at: None,
        };
        let value = serde_json::to_value(&session).unwrap();
        assert_eq!(value["parentID"], "ses_0");
        assert_eq!(value["createdAt"], 1_700_000_000_000u64);
        assert!(value.get("archivedAt").is_none());
    }
}