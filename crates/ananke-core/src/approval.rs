use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{PermissionId, SessionId, TurnId};

/// Policy mức session (§7.2, §7.3). Luật chi tiết theo pattern sẽ nằm ở tầng config.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
    /// Host tự enforce; engine không chặn.
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Ask,
    Deny,
}

impl PermissionMode {
    /// `mutating` do tool tự khai báo qua [`crate::tool::Tool::mutating`].
    pub fn verdict(&self, mutating: bool) -> Verdict {
        match (self, mutating) {
            (PermissionMode::ReadOnly, true) => Verdict::Deny,
            (PermissionMode::ReadOnly, false) => Verdict::Allow,
            (PermissionMode::WorkspaceWrite, true) => Verdict::Ask,
            (PermissionMode::WorkspaceWrite, false) => Verdict::Allow,
            (PermissionMode::DangerFullAccess | PermissionMode::External, _) => Verdict::Allow,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    AllowOnce,
    AllowAlways,
    Deny,
}

impl Decision {
    pub fn allows(&self) -> bool {
        !matches!(self, Decision::Deny)
    }
}

#[derive(Clone, Debug)]
pub struct PermissionRequest {
    pub id: PermissionId,
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub tool: String,
    pub arguments: Value,
}

/// AD-07: approval là event + POST, không cần kênh ngược.
///
/// Cài đặt thật sẽ phát `permission.requested`, chờ quyết định từ
/// `POST /api/session/{id}/permission/{permID}` rồi resolve future; core chỉ thấy
/// một `await` bình thường.
#[async_trait]
pub trait Approver: Send + Sync {
    async fn decide(&self, request: &PermissionRequest) -> Decision;
}

/// Dùng cho test và cho chế độ đã được host phê duyệt trước.
pub struct AllowAll;

#[async_trait]
impl Approver for AllowAll {
    async fn decide(&self, _request: &PermissionRequest) -> Decision {
        Decision::AllowOnce
    }
}