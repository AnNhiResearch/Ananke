//! Bộ phân tích SSE của OpenAI-compatible `chat/completions`.
//!
//! Mỗi dòng `data: {...}` là một chunk JSON; `data: [DONE]` kết thúc stream.
//! Tool call đến từng mảnh: chunk đầu mang `id`/`name`, các chunk sau chỉ bổ
//! sung `function.arguments`. Ta gom theo `index` rồi mới phát `ToolCall`.

use std::collections::{BTreeMap, VecDeque};

use ananke_core::{ProviderEvent, ProviderStream, ToolCallId, Usage};
use bytes::Bytes;
use futures_util::stream::{self, BoxStream, StreamExt};
use serde_json::Value;

pub(crate) type ByteStream = BoxStream<'static, Result<Bytes, reqwest::Error>>;

/// Bọc byte stream HTTP thành stream [`ProviderEvent`].
pub(crate) fn parse(bytes: ByteStream) -> ProviderStream {
    let state = StreamState::new(bytes);
    let stream = stream::unfold(state, |mut st| async move {
        loop {
            if let Some(event) = st.pending.pop_front() {
                return Some((event, st));
            }
            if st.finished {
                return None;
            }
            match st.bytes.next().await {
                Some(Ok(chunk)) => st.feed(&chunk),
                Some(Err(err)) => {
                    st.push(ProviderEvent::Failed {
                        reason: err.to_string(),
                    });
                    st.finished = true;
                }
                None => st.finish(),
            }
        }
    });
    Box::pin(stream)
}

struct StreamState {
    bytes: ByteStream,
    buffer: Vec<u8>,
    pending: VecDeque<ProviderEvent>,
    tools: BTreeMap<usize, ToolAcc>,
    usage: Usage,
    finished: bool,
}

#[derive(Default)]
struct ToolAcc {
    id: Option<String>,
    name: String,
    arguments: String,
}

impl StreamState {
    fn new(bytes: ByteStream) -> Self {
        Self {
            bytes,
            buffer: Vec::new(),
            pending: VecDeque::new(),
            tools: BTreeMap::new(),
            usage: Usage::default(),
            finished: false,
        }
    }

    fn push(&mut self, event: ProviderEvent) {
        self.pending.push_back(event);
    }

    /// Nạp thêm byte, tách các dòng hoàn chỉnh để xử lý.
    fn feed(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=pos).collect();
            line.pop(); // bỏ '\n'
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            self.handle_line(&line);
        }
    }

    fn handle_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            return;
        }
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data == "[DONE]" {
            self.finish();
            return;
        }
        if data.is_empty() {
            return;
        }
        // Dòng không phải JSON hợp lệ: bỏ qua thay vì làm hỏng cả stream.
        if let Ok(value) = serde_json::from_str::<Value>(data) {
            self.handle_chunk(&value);
        }
    }

    fn handle_chunk(&mut self, chunk: &Value) {
        if let Some(err) = chunk.get("error") {
            let reason = err
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| err.to_string());
            self.push(ProviderEvent::Failed { reason });
            self.finished = true;
            return;
        }

        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            if let Some(n) = usage.get("prompt_tokens").and_then(Value::as_u64) {
                self.usage.input_tokens = n;
            }
            if let Some(n) = usage.get("completion_tokens").and_then(Value::as_u64) {
                self.usage.output_tokens = n;
            }
        }

        let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
            return;
        };
        let mut stop = false;
        for choice in choices {
            if choice.get("index").and_then(Value::as_u64).unwrap_or(0) != 0 {
                continue;
            }
            if let Some(delta) = choice.get("delta") {
                // Nhiều gateway dùng `reasoning`, số khác dùng `reasoning_content`.
                let reasoning = delta
                    .get("reasoning")
                    .or_else(|| delta.get("reasoning_content"))
                    .and_then(Value::as_str);
                if let Some(text) = reasoning.filter(|t| !t.is_empty()) {
                    self.push(ProviderEvent::ReasoningDelta(text.to_string()));
                }
                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        self.push(ProviderEvent::TextDelta(text.to_string()));
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        self.absorb_tool_call(call);
                    }
                }
            }
            if choice
                .get("finish_reason")
                .map(|r| !r.is_null())
                .unwrap_or(false)
            {
                stop = true;
            }
        }
        if stop {
            self.flush_tools();
        }
    }

    fn absorb_tool_call(&mut self, call: &Value) {
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let acc = self.tools.entry(index).or_default();
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                acc.id = Some(id.to_string());
            }
        }
        if let Some(function) = call.get("function") {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                if !name.is_empty() {
                    acc.name = name.to_string();
                }
            }
            if let Some(args) = function.get("arguments").and_then(Value::as_str) {
                acc.arguments.push_str(args);
            }
        }
    }

    /// Phát tất cả tool call đã gom xong, theo thứ tự `index`.
    fn flush_tools(&mut self) {
        if self.tools.is_empty() {
            return;
        }
        let tools = std::mem::take(&mut self.tools);
        for (_, acc) in tools {
            if acc.name.is_empty() {
                continue;
            }
            let arguments = if acc.arguments.trim().is_empty() {
                Value::Object(Default::default())
            } else {
                serde_json::from_str(&acc.arguments).unwrap_or(Value::Null)
            };
            let call_id = acc
                .id
                .map(|id| ToolCallId::from(id.as_str()))
                .unwrap_or_default();
            self.push(ProviderEvent::ToolCall {
                call_id,
                tool: acc.name,
                arguments,
            });
        }
    }

    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.flush_tools();
        self.push(ProviderEvent::Done {
            usage: self.usage,
        });
        self.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(sse: &str) -> Vec<ProviderEvent> {
        let mut state = StreamState::new(stream::empty().boxed());
        state.feed(sse.as_bytes());
        state.finish();
        state.pending.into_iter().collect()
    }

    fn texts(events: &[ProviderEvent]) -> String {
        events
            .iter()
            .filter_map(|e| match e {
                ProviderEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn gom_text_va_usage() {
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Xin \"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"chào\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = decode(sse);
        assert_eq!(texts(&events), "Xin chào");
        match events.last() {
            Some(ProviderEvent::Done { usage }) => {
                assert_eq!(usage.input_tokens, 7);
                assert_eq!(usage.output_tokens, 2);
            }
            other => panic!("phải kết thúc bằng Done, nhận {other:?}"),
        }
    }

    #[test]
    fn gom_tool_call_tu_nhieu_manh() {
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-abc\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Hanoi\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = decode(sse);
        let call = events
            .iter()
            .find_map(|e| match e {
                ProviderEvent::ToolCall {
                    call_id,
                    tool,
                    arguments,
                } => Some((call_id.clone(), tool.clone(), arguments.clone())),
                _ => None,
            })
            .expect("phải có ToolCall");
        assert_eq!(call.0.as_str(), "call-abc");
        assert_eq!(call.1, "get_weather");
        assert_eq!(call.2, serde_json::json!({ "city": "Hanoi" }));
    }

    #[test]
    fn reasoning_duoc_tach_rieng() {
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"nghĩ\",\"content\":\"\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"đáp\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = decode(sse);
        assert_eq!(texts(&events), "đáp");
        assert!(events.iter().any(
            |e| matches!(e, ProviderEvent::ReasoningDelta(t) if t == "nghĩ")
        ));
    }

    #[test]
    fn error_chunk_thanh_failed() {
        let sse = "data: {\"error\":{\"message\":\"hết quota\"}}\n\n";
        let events = decode(sse);
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Failed { reason }) if reason.contains("hết quota")
        ));
    }
}