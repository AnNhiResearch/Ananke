//! Ananke core engine.
//!
//! Core không biết gì về UI hay transport: nó nhận input có kiểu và phát
//! [`Event`]. Xem `ARCHITECTURE.md` (AD-01, AD-06, AD-17).

#![forbid(unsafe_code)]

pub mod approval;
pub mod engine;
pub mod event;
pub mod ids;
pub mod message;
pub mod part;
pub mod provider;
pub mod session;
pub mod store;
pub mod tool;
pub mod turn;

pub use approval::{AllowAll, Approver, Decision, PermissionMode, PermissionRequest, Verdict};
pub use engine::{Engine, EngineError, MAX_TOOL_ITERATIONS};
pub use event::{Event, EventKind, EventSink, MemoryEventSink};
pub use ids::{MessageId, PartId, PermissionId, SessionId, ToolCallId, TurnId};
pub use message::{Message, Role};
pub use part::{Part, PartKind};
pub use provider::{ModelProvider, ProviderError, ProviderEvent, ProviderRequest, ProviderStream, ToolSchema};
pub use session::Session;
pub use store::{InMemoryStore, SessionStore, StoreError};
pub use tool::{RegistryError, Tool, ToolOutput, ToolRegistry};
pub use turn::{Turn, TurnStatus, Usage};