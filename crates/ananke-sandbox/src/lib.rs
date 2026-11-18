#![forbid(unsafe_code)]
//! Phát hiện **năng lực sandbox thật** của máy đang chạy (ARCHITECTURE §7.2) và
//! profile thiết bị theo RAM (AD-15).
//!
//! Nguyên tắc: *sandbox là theo năng lực, không theo lời hứa*. Crate này không
//! giả định nền tảng nào có sandbox; nó đo rồi trả [`SandboxReport`] để
//! `/api/meta` báo đúng một trong bốn giá trị `landlock` / `seatbelt` /
//! `restricted-token` / `none`. Trên Android/Termux kết quả luôn là `none` —
//! ở đó permission rules là hàng rào duy nhất, không phải OS sandbox.
//!
//! Crate này **chỉ đo**, không áp đặt policy lên tiến trình con. Phần chặn
//! đường dẫn ở mức API nằm trong `ananke-tools`; phần enforce cấp OS sẽ do cơ
//! chế tương ứng của từng nền tảng đảm nhận khi chạy tool.

mod detect;
mod profile;

pub use detect::detect;
pub use profile::{device_profile, mem_total_bytes, DeviceProfile};

use serde::{Deserialize, Serialize};

/// Cơ chế sandbox cấp OS — đúng bốn giá trị `/api/meta` được phép trả (§5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxKind {
    /// Linux ≥ 5.13: Landlock (+ seccomp, ưu tiên `bwrap` nếu có).
    Landlock,
    /// macOS: Seatbelt profile.
    Seatbelt,
    /// Windows: restricted token + AppContainer.
    RestrictedToken,
    /// Không enforce được — permission rules là hàng rào duy nhất.
    None,
}

impl SandboxKind {
    /// Chuỗi ổn định dùng trong `/api/meta` và log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Landlock => "landlock",
            Self::Seatbelt => "seatbelt",
            Self::RestrictedToken => "restricted-token",
            Self::None => "none",
        }
    }
}

/// Kết quả đo năng lực sandbox lúc khởi động.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxReport {
    kind: SandboxKind,
    /// Cơ chế cụ thể đo được, ví dụ `["bwrap", "landlock", "seccomp"]`.
    mechanisms: Vec<String>,
    /// Vì sao không enforce được, hoặc ghi chú về cơ chế được chọn.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl SandboxReport {
    /// Báo không có sandbox kèm lý do đo được; `/api/meta` sẽ trả `"none"`.
    pub fn none(reason: impl Into<String>) -> Self {
        Self {
            kind: SandboxKind::None,
            mechanisms: Vec::new(),
            reason: Some(reason.into()),
        }
    }

    pub(crate) fn available(
        kind: SandboxKind,
        mechanisms: Vec<String>,
        reason: Option<String>,
    ) -> Self {
        debug_assert_ne!(kind, SandboxKind::None, "dùng `none()` cho trường hợp không enforce");
        Self {
            kind,
            mechanisms,
            reason,
        }
    }

    pub fn kind(&self) -> SandboxKind {
        self.kind
    }

    pub fn mechanisms(&self) -> &[String] {
        &self.mechanisms
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// `false` khi policy `workspace-write` **không** được OS bảo đảm.
    pub fn is_enforced(&self) -> bool {
        self.kind != SandboxKind::None
    }

    /// Client (kể cả app điện thoại) phải hiển thị cảnh báo khi hàm này trả `true`.
    pub fn needs_warning(&self) -> bool {
        !self.is_enforced()
    }
}