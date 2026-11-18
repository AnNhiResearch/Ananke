//! RFC 9457 Problem Details — định dạng lỗi duy nhất của API (AD-03).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Content type của lỗi; **không** dùng `application/json`.
pub const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";

/// Mã lỗi máy đọc được đặt trong field mở rộng `code` (AD-15).
pub const CODE_LIMIT_EXCEEDED: &str = "limit_exceeded";

/// Không có TUI nào attach khi gọi `/api/tui/*` (§5.5).
pub const CODE_TUI_NOT_ATTACHED: &str = "tui_not_attached";

/// Lỗi chuẩn hoá theo RFC 9457.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProblemDetails {
    /// URI nhận dạng loại lỗi; mặc định `about:blank`.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// Mô tả ngắn, ổn định theo loại lỗi.
    pub title: String,
    /// Mã HTTP lặp lại trong body để proxy/log không phải đoán.
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Mã ổn định cho client, ví dụ [`CODE_LIMIT_EXCEEDED`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ProblemDetails {
    /// Lỗi với `type = "about:blank"` và chưa có chi tiết.
    pub fn new(status: u16, title: impl Into<String>) -> Self {
        Self {
            problem_type: "about:blank".to_owned(),
            title: title.into(),
            status,
            detail: None,
            instance: None,
            code: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ProblemDetails, PROBLEM_CONTENT_TYPE};

    #[test]
    fn mac_dinh_theo_rfc_9457() {
        let problem = ProblemDetails::new(404, "Not Found");
        let value = serde_json::to_value(&problem).unwrap();
        assert_eq!(value["type"], "about:blank");
        assert_eq!(value["status"], 404);
        assert!(value.get("detail").is_none());
        assert_eq!(PROBLEM_CONTENT_TYPE, "application/problem+json");
    }
}