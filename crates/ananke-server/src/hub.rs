//! Event hub: gán `seq`, persist event bền vững và broadcast cho SSE.
//!
//! AD-08: `seq` đơn điệu trong một session; `part.delta` chỉ để stream, không
//! bao giờ vào event log. Mỗi session có một kênh broadcast riêng, tạo lười khi
//! có subscriber hoặc event đầu tiên.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ananke_core::{Event, EventKind, EventSink, SessionId};
use ananke_protocol::EventDto;
use ananke_store::SqliteStore;
use async_trait::async_trait;
use tokio::sync::broadcast;

/// Số frame tối đa giữ cho một subscriber chậm; vượt thì nhận `Lagged`.
const CHANNEL_CAPACITY: usize = 1024;

pub struct EventHub {
    store: Arc<SqliteStore>,
    channels: Mutex<HashMap<SessionId, broadcast::Sender<EventDto>>>,
    /// Serialize bước "đọc max seq + append" để hai emit đồng thời không ghi
    /// trùng `(session_id, seq)` (khoá chính của bảng `event`).
    seq_lock: Mutex<()>,
    capacity: usize,
}

impl EventHub {
    pub fn new(store: Arc<SqliteStore>) -> Arc<Self> {
        Arc::new(Self {
            store,
            channels: Mutex::new(HashMap::new()),
            seq_lock: Mutex::new(()),
            capacity: CHANNEL_CAPACITY,
        })
    }

    /// Kênh broadcast của session, tạo mới nếu chưa có.
    fn sender(&self, id: &SessionId) -> broadcast::Sender<EventDto> {
        let mut channels = self
            .channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        channels
            .entry(id.clone())
            .or_insert_with(|| broadcast::channel(self.capacity).0)
            .clone()
    }

    /// Subscribe vào event của một session (SSE handler dùng).
    pub fn subscribe(&self, id: &SessionId) -> broadcast::Receiver<EventDto> {
        self.sender(id).subscribe()
    }

    /// Emit a resource.throttled event for admission control (AD-15).
    pub fn emit_resource_throttled(&self, limit: String, current: u32, action: String) {
        let session_id = SessionId::from("system");
        let event = Event {
            seq: 0, // Will be assigned in record
            session_id: session_id.clone(),
            at: ananke_core::event::now_millis(),
            kind: EventKind::ResourceThrottled {
                limit,
                current,
                action,
            },
        };
        self.record(event);
    }

    /// Atomic: get max_seq from store AND subscribe to channel.
    /// Returns (Receiver, last_seq) atomically to avoid race condition.
    /// This is used by SSE to avoid gap between query and subscribe.
    pub async fn subscribe_from(&self, id: &SessionId, since: u64) -> (broadcast::Receiver<EventDto>, u64) {
        // Get max_seq and create/reuse channel under same lock as record()
        let (sender, max_seq) = {
            let _guard = self
                .seq_lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let max = match self.store.max_event_seq(id) {
                Ok(seq) => seq,
                Err(err) => {
                    tracing::error!(session = %id, error = %err, "không đọc được max event seq");
                    0
                }
            };

            let mut channels = self
                .channels
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let sender = channels
                .entry(id.clone())
                .or_insert_with(|| broadcast::channel(self.capacity).0)
                .clone();

            (sender, max)
        };

        // Subscribe to the channel
        let receiver = sender.subscribe();

        // Replay events from store if since < max_seq
        if since < max_seq {
            let events = match self.store.events_since(id, since) {
                Ok(events) => events,
                Err(err) => {
                    tracing::error!(session = %id, error = %err, "không replay được event từ store");
                    Vec::new()
                }
            };

            for event in events {
                if let Some(dto) = crate::dto::event_to_dto(&event, event.seq) {
                    let _ = sender.send(dto);
                }
            }
        }

        (receiver, max_seq)
    }

    /// Gán `seq`, persist nếu là event bền vững, rồi broadcast.
    ///
    /// `part.delta` không persist: `seq` broadcast của nó chỉ là `max_event_seq`
    /// hiện tại, nên **không đơn điệu tăng và không duy nhất** giữa các delta —
    /// client chỉ dùng nó để hiển thị, còn resync thì dựa vào `/state` và
    /// `last_seq` của event bền vững (AD-08).
    pub fn record(&self, mut event: Event) -> Option<EventDto> {
        let session_id = event.session_id.clone();

        {
            let _guard = self
                .seq_lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let max = match self.store.max_event_seq(&session_id) {
                Ok(seq) => seq,
                Err(err) => {
                    tracing::error!(session = %session_id, error = %err, "không đọc được max event seq");
                    0
                }
            };

            if matches!(event.kind, EventKind::PartDelta { .. }) {
                event.seq = max;
            } else {
                event.seq = max.saturating_add(1);
                if let Err(err) = self.store.append_events(std::slice::from_ref(&event)) {
                    tracing::error!(
                        session = %session_id,
                        seq = event.seq,
                        error = %err,
                        "không ghi được event vào store"
                    );
                }
            }
        }

        let dto = crate::dto::event_to_dto(&event, event.seq)?;
        // Không có subscriber là trạng thái bình thường: bỏ qua `SendError`.
        let _ = self.sender(&session_id).send(dto.clone());
        Some(dto)
    }
}

#[async_trait]
impl EventSink for EventHub {
    async fn emit(&self, event: Event) {
        self.record(event);
    }
}