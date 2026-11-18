//! Điều phối yêu cầu phê duyệt quyền (AD-07).
//!
//! Engine phát `permission.requested`, chờ [`Approver::decide`], rồi phát
//! `permission.resolved`. Broker chỉ giữ oneshot chờ quyết định từ HTTP; không
//! tự phát event để tránh trùng với engine.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ananke_core::{Approver, Decision, PermissionId, PermissionRequest, SessionId};
use ananke_store::SqliteStore;
use async_trait::async_trait;
use tokio::sync::oneshot;

use crate::hub::EventHub;

/// Chờ client trả lời tối đa 5 phút; quá hạn coi như từ chối.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

pub struct ApprovalBroker {
    pending: Mutex<HashMap<PermissionId, (String, oneshot::Sender<Decision>)>>,
    /// AllowAlways scope persistence: maps tool name to scope string (once, session, project)
    allow_always_scopes: Mutex<HashMap<String, String>>,
    /// Store for persisting AllowAlways scope
    store: Arc<SqliteStore>,
    /// Giữ hub để tầng route mở rộng nếu cần; engine đã phát cả hai event
    /// permission nên broker không emit gì.
    #[allow(dead_code)]
    hub: Arc<EventHub>,
}

impl ApprovalBroker {
    pub fn new(hub: Arc<EventHub>, store: Arc<SqliteStore>) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(HashMap::new()),
            allow_always_scopes: Mutex::new(HashMap::new()),
            store,
            hub,
        })
    }

    /// Gửi quyết định cho lời gọi đang chờ; `false` nếu không có pending.
    /// Now accepts optional scope for AllowAlways persistence.
    pub fn resolve(&self, id: &PermissionId, decision: Decision, scope: Option<String>, session_id: &SessionId) -> bool {
        let entry = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id);

        let (tool_name, sender) = match entry {
            Some(pair) => pair,
            None => return false,
        };

        // Persist scope for AllowAlways decisions to store
        if matches!(decision, Decision::AllowAlways) {
            if let Some(scope_str) = scope {
                if let Err(err) = self.store.save_allow_always_scope(id, session_id, &scope_str) {
                    tracing::error!(permission = %id, error = %err, "không lưu được AllowAlways scope");
                }
                self.allow_always_scopes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(tool_name, scope_str);
            }
        }

        sender.send(decision).is_ok()
    }

    pub fn has_pending(&self, id: &PermissionId) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(id)
    }

    /// Get persisted scope for AllowAlways permission by tool name (for future auto-approval)
    pub fn get_allow_always_scope_for_tool(&self, tool: &str) -> Option<String> {
        self.allow_always_scopes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(tool)
            .cloned()
    }

    /// Get persisted scope for AllowAlways permission by PermissionId (legacy/fallback)
    pub fn get_allow_always_scope(&self, id: &PermissionId) -> Option<String> {
        match self.store.get_allow_always_scope(id) {
            Ok(Some(scope)) => Some(scope),
            _ => None,
        }
    }
}

#[async_trait]
impl Approver for ApprovalBroker {
    async fn decide(&self, request: &PermissionRequest) -> Decision {
        // Check if we have a persisted AllowAlways scope for this tool
        if let Some(scope) = self.get_allow_always_scope_for_tool(&request.tool) {
            tracing::debug!(permission = %request.id, tool = %request.tool, %scope, "auto-allow from persisted scope");
            return Decision::AllowAlways;
        }

        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(request.id.clone(), (request.tool.clone(), sender));

        let decision = match tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await {
            Ok(Ok(decision)) => decision,
            Ok(Err(_)) => Decision::Deny,
            Err(_) => {
                tracing::warn!(
                    permission = %request.id,
                    session = %request.session_id,
                    tool = %request.tool,
                    "hết thời gian chờ phê duyệt, từ chối"
                );
                Decision::Deny
            }
        };

        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&request.id);

        decision
    }
}