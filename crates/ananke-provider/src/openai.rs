//! Adapter cho API OpenAI-compatible `chat/completions` (streaming).

use ananke_core::{
    Message, ModelProvider, Part, ProviderError, ProviderRequest, ProviderStream, Role,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::sse;

/// Base URL của Kilo Gateway (ẩn danh, không cần API key).
pub const KILO_BASE_URL: &str = "https://api.kilo.ai/api/gateway";

#[derive(Clone, Debug)]
pub struct OpenAiCompatConfig {
    /// Gốc API, ví dụ `https://api.kilo.ai/api/gateway`; crate tự thêm `/v1/chat/completions`.
    pub base_url: String,
    /// `None` → không gửi `Authorization` (chế độ ẩn danh của Kilo).
    pub api_key: Option<String>,
    /// Header phụ (ví dụ `HTTP-Referer` cho OpenRouter).
    pub headers: Vec<(String, String)>,
}

impl OpenAiCompatConfig {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            headers: Vec::new(),
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// Provider gọi endpoint OpenAI-compatible và stream kết quả.
pub struct OpenAiCompatProvider {
    http: reqwest::Client,
    config: OpenAiCompatConfig,
}

impl OpenAiCompatProvider {
    pub fn new(config: OpenAiCompatConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    /// Kilo Gateway, chế độ ẩn danh (200 request/giờ/IP, không cần API key).
    pub fn kilo() -> Self {
        Self::new(OpenAiCompatConfig::new(KILO_BASE_URL))
    }

    pub fn config(&self) -> &OpenAiCompatConfig {
        &self.config
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatProvider {
    async fn stream(&self, request: ProviderRequest) -> Result<ProviderStream, ProviderError> {
        let url = format!(
            "{}/v1/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let body = build_body(&request);

        let mut builder = self.http.post(&url).json(&body);
        if let Some(key) = self.config.api_key.as_deref() {
            builder = builder.bearer_auth(key);
        }
        for (name, value) in &self.config.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }

        let response = builder
            .send()
            .await
            .map_err(|err| ProviderError::Failed(err.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Failed(format!("HTTP {status}: {text}")));
        }

        Ok(sse::parse(response.bytes_stream().boxed()))
    }
}

fn build_body(request: &ProviderRequest) -> Value {
    let mut messages = Vec::new();
    for message in &request.messages {
        encode_message(&mut messages, message);
    }

    let mut body = json!({
        "model": request.model,
        "stream": true,
        "messages": messages,
    });

    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }

    body
}

/// Một [`Message`] của core có thể sinh nhiều message OpenAI (mỗi tool result là một).
fn encode_message(out: &mut Vec<Value>, message: &Message) {
    match message.role {
        Role::User => out.push(json!({
            "role": "user",
            "content": collect_text(&message.parts),
        })),
        Role::Assistant => {
            let content = collect_text(&message.parts);
            let tool_calls: Vec<Value> = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::ToolCall {
                        call_id,
                        tool,
                        arguments,
                        ..
                    } => Some(json!({
                        "id": call_id.as_str(),
                        "type": "function",
                        "function": {
                            "name": tool,
                            "arguments": arguments.to_string(),
                        }
                    })),
                    _ => None,
                })
                .collect();
            let mut encoded = json!({
                "role": "assistant",
                "content": if content.is_empty() {
                    Value::Null
                } else {
                    Value::String(content)
                },
            });
            if !tool_calls.is_empty() {
                encoded["tool_calls"] = Value::Array(tool_calls);
            }
            out.push(encoded);
        }
        Role::Tool => {
            for part in &message.parts {
                if let Part::ToolResult {
                    call_id, ok, output, ..
                } = part
                {
                    let content = if *ok {
                        output.clone()
                    } else {
                        format!("error: {output}")
                    };
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id.as_str(),
                        "content": content,
                    }));
                }
            }
        }
    }
}

fn collect_text(parts: &[Part]) -> String {
    let mut text = String::new();
    for part in parts {
        if let Part::Text { text: chunk, .. } = part {
            text.push_str(chunk);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ananke_core::ToolSchema;

    #[test]
    fn map_user_va_tool_result() {
        let mut out = Vec::new();
        encode_message(&mut out, &Message::text(Role::User, "chào"));
        let mut tool_msg = Message::new(Role::Tool, vec![]);
        tool_msg.parts.push(Part::tool_result(
            ananke_core::ToolCallId::from("call-1"),
            "echo",
            true,
            "ok",
        ));
        encode_message(&mut out, &tool_msg);

        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], "chào");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call-1");
        assert_eq!(out[1]["content"], "ok");
    }

    #[test]
    fn assistant_tool_call_thanh_tool_calls() {
        let mut message = Message::new(Role::Assistant, vec![]);
        message.parts.push(Part::tool_call(
            ananke_core::ToolCallId::from("call-9"),
            "get_weather",
            json!({ "city": "Hanoi" }),
        ));
        let mut out = Vec::new();
        encode_message(&mut out, &message);
        assert!(out[0]["content"].is_null());
        assert_eq!(out[0]["tool_calls"][0]["id"], "call-9");
        assert_eq!(out[0]["tool_calls"][0]["function"]["name"], "get_weather");
        let args: Value = serde_json::from_str(
            out[0]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(args, json!({ "city": "Hanoi" }));
    }

    #[test]
    fn tools_duoc_ma_hoa() {
        let request = ProviderRequest {
            model: "kilo-auto/free".into(),
            messages: vec![Message::text(Role::User, "hi")],
            tools: vec![ToolSchema {
                name: "echo".into(),
                description: "lặp lại".into(),
                parameters: json!({ "type": "object" }),
            }],
        };
        let body = build_body(&request);
        assert_eq!(body["model"], "kilo-auto/free");
        assert_eq!(body["stream"], true);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "echo");
    }
}