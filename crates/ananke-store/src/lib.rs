//! Bền hoá SQLite cho Ananke (AD-09).
//!
//! Crate này là cài đặt của [`ananke_core::SessionStore`] trên SQLite, kèm
//! event log append-only để client replay sau khi mất kết nối. Nó không biết gì
//! về HTTP hay UI: chỉ nhận primitive của core và trả chúng về nguyên trạng.
//!
//! Ràng buộc đã áp lên đây:
//! - **AD-08**: `part.delta` không vào event log; mỗi session có `seq` liên tục.
//! - **AD-09**: event ghi cùng transaction với state, nên không có event "ma"
//!   khi transaction rollback.
//! - **AD-19**: PRAGMA dành cho máy nhỏ (`cache_size`, `mmap_size`,
//!   `synchronous`) và checkpoint `TRUNCATE` định kỳ.

#![forbid(unsafe_code)]

mod migrations;
mod store;

pub use store::{RetentionConfig, SessionMeta, SqliteStore};