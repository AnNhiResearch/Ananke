//! Tiện ích dùng chung: đọc tham số JSON và cắt output.

use std::path::Path;

use ananke_core::ToolOutput;
use serde_json::Value;

/// Lấy tham số chuỗi bắt buộc, không được rỗng.
pub(crate) fn require_str(args: &Value, key: &str) -> Result<String, ToolOutput> {
    match args.get(key).and_then(Value::as_str) {
        Some(value) if !value.is_empty() => Ok(value.to_owned()),
        Some(_) => Err(ToolOutput::error(format!("tham số `{key}` không được rỗng"))),
        None => Err(ToolOutput::error(format!("thiếu tham số `{key}`"))),
    }
}

/// Như [`require_str`] nhưng cho phép chuỗi rỗng (ví dụ `content` khi tạo file trống).
pub(crate) fn require_str_allow_empty(args: &Value, key: &str) -> Result<String, ToolOutput> {
    match args.get(key).and_then(Value::as_str) {
        Some(value) => Ok(value.to_owned()),
        None => Err(ToolOutput::error(format!("thiếu tham số `{key}`"))),
    }
}

pub(crate) fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(crate) fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

pub(crate) fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

/// Cắt chuỗi theo biên ký tự UTF-8, kèm ghi chú để model biết đã bị cắt.
pub(crate) fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… (đã cắt bớt, tổng {} byte)", &text[..end], text.len())
}

/// Hiển thị đường dẫn tương đối so với workspace khi có thể.
pub(crate) fn display_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}