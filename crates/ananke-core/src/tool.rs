use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSchema;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolOutput {
    pub ok: bool,
    pub content: String,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            ok: true,
            content: content.into(),
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            ok: false,
            content: content.into(),
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    fn description(&self) -> &str;

    fn schema(&self) -> Value;

    /// Tool có thay đổi trạng thái ngoài (ghi file, chạy lệnh, gọi mạng) hay không.
    /// Engine dựa vào đây để áp permission policy (§7.3).
    fn mutating(&self) -> bool {
        false
    }

    async fn execute(&self, arguments: Value) -> ToolOutput;
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("tool `{0}` đã được đăng ký")]
    Duplicate(String),
}

/// Sổ đăng ký tool trong bộ nhớ. Trùng tên là lỗi chứ không ghi đè âm thầm.
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        let name = tool.name().to_owned();
        if self.tools.contains_key(&name) {
            return Err(RegistryError::Duplicate(name));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .values()
            .map(|tool| ToolSchema {
                name: tool.name().to_owned(),
                description: tool.description().to_owned(),
                parameters: tool.schema(),
            })
            .collect()
    }
}