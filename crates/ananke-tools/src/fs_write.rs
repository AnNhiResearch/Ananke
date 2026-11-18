//! Tool `write`: ghi đè toàn bộ nội dung một file.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use ananke_core::{Tool, ToolOutput};

use crate::policy::ToolPolicy;
use crate::util::{display_path, require_str, require_str_allow_empty};

pub struct WriteTool {
    policy: Arc<ToolPolicy>,
}

impl WriteTool {
    pub fn new(policy: Arc<ToolPolicy>) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Ghi nội dung vào file, tạo thư mục cha nếu cần và ghi đè nếu file đã tồn tại. \
         Dùng `edit` khi chỉ muốn sửa một đoạn."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Đường dẫn file cần ghi."
                },
                "content": {
                    "type": "string",
                    "description": "Toàn bộ nội dung mới của file."
                }
            },
            "required": ["path", "content"],
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
        let content = match require_str_allow_empty(&arguments, "content") {
            Ok(content) => content,
            Err(error) => return error,
        };
        let path = match self.policy.resolve_write(&raw) {
            Ok(path) => path,
            Err(error) => return ToolOutput::error(error.to_string()),
        };

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(error) = tokio::fs::create_dir_all(parent).await {
                    return ToolOutput::error(format!(
                        "không tạo được thư mục `{}`: {error}",
                        parent.display()
                    ));
                }
            }
        }

        if let Err(error) = tokio::fs::write(&path, &content).await {
            return ToolOutput::error(format!("không ghi được `{raw}`: {error}"));
        }
        ToolOutput::ok(format!(
            "đã ghi {} byte vào {}",
            content.len(),
            display_path(&path, self.policy.base())
        ))
    }
}