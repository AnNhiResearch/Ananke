//! Part — đơn vị nội dung nhỏ nhất client render được (AD-06).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// Loại part dùng trong `part.started` / `part.delta` (§6).
///
/// AD-06 còn mô tả `permission_request`, `file_change`, `command`,
/// `compaction`, `attachment`; các loại đó được thêm khi server thật sự phát ra.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartKindDto {
    Text,
    Reasoning,
    ToolCall,
    ToolResult,
}

/// Part trên dây. Client gửi input chỉ dùng [`crate::turn::InputPartDto`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartDto {
    Text {
        id: String,
        text: String,
    },
    Reasoning {
        id: String,
        text: String,
    },
    ToolCall {
        id: String,
        #[serde(rename = "callID")]
        call_id: String,
        tool: String,
        #[schema(value_type = Object)]
        arguments: Value,
    },
    ToolResult {
        id: String,
        #[serde(rename = "callID")]
        call_id: String,
        tool: String,
        ok: bool,
        output: String,
    },
}

#[cfg(test)]
mod tests {
    use super::PartDto;

    #[test]
    fn tool_call_dung_tag_type_va_call_id_hoa() {
        let part = PartDto::ToolCall {
            id: "prt_1".to_owned(),
            call_id: "call_1".to_owned(),
            tool: "bash".to_owned(),
            arguments: serde_json::json!({ "command": "ls" }),
        };
        let value = serde_json::to_value(&part).unwrap();
        assert_eq!(value["type"], "tool_call");
        assert_eq!(value["callID"], "call_1");
        assert_eq!(value["arguments"]["command"], "ls");

        let back: PartDto = serde_json::from_value(value).unwrap();
        assert_eq!(back, part);
    }

    #[test]
    fn tool_result_giu_ok_va_output() {
        let part = PartDto::ToolResult {
            id: "prt_2".to_owned(),
            call_id: "call_1".to_owned(),
            tool: "bash".to_owned(),
            ok: false,
            output: "exit 1".to_owned(),
        };
        let value = serde_json::to_value(&part).unwrap();
        assert_eq!(value["type"], "tool_result");
        assert_eq!(value["ok"], false);
        assert_eq!(value["callID"], "call_1");
    }
}