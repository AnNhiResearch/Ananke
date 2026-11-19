//! Cấu hình server đọc từ biến môi trường (không gọi mạng).

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;

/// Địa chỉ bind mặc định khi không có `ANANKE_BIND`.
pub const DEFAULT_BIND: &str = "127.0.0.1:9876";
/// Đường dẫn SQLite mặc định khi không có `ANANKE_DB`.
pub const DEFAULT_DB: &str = "ananke.db";
/// Model mặc định khi không có `ANANKE_MODEL`.
pub const DEFAULT_MODEL: &str = "kilo-default";

/// Cấu hình runtime của server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub db_path: PathBuf,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

impl Config {
    /// Đọc cấu hình từ env; lỗi nếu `ANANKE_BIND` không phải `SocketAddr` hợp lệ.
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = std::env::var("ANANKE_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_owned());
        let bind_addr = bind
            .parse::<SocketAddr>()
            .with_context(|| format!("ANANKE_BIND không hợp lệ: {bind}"))?;

        let db_path = PathBuf::from(
            std::env::var("ANANKE_DB").unwrap_or_else(|_| DEFAULT_DB.to_owned()),
        );
        let model = std::env::var("ANANKE_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
        let base_url = std::env::var("ANANKE_BASE_URL").ok();
        let api_key = std::env::var("ANANKE_API_KEY").ok();

        Ok(Self {
            bind_addr,
            db_path,
            model,
            base_url,
            api_key,
        })
    }
}