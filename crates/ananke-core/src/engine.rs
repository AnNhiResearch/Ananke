use std::future::poll_fn;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;

use crate::approval::{Approver, Decision, PermissionMode, PermissionRequest, Verdict};
use crate::event::{now_millis, Event, EventKind, EventSink};
use crate::ids::{PartId, PermissionId, SessionId, ToolCallId, TurnId};
use crate::message::{Message, Role};
use crate::part::{Part, PartKind};
use crate::provider::{ModelProvider, ProviderEvent, ProviderRequest};
use crate::session::Session;
use crate::store::{SessionStore, StoreError};
use crate::tool::{ToolOutput, ToolRegistry};
use crate::turn::{Turn, TurnStatus, Usage};

/// Chặn vòng lặp tool vô hạn khi model không chịu dừng.
pub const MAX_TOOL_ITERATIONS: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Agent loop của Ananke. Một `Engine` phục vụ nhiều session, mỗi lần chạy turn
/// nhận `&mut Session` để core không phải giữ state ẩn (AD-01, AD-06).
pub struct Engine {
    provider: Arc<dyn ModelProvider>,
    tools: Arc<ToolRegistry>,
    approver: Arc<dyn Approver>,
    events: Arc<dyn EventSink>,
    store: Arc<dyn SessionStore>,
    permission: PermissionMode,
    seq: AtomicU64,
}

impl Engine {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        tools: Arc<ToolRegistry>,
        approver: Arc<dyn Approver>,
        events: Arc<dyn EventSink>,
        store: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            provider,
            tools,
            approver,
            events,
            store,
            permission: PermissionMode::WorkspaceWrite,
            seq: AtomicU64::new(0),
        }
    }

    pub fn with_permission(mut self, permission: PermissionMode) -> Self {
        self.permission = permission;
        self
    }

    pub fn permission_mode(&self) -> PermissionMode {
        self.permission
    }

    /// Chạy một turn mới: ghi input, persist turn, rồi lặp model ↔ tool tới khi xong.
    pub async fn run_turn(
        &self,
        session: &mut Session,
        input: Vec<Message>,
    ) -> Result<TurnId, EngineError> {
        for message in input {
            session.push_message(message);
        }

        let turn = Turn::new();
        let turn_id = turn.id.clone();
        session.push_turn(turn);

        let session_id = session.id.clone();
        if let Some(turn) = session.turn(&turn_id) {
            self.store.save_turn(&session_id, turn).await?;
        }
        self.emit(&session_id, EventKind::TurnStarted { turn_id: turn_id.clone() })
            .await;

        self.drive(session, &turn_id).await?;
        Ok(turn_id)
    }

    /// Hủy turn đang chạy hoặc chờ tool.
    /// - Nếu turn đang `Running` hoặc `AwaitingTool`, đổi status thành `Interrupted`.
    /// - Phát event `TurnInterrupted`.
    /// - Persist thay đổi status qua store.
    pub async fn cancel_turn(
        &self,
        session: &mut Session,
        turn_id: &TurnId,
    ) -> Result<(), EngineError> {
        let session_id = session.id.clone();
        let turn = session.turn_mut(turn_id).ok_or_else(|| {
            EngineError::Store(StoreError::NotFound(format!("turn {turn_id}")))
        })?;

        // Chỉ cho phép hủy khi đang Running hoặc AwaitingTool
        if !matches!(
            turn.status,
            TurnStatus::Running | TurnStatus::AwaitingTool
        ) {
            return Ok(());
        }

        turn.cancel();
        turn.status = TurnStatus::Interrupted;
        self.store.save_turn(&session_id, turn).await?;

        self.emit(
            &session_id,
            EventKind::TurnInterrupted {
                turn_id: turn_id.clone(),
            },
        )
        .await;
        Ok(())
    }

    /// Điều hướng turn: thay đổi input đang chờ và tiếp tục drive loop.
    /// Chỉ cho phép khi turn đang `AwaitingTool` hoặc `Running` (trước khi dispatch tool).
    pub async fn steer_turn(
        &self,
        session: &mut Session,
        turn_id: &TurnId,
        new_input: Vec<Message>,
    ) -> Result<(), EngineError> {
        let session_id = session.id.clone();

        // First check turn status and get info we need
        let can_steer = {
            let turn = session.turn(turn_id).ok_or_else(|| {
                EngineError::Store(StoreError::NotFound(format!("turn {turn_id}")))
            })?;
            matches!(turn.status, TurnStatus::Running | TurnStatus::AwaitingTool)
        };

        if !can_steer {
            return Err(EngineError::Store(StoreError::Failed(
                "turn không ở trạng thái cho phép steer".to_string(),
            )));
        }

        // Xóa message assistant cũ nếu có
        if let Some(last_msg) = session.messages().last() {
            if matches!(last_msg.role, Role::Assistant) {
                session.pop_message();
            }
        }

        // Thêm input mới
        for message in new_input {
            session.push_message(message);
        }

        // Nếu đang AwaitingTool, reset về Running để tiếp tục drive loop
        let should_reset = {
            let turn = session.turn(turn_id).ok_or_else(|| {
                EngineError::Store(StoreError::NotFound(format!("turn {turn_id}")))
            })?;
            matches!(turn.status, TurnStatus::AwaitingTool)
        };

        if should_reset {
            if let Some(turn) = session.turn_mut(turn_id) {
                turn.status = TurnStatus::Running;
                self.store.save_turn(&session_id, turn).await?;
            }
        }

        // Tiếp tục drive loop
        self.drive(session, turn_id).await
    }

    /// Hoàn tất turn đang `AwaitingTool` sau khi process khởi động lại (AD-17).
    pub async fn resume_turn(
        &self,
        session: &mut Session,
        turn_id: &TurnId,
    ) -> Result<(), EngineError> {
        let calls: Vec<PendingCall> = match session.turn(turn_id) {
            Some(turn) if matches!(turn.status, TurnStatus::AwaitingTool) => turn
                .pending_tool_calls()
                .into_iter()
                .map(|(call_id, tool, arguments)| PendingCall {
                    call_id: call_id.clone(),
                    tool: tool.to_owned(),
                    arguments: arguments.clone(),
                })
                .collect(),
            _ => return Ok(()),
        };

        let session_id = session.id.clone();
        if let Some(turn) = session.turn_mut(turn_id) {
            turn.status = TurnStatus::Running;
            self.store.save_turn(&session_id, turn).await?;
        }

        if !calls.is_empty() {
            self.run_tools(session, turn_id, calls).await?;
        }
        self.drive(session, turn_id).await
    }

    async fn drive(&self, session: &mut Session, turn_id: &TurnId) -> Result<(), EngineError> {
        for _ in 0..MAX_TOOL_ITERATIONS {
            // Kiểm tra cancellation trước mỗi iteration
            if let Some(turn) = session.turn(turn_id) {
                if turn.is_cancelled() {
                    self.fail_turn(session, turn_id, "turn bị hủy".to_string()).await?;
                    return Ok(());
                }
            }

            let outcome = self.model_step(session, turn_id).await?;

            // Kiểm tra cancellation sau model_step
            if let Some(turn) = session.turn(turn_id) {
                if turn.is_cancelled() {
                    self.fail_turn(session, turn_id, "turn bị hủy".to_string()).await?;
                    return Ok(());
                }
            }

            if let Some(reason) = outcome.failed {
                self.fail_turn(session, turn_id, reason).await?;
                return Ok(());
            }
            if outcome.tool_calls.is_empty() {
                self.complete_turn(session, turn_id).await?;
                return Ok(());
            }
            self.run_tools(session, turn_id, outcome.tool_calls).await?;
        }

        self.fail_turn(
            session,
            turn_id,
            format!("vượt quá {MAX_TOOL_ITERATIONS} vòng tool"),
        )
        .await
    }

    /// Một lượt gọi model: stream part ra event sink, gom tool call để dispatch sau.
    async fn model_step(
        &self,
        session: &mut Session,
        turn_id: &TurnId,
    ) -> Result<ModelOutcome, EngineError> {
        let session_id = session.id.clone();
        let request = ProviderRequest {
            model: session.model.clone(),
            messages: session.messages().to_vec(),
            tools: self.tools.schemas(),
        };

        let mut stream = match self.provider.stream(request).await {
            Ok(stream) => stream,
            Err(error) => {
                return Ok(ModelOutcome {
                    tool_calls: Vec::new(),
                    failed: Some(error.to_string()),
                })
            }
        };

        let mut parts: Vec<Part> = Vec::new();
        let mut open: Option<OpenPart> = None;
        let mut tool_calls: Vec<PendingCall> = Vec::new();
        let mut usage = Usage::default();
        let mut failed = None;

        while let Some(event) = poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            // Kiểm tra cancellation trong loop streaming
            if let Some(turn) = session.turn(turn_id) {
                if turn.is_cancelled() {
                    failed = Some("turn bị hủy".to_string());
                    break;
                }
            }

            match event {
                ProviderEvent::TextDelta(delta) => {
                    if !matches!(open, Some(OpenPart::Text { .. })) {
                        self.finish_open(&session_id, turn_id, &mut open, &mut parts)
                            .await;
                        open = Some(self.start_open(&session_id, turn_id, PartKind::Text).await);
                    }
                    if let Some(OpenPart::Text { id, text }) = open.as_mut() {
                        text.push_str(&delta);
                        self.emit(
                            &session_id,
                            EventKind::PartDelta {
                                turn_id: turn_id.clone(),
                                part_id: id.clone(),
                                kind: PartKind::Text,
                                delta,
                            },
                        )
                        .await;
                    }
                }
                ProviderEvent::ReasoningDelta(delta) => {
                    if !matches!(open, Some(OpenPart::Reasoning { .. })) {
                        self.finish_open(&session_id, turn_id, &mut open, &mut parts)
                            .await;
                        open =
                            Some(self.start_open(&session_id, turn_id, PartKind::Reasoning).await);
                    }
                    if let Some(OpenPart::Reasoning { id, text }) = open.as_mut() {
                        text.push_str(&delta);
                        self.emit(
                            &session_id,
                            EventKind::PartDelta {
                                turn_id: turn_id.clone(),
                                part_id: id.clone(),
                                kind: PartKind::Reasoning,
                                delta,
                            },
                        )
                        .await;
                    }
                }
                ProviderEvent::ToolCall {
                    call_id,
                    tool,
                    arguments,
                } => {
                    self.finish_open(&session_id, turn_id, &mut open, &mut parts)
                        .await;
                    let part = Part::tool_call(call_id.clone(), tool.clone(), arguments.clone());
                    self.emit(
                        &session_id,
                        EventKind::PartCompleted {
                            turn_id: turn_id.clone(),
                            part: part.clone(),
                        },
                    )
                    .await;
                    parts.push(part);
                    tool_calls.push(PendingCall {
                        call_id,
                        tool,
                        arguments,
                    });
                }
                ProviderEvent::Done { usage: reported } => usage = reported,
                ProviderEvent::Failed { reason } => {
                    failed = Some(reason);
                    break;
                }
            }
        }
        self.finish_open(&session_id, turn_id, &mut open, &mut parts)
            .await;

        let history: Vec<Part> = parts
            .iter()
            .filter(|part| matches!(part, Part::Text { .. } | Part::ToolCall { .. }))
            .cloned()
            .collect();

        if let Some(turn) = session.turn_mut(turn_id) {
            turn.parts.extend(parts);
            turn.usage.merge(usage);
            self.store.save_turn(&session_id, turn).await?;
        }

        if failed.is_none() && !history.is_empty() {
            session.push_message(Message::new(Role::Assistant, history));
        }

        Ok(ModelOutcome { tool_calls, failed })
    }

    /// Dispatch tool. AD-17: persist `AwaitingTool` trước khi chạy để lần khởi động
    /// sau biết phải hoàn tất turn này.
    async fn run_tools(
        &self,
        session: &mut Session,
        turn_id: &TurnId,
        calls: Vec<PendingCall>,
    ) -> Result<(), EngineError> {
        let session_id = session.id.clone();

        if let Some(turn) = session.turn_mut(turn_id) {
            turn.status = TurnStatus::AwaitingTool;
            self.store.save_turn(&session_id, turn).await?;
        }
        self.emit(
            &session_id,
            EventKind::TurnAwaitingTool {
                turn_id: turn_id.clone(),
            },
        )
        .await;

        for call in calls {
            // Kiểm tra cancellation trước khi chạy mỗi tool
            if let Some(turn) = session.turn(turn_id) {
                if turn.is_cancelled() {
                    self.fail_turn(session, turn_id, "turn bị hủy".to_string()).await?;
                    return Ok(());
                }
            }

            let output = match self.tools.get(&call.tool) {
                None => ToolOutput::error(format!("tool `{}` không tồn tại", call.tool)),
                Some(tool) => match self.permission.verdict(tool.mutating()) {
                    Verdict::Allow => tool.execute(call.arguments.clone()).await,
                    Verdict::Deny => ToolOutput::error(format!(
                        "policy `{:?}` không cho phép tool `{}`",
                        self.permission, call.tool
                    )),
                    Verdict::Ask => match self.ask_permission(session, turn_id, &call).await {
                        Decision::Deny => ToolOutput::error("người dùng từ chối chạy tool"),
                        _ => tool.execute(call.arguments.clone()).await,
                    },
                },
            };

            // Kiểm tra cancellation sau khi tool chạy xong
            if let Some(turn) = session.turn(turn_id) {
                if turn.is_cancelled() {
                    self.fail_turn(session, turn_id, "turn bị hủy".to_string()).await?;
                    return Ok(());
                }
            }

            let part = Part::tool_result(call.call_id.clone(), call.tool.clone(), output.ok, output.content);
            self.emit(
                &session_id,
                EventKind::PartCompleted {
                    turn_id: turn_id.clone(),
                    part: part.clone(),
                },
            )
            .await;

            if let Some(turn) = session.turn_mut(turn_id) {
                turn.parts.push(part.clone());
                self.store.save_turn(&session_id, turn).await?;
            }
            session.push_message(Message::new(Role::Tool, vec![part]));
        }

        if let Some(turn) = session.turn_mut(turn_id) {
            turn.status = TurnStatus::Running;
            self.store.save_turn(&session_id, turn).await?;
        }
        Ok(())
    }

    /// AD-07: phát `permission.requested` rồi chờ `Approver` quyết định; không có kênh ngược.
    async fn ask_permission(
        &self,
        session: &Session,
        turn_id: &TurnId,
        call: &PendingCall,
    ) -> Decision {
        let permission_id = PermissionId::new();
        self.emit(
            &session.id,
            EventKind::PermissionRequested {
                turn_id: turn_id.clone(),
                permission_id: permission_id.clone(),
                tool: call.tool.clone(),
                arguments: call.arguments.clone(),
            },
        )
        .await;

        let request = PermissionRequest {
            id: permission_id.clone(),
            session_id: session.id.clone(),
            turn_id: turn_id.clone(),
            tool: call.tool.clone(),
            arguments: call.arguments.clone(),
        };
        let decision = self.approver.decide(&request).await;

        self.emit(
            &session.id,
            EventKind::PermissionResolved {
                turn_id: turn_id.clone(),
                permission_id,
                decision,
            },
        )
        .await;
        decision
    }

    async fn complete_turn(
        &self,
        session: &mut Session,
        turn_id: &TurnId,
    ) -> Result<(), EngineError> {
        let session_id = session.id.clone();
        let usage = match session.turn_mut(turn_id) {
            Some(turn) => {
                turn.status = TurnStatus::Completed;
                self.store.save_turn(&session_id, turn).await?;
                turn.usage
            }
            None => return Ok(()),
        };

        self.emit(
            &session_id,
            EventKind::TurnCompleted {
                turn_id: turn_id.clone(),
                usage,
            },
        )
        .await;
        Ok(())
    }

    async fn fail_turn(
        &self,
        session: &mut Session,
        turn_id: &TurnId,
        reason: String,
    ) -> Result<(), EngineError> {
        let session_id = session.id.clone();
        if let Some(turn) = session.turn_mut(turn_id) {
            turn.status = TurnStatus::Failed {
                reason: reason.clone(),
            };
            self.store.save_turn(&session_id, turn).await?;
        }
        self.emit(
            &session_id,
            EventKind::TurnFailed {
                turn_id: turn_id.clone(),
                reason,
            },
        )
        .await;
        Ok(())
    }

    async fn start_open(&self, session_id: &SessionId, turn_id: &TurnId, kind: PartKind) -> OpenPart {
        let id = PartId::new();
        self.emit(
            session_id,
            EventKind::PartStarted {
                turn_id: turn_id.clone(),
                part_id: id.clone(),
                kind,
            },
        )
        .await;

        match kind {
            PartKind::Reasoning => OpenPart::Reasoning {
                id,
                text: String::new(),
            },
            _ => OpenPart::Text {
                id,
                text: String::new(),
            },
        }
    }

    async fn finish_open(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        open: &mut Option<OpenPart>,
        parts: &mut Vec<Part>,
    ) {
        if let Some(opened) = open.take() {
            let part = opened.into_part();
            self.emit(
                session_id,
                EventKind::PartCompleted {
                    turn_id: turn_id.clone(),
                    part: part.clone(),
                },
            )
            .await;
            parts.push(part);
        }
    }

    async fn emit(&self, session_id: &SessionId, kind: EventKind) {
        let event = Event {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            session_id: session_id.clone(),
            at: now_millis(),
            kind,
        };
        self.events.emit(event).await;
    }
}

struct ModelOutcome {
    tool_calls: Vec<PendingCall>,
    failed: Option<String>,
}

struct PendingCall {
    call_id: ToolCallId,
    tool: String,
    arguments: Value,
}

enum OpenPart {
    Text { id: PartId, text: String },
    Reasoning { id: PartId, text: String },
}

impl OpenPart {
    fn into_part(self) -> Part {
        match self {
            OpenPart::Text { id, text } => Part::Text { id, text },
            OpenPart::Reasoning { id, text } => Part::Reasoning { id, text },
        }
    }
}