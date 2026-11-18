//! `/api/meta` — thông tin phiên bản và năng lực thật của máy (§5.1).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Profile thiết bị theo RAM (AD-15); mirror của `ananke_sandbox::DeviceProfile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeviceProfileDto {
    /// `< 1.5 GiB`: tắt parallel subagent, LSP nền, watch file, TUI full.
    Constrained,
    /// `< 4 GiB`.
    Small,
    /// Còn lại.
    Default,
}

/// Cơ chế sandbox cấp OS thực tế đo được (§7.2); mirror của `SandboxKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxKindDto {
    Landlock,
    Seatbelt,
    RestrictedToken,
    /// Không enforce được — permission rules là hàng rào duy nhất.
    None,
}

/// Response của `GET /api/meta`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetaResponse {
    /// Client phải kiểm tra trước khi dùng API; khác major thì từ chối (§12).
    pub protocol_version: u32,
    /// Tính năng server hỗ trợ; client bỏ qua tên lạ và degrade (§12).
    pub capabilities: Vec<String>,
    pub server_version: String,
    pub device_profile: DeviceProfileDto,
    /// Giá trị đo lúc start, không phải lời hứa cấu hình (§7.2).
    pub sandbox: SandboxKindDto,
}

#[cfg(test)]
mod tests {
    use super::{DeviceProfileDto, MetaResponse, SandboxKindDto};

    #[test]
    fn sandbox_du_bon_gia_tri_kebab_case() {
        let cases = [
            (SandboxKindDto::Landlock, "\"landlock\""),
            (SandboxKindDto::Seatbelt, "\"seatbelt\""),
            (SandboxKindDto::RestrictedToken, "\"restricted-token\""),
            (SandboxKindDto::None, "\"none\""),
        ];
        for (kind, expected) in cases {
            assert_eq!(serde_json::to_string(&kind).unwrap(), expected);
        }
    }

    #[test]
    fn device_profile_du_ba_gia_tri() {
        assert_eq!(
            serde_json::to_string(&DeviceProfileDto::Constrained).unwrap(),
            "\"constrained\""
        );
        assert_eq!(
            serde_json::to_string(&DeviceProfileDto::Small).unwrap(),
            "\"small\""
        );
        assert_eq!(
            serde_json::to_string(&DeviceProfileDto::Default).unwrap(),
            "\"default\""
        );
    }

    #[test]
    fn meta_dung_camel_case_va_hau_to_id_hoa() {
        let meta = MetaResponse {
            protocol_version: 1,
            capabilities: vec!["sse".to_owned()],
            server_version: "0.1.0".to_owned(),
            device_profile: DeviceProfileDto::Constrained,
            sandbox: SandboxKindDto::None,
        };
        let value = serde_json::to_value(&meta).unwrap();
        assert_eq!(value["protocolVersion"], 1);
        assert_eq!(value["serverVersion"], "0.1.0");
        assert_eq!(value["deviceProfile"], "constrained");
        assert_eq!(value["sandbox"], "none");
        assert_eq!(value["capabilities"][0], "sse");
    }
}