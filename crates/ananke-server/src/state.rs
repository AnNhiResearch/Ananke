//! Trạng thái chia sẻ giữa các handler HTTP/SSE.
//!
//! `AppState` chỉ chứa handle `Arc`: clone rẻ và mọi handler dùng chung đúng
//! một engine, một store và một registry session đang sống trong RAM.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ananke_core::{Engine, Session, SessionId, StoreError, TurnId, TurnStatus};
use ananke_protocol::{LimitsDto, MetaResponse};
use ananke_provider::DynamicModelRegistry;
use ananke_store::SqliteStore;
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use tokio::task::JoinHandle;

use crate::approval::ApprovalBroker;
use crate::config::Config;
use crate::hub::EventHub;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub engine: Arc<Engine>,
    pub store: Arc<SqliteStore>,
    pub hub: Arc<EventHub>,
    pub sessions: Arc<SessionRegistry>,
    pub approval: Arc<ApprovalBroker>,
    pub limits: Arc<RwLock<LimitsDto>>,
    pub meta: Arc<MetaResponse>,
    pub models: Arc<AsyncMutex<DynamicModelRegistry>>,
    pub running_tasks: Arc<AsyncMutex<HashMap<TurnId, JoinHandle<()>>>>,
}

type SessionHandle = Arc<AsyncMutex<Session>>;

/// Các session đang nạp trong RAM. Persist vẫn nằm ở `SqliteStore`; registry chỉ
/// giữ state sống để turn đang chạy không bị nạp lại giữa chừng (AD-15).
#[derive(Default)]
pub struct SessionRegistry {
    inner: AsyncMutex<HashMap<SessionId, SessionHandle>>,
    /// Global counter of active turns (Running or AwaitingTool) across all sessions.
    /// Incremented when a turn starts, decremented when it ends. Used for admission control AD-15.
    active_turns: AtomicUsize,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    async fn lock(&self) -> tokio::sync::MutexGuard<'_, HashMap<SessionId, SessionHandle>> {
        self.inner.lock().await
    }

    pub async fn get(&self, id: &SessionId) -> Option<SessionHandle> {
        self.lock().await.get(id).cloned()
    }

    pub async fn contains(&self, id: &SessionId) -> bool {
        self.lock().await.contains_key(id)
    }

    /// Đưa session vào RAM, trả handle dùng chung. Nếu đã có thì ghi đè.
    pub async fn insert(&self, session: Session) -> SessionHandle {
        let id = session.id.clone();
        let handle = Arc::new(AsyncMutex::new(session));
        self.lock().await.insert(id, Arc::clone(&handle));
        handle
    }

    pub async fn remove(&self, id: &SessionId) -> bool {
        self.lock().await.remove(id).is_some()
    }

    /// Count resident sessions in RAM (for admission control AD-15)
    pub async fn resident_session_count(&self) -> usize {
        self.lock().await.len()
    }

    /// Count active turns (Running or AwaitingTool) for a session
    pub async fn active_turn_count(&self, session_id: &SessionId) -> usize {
        if let Some(handle) = self.lock().await.get(session_id) {
            let session = handle.lock().await;
            session.turns().iter()
                .filter(|turn| matches!(turn.status, TurnStatus::Running | TurnStatus::AwaitingTool))
                .count()
        } else {
            0
        }
    }

    /// Count active turns (Running or AwaitingTool) globally across all sessions (for admission control AD-15)
    /// Uses atomic counter for lock-free, fast access.
    pub fn global_active_turn_count(&self) -> usize {
        self.active_turns.load(Ordering::Acquire)
    }

    /// Increment the global active turns counter (called when a turn starts)
    pub fn increment_active_turns(&self) {
        self.active_turns.fetch_add(1, Ordering::AcqRel);
    }

    /// Decrement the global active turns counter (called when a turn ends)
    pub fn decrement_active_turns(&self) {
        self.active_turns.fetch_sub(1, Ordering::AcqRel);
    }

    /// Lấy session từ RAM; nếu chưa có thì nạp từ store.
    ///
    /// `load_session` trả `NotFound` khi session chưa được lưu → `Ok(None)`;
    /// lỗi store thật (DB hỏng, I/O) được trả về để handler thành 500 thay vì
    /// 404 sai.
    pub async fn get_or_load(
        &self,
        store: &SqliteStore,
        id: &SessionId,
    ) -> Result<Option<SessionHandle>, StoreError> {
        // Double-checked locking pattern to avoid race
        if let Some(existing) = self.get(id).await {
            return Ok(Some(existing));
        }

        match store.load_session(id) {
            Ok(session) => {
                let handle = Arc::new(AsyncMutex::new(session));
                // Nạp đồng thời: giữ handle đã có để không mất state của thread kia.
                let mut inner = self.lock().await;
                let existing = inner.entry(id.clone()).or_insert(handle);
                Ok(Some(Arc::clone(existing)))
            }
            Err(StoreError::NotFound(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }
}