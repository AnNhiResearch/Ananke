use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::ids::{SessionId, TurnId};
use crate::turn::{Turn, TurnStatus};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("không tìm thấy: {0}")]
    NotFound(String),
    #[error("store lỗi: {0}")]
    Failed(String),
}

/// Bền hoá turn (AD-09). SQLite sẽ cài đặt trait này ở crate `ananke-store`;
/// core chỉ cần hợp đồng: ghi turn trước khi dispatch tool (AD-17).
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn save_turn(&self, session_id: &SessionId, turn: &Turn) -> Result<(), StoreError>;

    async fn load_turns(&self, session_id: &SessionId) -> Result<Vec<Turn>, StoreError>;

    async fn set_turn_status(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        status: &TurnStatus,
    ) -> Result<(), StoreError>;
}

/// Bản trong bộ nhớ: đủ cho test và cho chế độ không cần bền hoá.
#[derive(Default)]
pub struct InMemoryStore {
    turns: Mutex<HashMap<SessionId, Vec<Turn>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Vec<Turn>>> {
        self.turns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait]
impl SessionStore for InMemoryStore {
    async fn save_turn(&self, session_id: &SessionId, turn: &Turn) -> Result<(), StoreError> {
        let mut turns = self.lock();
        let entry = turns.entry(session_id.clone()).or_default();
        match entry.iter_mut().find(|saved| saved.id == turn.id) {
            Some(saved) => *saved = turn.clone(),
            None => entry.push(turn.clone()),
        }
        Ok(())
    }

    async fn load_turns(&self, session_id: &SessionId) -> Result<Vec<Turn>, StoreError> {
        Ok(self.lock().get(session_id).cloned().unwrap_or_default())
    }

    async fn set_turn_status(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        status: &TurnStatus,
    ) -> Result<(), StoreError> {
        let mut turns = self.lock();
        let turn = turns
            .get_mut(session_id)
            .and_then(|session_turns| session_turns.iter_mut().find(|turn| &turn.id == turn_id));

        match turn {
            Some(turn) => {
                turn.status = status.clone();
                Ok(())
            }
            None => Err(StoreError::NotFound(format!("turn {turn_id}"))),
        }
    }
}