//! Approval và policy quyền (AD-07, §7.3).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Policy hiệu lực của session; giá trị trùng `PermissionMode` của core.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionModeDto {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
    /// Host tự enforce; engine không chặn.
    External,
}

/// Quyết định trả lời `permission.requested` — đúng ba giá trị AD-07.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecisionDto {
    Allow,
    Deny,
    AllowAlways,
}

/// Phạm vi áp dụng của quyết định (AD-07).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionScopeDto {
    Once,
    Session,
    Project,
}

/// Body của `POST /api/session/{id}/permission/{permID}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDecisionRequest {
    pub decision: PermissionDecisionDto,
    /// Không cần khi `decision = deny`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<PermissionScopeDto>,
}

#[cfg(test)]
mod tests {
    use super::{PermissionDecisionDto, PermissionDecisionRequest, PermissionScopeDto};

    #[test]
    fn quyet_dinh_dung_ad_07() {
        assert_eq!(
            serde_json::to_string(&PermissionDecisionDto::Allow).unwrap(),
            "\"allow\""
        );
        assert_eq!(
            serde_json::to_string(&PermissionDecisionDto::Deny).unwrap(),
            "\"deny\""
        );
        assert_eq!(
            serde_json::to_string(&PermissionDecisionDto::AllowAlways).unwrap(),
            "\"allow_always\""
        );
        assert_eq!(
            serde_json::to_string(&PermissionScopeDto::Project).unwrap(),
            "\"project\""
        );
    }

    #[test]
    fn body_allow_co_scope_deny_khong_scope() {
        let allow = PermissionDecisionRequest {
            decision: PermissionDecisionDto::Allow,
            scope: Some(PermissionScopeDto::Once),
        };
        let value = serde_json::to_value(allow).unwrap();
        assert_eq!(value["decision"], "allow");
        assert_eq!(value["scope"], "once");

        let deny = PermissionDecisionRequest {
            decision: PermissionDecisionDto::Deny,
            scope: None,
        };
        let value = serde_json::to_value(deny).unwrap();
        assert!(value.get("scope").is_none());
    }
}