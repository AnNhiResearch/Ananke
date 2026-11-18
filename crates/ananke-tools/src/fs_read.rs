//! Tool `read`: đọc file văn bản theo dòng.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use ananke_core::{Tool, ToolOutput};

use crate::policy::ToolPolicy;
use crate::util::{display_path, opt_u64, require_str, truncate_text};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_LINES: usize = 2_000;
const MAX_LINES: usize = 20_000;
const MAX_OUTPUT_BYTES: usize = 256_000;

pub struct ReadTool {
    policy: Arc<ToolPolicy>,
}

impl ReadTool {
    pub fn new(policy: Arc<ToolPolicy>) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Đọc file văn bản trong workspace. Trả về nội dung kèm số dòng; \
         dùng `offset` và `limit` để đọc từng phần với file lớn."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Đường dẫn file, tương đối so với workspace hoặc tuyệt đối."
                },
                "offset": {
                    "type": "integer",
                    "description": "Dòng bắt đầu (1-based). Mặc định 1."
                },
                "limit": {
                    "type": "integer",
                    "description": "Số dòng tối đa cần đọc. Mặc định 2000."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, arguments: Value) -> ToolOutput {
        let raw = match require_str(&arguments, "path") {
            Ok(raw) => raw,
            Err(error) => return error,
        };
        let path = match self.policy.resolve_read(&raw) {
            Ok(path) => path,
            Err(error) => return ToolOutput::error(error.to_string()),
        };

        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) => return ToolOutput::error(format!("không đọc được `{raw}`: {error}")),
        };
        if metadata.is_dir() {
            return ToolOutput::error(format!("`{raw}` là thư mục, không phải file"));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return ToolOutput::error(format!(
                "file `{raw}` lớn hơn giới hạn {} byte",
                MAX_FILE_BYTES
            ));
        }

        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) => return ToolOutput::error(format!("không đọc được `{raw}`: {error}")),
        };
        if bytes.iter().take(8192).any(|byte| *byte == 0) {
            return ToolOutput::error(format!("`{raw}` là file nhị phân, không đọc bằng tool này"));
        }
        let text = String::from_utf8_lossy(&bytes);

        let offset = opt_u64(&arguments, "offset").unwrap_or(1).max(1) as usize;
        let limit = opt_u64(&arguments, "limit")
            .unwrap_or(DEFAULT_LINES as u64)
            .clamp(1, MAX_LINES as u64) as usize;

        let name = display_path(&path, self.policy.base());
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        if total == 0 {
            return ToolOutput::ok(format!("{name}: file rỗng"));
        }
        if offset > total {
            return ToolOutput::ok(format!(
                "{name}: chỉ có {total} dòng, `offset` {offset} vượt quá cuối file"
            ));
        }

        let start = offset;
        let end = total.min(start.saturating_sub(1).saturating_add(limit));
        let body = lines[start - 1..end].join("\n");

        let mut result = format!("{name}: dòng {start}-{end} / {total}\n{body}");
        if end < total {
            result.push_str(&format!(
                "\n… còn {} dòng; dùng `offset` để đọc tiếp",
                total - end
            ));
        }
        ToolOutput::ok(truncate_text(&result, MAX_OUTPUT_BYTES))
    }
}