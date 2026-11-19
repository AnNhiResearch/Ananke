//! Cấu hình và nạp danh sách model từ file `models.json`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use ananke_core::{ModelProvider, ProviderError, ProviderRequest, ProviderStream};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::openai::{OpenAiCompatConfig, OpenAiCompatProvider};

/// Đặc tả của một model được khai báo trong `models.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(alias = "baseUrl")]
    pub base_url: String,
    #[serde(default, alias = "apiKey")]
    pub api_key: Option<String>,
    #[serde(default, alias = "apiKeyEnv")]
    pub api_key_env: Option<String>,
    #[serde(default, alias = "contextWindow")]
    pub context_window: Option<usize>,
    #[serde(default, alias = "maxTokens")]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Field bổ sung gửi kèm mọi request tới model này (ví dụ `reasoning_effort`).
    #[serde(default, alias = "extraBody")]
    pub extra_body: serde_json::Map<String, serde_json::Value>,
}

impl ModelSpec {
    /// Lấy api_key đã giải quyết (ưu tiên api_key trực tiếp, nếu không có đọc từ env).
    pub fn resolve_api_key(&self) -> Option<String> {
        if let Some(ref k) = self.api_key {
            if !k.trim().is_empty() {
                return Some(k.clone());
            }
        }
        if let Some(ref env_name) = self.api_key_env {
            if let Ok(val) = std::env::var(env_name) {
                if !val.trim().is_empty() {
                    return Some(val);
                }
            }
        }
        None
    }

    /// Khởi tạo `OpenAiCompatProvider` từ cấu hình model này.
    pub fn to_provider(&self) -> OpenAiCompatProvider {
        let mut config = OpenAiCompatConfig::new(&self.base_url);
        if let Some(key) = self.resolve_api_key() {
            config = config.with_api_key(key);
        }
        for (k, v) in &self.headers {
            config = config.with_header(k, v);
        }
        config = config.with_extra_body(self.extra_body.clone());
        OpenAiCompatProvider::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_models_json_camel_case() {
        let json = r#"{
            "models": [
                {
                    "id": "nvidia/nemotron-3-ultra-550b-a55b:free",
                    "name": "NVIDIA Nemotron 3 Ultra 550B",
                    "baseUrl": "https://ai-gateway.kilo.ai/api/v1",
                    "apiKey": "test-key-123"
                }
            ]
        }"#;
        let config = ModelsConfigFile::from_json_str(json).unwrap();
        assert_eq!(config.models.len(), 1);
        let m = &config.models[0];
        assert_eq!(m.id, "nvidia/nemotron-3-ultra-550b-a55b:free");
        assert_eq!(m.base_url, "https://ai-gateway.kilo.ai/api/v1");
        assert_eq!(m.resolve_api_key(), Some("test-key-123".to_string()));
    }
}

/// File cấu hình chứa mảng các `models`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelsConfigFile {
    #[serde(default)]
    pub models: Vec<ModelSpec>,
}

impl ModelsConfigFile {
    /// Đọc từ chuỗi JSON.
    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Đọc từ đường dẫn file `models.json`.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let content = fs::read_to_string(path)?;
        Self::from_json_str(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

/// Registry quản lý danh sách model động và ánh xạ sang provider tương ứng.
#[derive(Clone)]
pub struct DynamicModelRegistry {
    models: HashMap<String, ModelSpec>,
    providers: HashMap<String, Arc<dyn ModelProvider>>,
    default_provider: Option<Arc<dyn ModelProvider>>,
}

impl DynamicModelRegistry {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
            providers: HashMap::new(),
            default_provider: None,
        }
    }

    /// Thiết lập provider mặc định (fallback khi không tìm thấy model).
    pub fn with_default_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        self.default_provider = Some(provider);
        self
    }

    /// Thêm hoặc ghi đè một model.
    pub fn register(&mut self, spec: ModelSpec) {
        let provider = Arc::new(spec.to_provider());
        self.providers.insert(spec.id.clone(), provider);
        self.models.insert(spec.id.clone(), spec);
    }

    /// Nạp cấu hình từ `models.json`.
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<usize, std::io::Error> {
        let config = ModelsConfigFile::load_from_file(path)?;
        let count = config.models.len();
        for spec in config.models {
            self.register(spec);
        }
        Ok(count)
    }

    /// Lấy danh sách thông tin các model đã đăng ký.
    pub fn list_models(&self) -> Vec<ModelSpec> {
        self.models.values().cloned().collect()
    }

    /// Kiểm tra model có tồn tại không.
    pub fn has_model(&self, id: &str) -> bool {
        self.models.contains_key(id)
    }
}

#[async_trait]
impl ModelProvider for DynamicModelRegistry {
    async fn stream(&self, request: ProviderRequest) -> Result<ProviderStream, ProviderError> {
        let model_id = &request.model;
        if let Some(provider) = self.providers.get(model_id) {
            provider.stream(request).await
        } else if let Some(ref fallback) = self.default_provider {
            fallback.stream(request).await
        } else {
            Err(ProviderError::Failed(format!(
                "model '{}' not registered in models.json and no fallback provider configured",
                model_id
            )))
        }
    }
}
