//! Tool `edit`: thay thế chính xác một đoạn văn bản trong file.
//!
//! Không dùng fuzzy match: `old_string` phải xuất hiện đúng một lần (trừ khi
//! bật `replace_all`), nếu không tool trả lỗi để model đọc lại file và sửa lời
//! gọi. Nhờ vậy không có thay đổi "gần đúng" ngoài ý muốn.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use ananke_core::{Tool, ToolOutput};

use crate::policy::ToolPolicy;
use crate::util::{display_path, opt_bool, require_str, require_str_allow_empty};

const MAX_EDIT_BYTES: u64 = 2 * 1024 * 1024;

pub struct EditTool {
    policy: Arc<ToolPolicy>,
}

impl EditTool {
    pub fn new(policy: Arc<ToolPolicy>) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Thay thế một đoạn văn bản trong file. `old_string` phải khớp chính xác \
         và duy nhất; đặt `replace_all` nếu muốn thay mọi lần xuất hiện."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Đường dẫn file cần sửa."
                },
                "old_string": {
                    "type": "string",
                    "description": "Đoạn văn bản gốc cần thay, phải khớp chính xác."
                },
                "new_string": {
                    "type": "string",
                    "description": "Đoạn văn bản thay thế. Chuỗi rỗng nghĩa là xoá."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Thay mọi lần xuất hiện thay vì yêu cầu duy nhất. Mặc định false."
                }
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        })
    }

    fn mutating(&self) -> bool {
        true
    }

    async fn execute(&self, arguments: Value) -> ToolOutput {
        let raw = match require_str(&arguments, "path") {
            Ok(raw) => raw,
            Err(error) => return error,
        };
        let old_string = match require_str(&arguments, "old_string") {
            Ok(value) => value,
            Err(error) => return error,
        };
        let new_string = match require_str_allow_empty(&arguments, "new_string") {
            Ok(value) => value,
            Err(error) => return error,
        };
        let replace_all = opt_bool(&arguments, "replace_all").unwrap_or(false);

        let path = match self.policy.resolve_write(&raw) {
            Ok(path) => path,
            Err(error) => return ToolOutput::error(error.to_string()),
        };

        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) => return ToolOutput::error(format!("không đọc được `{raw}`: {error}")),
        };
        if metadata.len() > MAX_EDIT_BYTES {
            return ToolOutput::error(format!(
                "file `{raw}` lớn hơn giới hạn {} byte",
                MAX_EDIT_BYTES
            ));
        }

        let text = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(error) => {
                return ToolOutput::error(format!("không đọc được `{raw}` dạng UTF-8: {error}"))
            }
        };

        let matches = text.matches(&old_string).count();
        if matches == 0 {
            return ToolOutput::error(format!(
                "`old_string` không xuất hiện trong `{raw}`; hãy đọc lại file rồi thử lại"
            ));
        }
        if matches > 1 && !replace_all {
            return ToolOutput::error(format!(
                "`old_string` xuất hiện {matches} lần trong `{raw}`; thêm ngữ cảnh để khớp duy nhất \
                 hoặc đặt `replace_all: true`"
            ));
        }

        let updated = if replace_all {
            text.replace(&old_string, &new_string)
        } else {
            text.replacen(&old_string, &new_string, 1)
        };
        if let Err(error) = tokio::fs::write(&path, &updated).await {
            return ToolOutput::error(format!("không ghi được `{raw}`: {error}"));
        }

        let replaced = if replace_all { matches } else { 1 };
        ToolOutput::ok(format!(
            "đã thay {replaced} chỗ trong {}",
            display_path(&path, self.policy.base())
        ))
    }
}