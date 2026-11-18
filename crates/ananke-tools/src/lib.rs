//! Tool tích hợp sẵn của Ananke: `bash`, `read`, `write`, `edit`, `grep`, `glob`.
//!
//! Mỗi tool implement [`ananke_core::Tool`] nên engine không cần biết tool đến từ
//! đâu (AD-01). Phần áp chính sách nằm ở [`ToolPolicy`] — xem `ARCHITECTURE.md`
//! §7.2: trên Android/Termux không có sandbox cấp OS, nên policy trong tiến trình
//! là hàng rào duy nhất mà crate này tự dựng được; `/api/meta` vẫn phải báo
//! `sandbox = "none"` cho client.
//!
//! Tool có `mutating() == true` (`bash`, `write`, `edit`) sẽ bị engine đem qua
//! permission rules (§7.3) trước khi chạy.

#![forbid(unsafe_code)]

mod bash;
mod fs_edit;
mod fs_read;
mod fs_write;
mod policy;
mod search;
mod util;
mod walk;

pub use bash::BashTool;
pub use fs_edit::EditTool;
pub use fs_read::ReadTool;
pub use fs_write::WriteTool;
pub use policy::{PolicyError, PolicyMode, ToolPolicy};
pub use search::{GlobTool, GrepTool};

use std::sync::Arc;

use ananke_core::{RegistryError, ToolRegistry};

/// Đăng ký 6 tool tích hợp vào `registry`. Trùng tên là lỗi (không ghi đè).
pub fn register_builtin(
    registry: &mut ToolRegistry,
    policy: ToolPolicy,
) -> Result<(), RegistryError> {
    let policy = Arc::new(policy);
    registry.register(Arc::new(BashTool::new(Arc::clone(&policy))))?;
    registry.register(Arc::new(ReadTool::new(Arc::clone(&policy))))?;
    registry.register(Arc::new(WriteTool::new(Arc::clone(&policy))))?;
    registry.register(Arc::new(EditTool::new(Arc::clone(&policy))))?;
    registry.register(Arc::new(GrepTool::new(Arc::clone(&policy))))?;
    registry.register(Arc::new(GlobTool::new(Arc::clone(&policy))))?;
    Ok(())
}