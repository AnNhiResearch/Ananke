//! Provider adapter cho Ananke.
//!
//! Crate này hiện thực [`ananke_core::ModelProvider`] trên giao thức
//! OpenAI-compatible `chat/completions` (streaming). Nó không biết gì về
//! session hay event: chỉ nhận [`ananke_core::ProviderRequest`] và trả một
//! stream [`ananke_core::ProviderEvent`] (AD-01: provider là crate riêng).
//!
//! Kilo Gateway dùng được **ẩn danh**: truyền `api_key = None` thì request
//! không mang header `Authorization`.

#![forbid(unsafe_code)]

mod openai;
mod sse;

pub use openai::{OpenAiCompatConfig, OpenAiCompatProvider};