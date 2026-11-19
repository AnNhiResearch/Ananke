#![forbid(unsafe_code)]
//! Hợp đồng dữ liệu của giao thức Ananke (AD-03, AD-10).
//!
//! Crate này chỉ chứa DTO + schema OpenAPI, **không** phụ thuộc `ananke-core`
//! hay `ananke-sandbox`: client chỉ cần `ananke-protocol` là đủ (AD-05).
//! Server map từ type nội bộ sang DTO ở đường biên HTTP.
//!
//! Quy ước dây (wire):
//! - JSON camelCase; hậu tố `ID` viết hoa (`sessionID`, `turnID`, `partID`) theo §6.
//! - Thời gian là mili-giây Unix (`u64`).
//! - Mọi request mang header [`PROTOCOL_HEADER`] với giá trị [`PROTOCOL_VERSION`].
//! - Lỗi dùng RFC 9457 [`ProblemDetails`] với content type [`PROBLEM_CONTENT_TYPE`].

pub mod error;
pub mod event;
pub mod limits;
pub mod meta;
pub mod message;
pub mod model;
pub mod openapi;
pub mod part;
pub mod permission;
pub mod session;
pub mod turn;

pub use error::{ProblemDetails, PROBLEM_CONTENT_TYPE};
pub use event::{EventDto, EVENT_TYPES};
pub use limits::{LimitsDto, PatchLimitsRequest};
pub use meta::{DeviceProfileDto, MetaResponse, SandboxKindDto};
pub use message::{MessageDto, RoleDto};
pub use model::{ListModelsResponse, ModelDto, RegisterModelRequest};
pub use part::{PartDto, PartKindDto};
pub use permission::{
    PermissionDecisionDto, PermissionDecisionRequest, PermissionModeDto, PermissionScopeDto,
};
pub use session::{CreateSessionRequest, SessionDto, SessionStateDto, UpdateSessionRequest};
pub use turn::{
    CreateTurnRequest, InputPartDto, SteerRequest, TurnAcceptedResponse, TurnDto, TurnStatusDto,
    UsageDto,
};

/// Phiên bản giao thức. Tăng khi thay đổi phá vỡ tương thích (AD-03, §12).
pub const PROTOCOL_VERSION: u32 = 1;

/// Header bắt buộc trên mọi request (`X-Ananke-Protocol: 1`).
pub const PROTOCOL_HEADER: &str = "X-Ananke-Protocol";

/// Base path của toàn bộ API (§5).
pub const API_BASE_PATH: &str = "/api";