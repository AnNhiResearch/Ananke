//! `GET /api/session/{id}/event` (alias `/events`) — SSE replay + live (AD-08).
//!
//! Replay đọc từ event log theo `since`/`Last-Event-ID`; sau đó stream tiếp các
//! `EventDto` do hub broadcast. `part.delta` chỉ có ở phần live vì không persist.

use std::convert::Infallible;
use std::time::Duration;

use ananke_protocol::EventDto;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures_util::Stream;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use crate::error::ApiResult;
use crate::routes::{internal, not_found, session_id};
use crate::state::AppState;

/// Ping định kỳ để proxy/NAT không cắt kết nối rảnh.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    since: Option<u64>,
}

/// Mở stream event của session; 404 nếu session chưa từng tồn tại.
pub async fn events(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> ApiResult<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
    let id = session_id(&raw_id);

    let exists = state.store.session_exists(&id).map_err(internal)?;
    if !exists {
        return Err(not_found(&id));
    }

    let since = query
        .since
        .unwrap_or(0)
        .max(last_event_id(&headers).unwrap_or(0));

    // Atomic subscribe from sequence - get receiver that replays from `since`
    // and returns the actual last_seq for race detection
    let (mut receiver, actual_last_seq) = state.hub.subscribe_from(&id, since).await;
    let log_id = id.to_string();

    let stream = async_stream::stream! {
        let mut last_seen_seq = 0u64;
        let mut first_event = true;
        
        loop {
            match receiver.recv().await {
                Ok(dto) => {
                    // Deduplication: skip if we've already seen this seq
                    if dto.seq <= last_seen_seq {
                        continue;
                    }
                    
                    // Check for gap (resync_required) - only on first live event after replay
                    // Gap exists if there are missing sequences between client's since and server's oldest
                    if first_event && dto.seq > since + 1 && since > 0 {
                        // There's a gap between client's last_event_id and server's oldest available
                        let resync_event = EventDto {
                            seq: dto.seq, // Use current seq for resync event
                            session_id: dto.session_id.clone(),
                            at: ananke_core::event::now_millis(),
                            payload: ananke_protocol::event::EventPayloadDto::EventResyncRequired {
                                last_server_seq: actual_last_seq,
                                reason: format!("gap detected: client has {}, server oldest is {}", since + 1, dto.seq),
                            },
                        };
                        if let Ok(frame) = frame(&resync_event) {
                            yield Ok(frame);
                        }
                    }
                    first_event = false;
                    last_seen_seq = dto.seq;
                    
                    match frame(&dto) {
                        Ok(frame) => yield Ok(frame),
                        Err(err) => {
                            tracing::error!(session = %log_id, error = %err, "không serialize được event live");
                            return;
                        }
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(session = %log_id, skipped, "SSE subscriber bị lag, bỏ qua event");
                }
                Err(RecvError::Closed) => return,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE_INTERVAL)))
}

/// Một frame SSE: `id` là `seq` để client replay bằng `Last-Event-ID`, `data`
/// là envelope `EventDto` đã serialize.
fn frame(dto: &EventDto) -> Result<SseEvent, serde_json::Error> {
    Ok(SseEvent::default()
        .id(dto.seq.to_string())
        .data(serde_json::to_string(dto)?))
}

/// `Last-Event-ID` không hợp lệ bị bỏ qua để stream vẫn mở được.
fn last_event_id(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("last-event-id")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}