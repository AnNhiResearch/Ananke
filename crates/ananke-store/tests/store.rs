//! Test hợp đồng của `ananke-store`: round-trip, event log, retention và AD-17.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ananke_core::{
    Event, EventKind, Part, Session, SessionId, SessionStore, StoreError, ToolCallId, Turn,
    TurnId, TurnStatus, Usage,
};
use ananke_store::SqliteStore;
use serde_json::json;

fn event(session_id: &SessionId, seq: u64, kind: EventKind) -> Event {
    Event {
        seq,
        session_id: session_id.clone(),
        at: 1_700_000_000_000 + seq,
        kind,
    }
}

fn turn_with_tool_call() -> Turn {
    let mut turn = Turn::new();
    turn.parts.push(Part::text("đọc file giúp mình"));
    turn.parts.push(Part::tool_call(
        ToolCallId::from("call_1"),
        "read_file",
        json!({ "path": "README.md" }),
    ));
    turn.usage = Usage {
        input_tokens: 12,
        output_tokens: 3,
    };
    turn
}

fn temp_db(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "ananke-store-{name}-{}-{nanos}.sqlite",
        std::process::id()
    ))
}

fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

#[tokio::test]
async fn round_trip_turn() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let session_id = SessionId::from("ses_test");
    let turn = turn_with_tool_call();

    store.save_turn(&session_id, &turn).await.expect("lưu turn");

    let loaded = store.load_turns(&session_id).await.expect("nạp turn");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, turn.id);
    assert_eq!(loaded[0].status, TurnStatus::Running);
    assert_eq!(loaded[0].usage, turn.usage);
    assert_eq!(
        serde_json::to_value(&loaded[0].parts).expect("parts"),
        serde_json::to_value(&turn.parts).expect("parts")
    );
}

#[tokio::test]
async fn save_turn_lan_hai_upsert_khong_nhan_ban() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let session_id = SessionId::from("ses_test");
    let mut turn = turn_with_tool_call();

    store.save_turn(&session_id, &turn).await.expect("lưu lần 1");

    turn.parts.push(Part::tool_result(
        ToolCallId::from("call_1"),
        "read_file",
        true,
        "nội dung",
    ));
    store.save_turn(&session_id, &turn).await.expect("lưu lần 2");

    let loaded = store.load_turns(&session_id).await.expect("nạp turn");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].parts.len(), 3);
}

#[tokio::test]
async fn set_turn_status_tra_not_found_khi_thieu_turn() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let err = store
        .set_turn_status(
            &SessionId::from("ses_test"),
            &TurnId::from("trn_khong_co"),
            &TurnStatus::Completed,
        )
        .await
        .expect_err("phải NotFound");

    assert!(matches!(err, StoreError::NotFound(_)), "nhận {err:?}");
}

#[tokio::test]
async fn set_turn_status_cap_nhat_trang_thai() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let session_id = SessionId::from("ses_test");
    let turn = turn_with_tool_call();
    store.save_turn(&session_id, &turn).await.expect("lưu turn");

    store
        .set_turn_status(&session_id, &turn.id, &TurnStatus::AwaitingTool)
        .await
        .expect("đổi trạng thái");

    let loaded = store.load_turns(&session_id).await.expect("nạp turn");
    assert_eq!(loaded[0].status, TurnStatus::AwaitingTool);
}

#[tokio::test]
async fn part_delta_khong_vao_event_log() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let session_id = SessionId::from("ses_test");
    let turn_id = TurnId::from("trn_1");
    let part_id = ananke_core::PartId::from("prt_1");

    let written = store
        .append_events(&[
            event(
                &session_id,
                1,
                EventKind::TurnStarted {
                    turn_id: turn_id.clone(),
                },
            ),
            event(
                &session_id,
                2,
                EventKind::PartDelta {
                    turn_id: turn_id.clone(),
                    part_id: part_id.clone(),
                    kind: ananke_core::PartKind::Text,
                    delta: "xin chào".into(),
                },
            ),
            event(
                &session_id,
                3,
                EventKind::PartCompleted {
                    turn_id,
                    part: Part::Text {
                        id: part_id,
                        text: "xin chào".into(),
                    },
                },
            ),
        ])
        .expect("append");

    assert_eq!(written, 2, "chỉ 2 event persistent được ghi");

    let replayed = store.events_since(&session_id, 0).expect("replay");
    assert_eq!(replayed.len(), 2);
    assert!(replayed
        .iter()
        .all(|event| !matches!(event.kind, EventKind::PartDelta { .. })));
    assert_eq!(replayed[1].seq, 3);
}

#[tokio::test]
async fn replay_since_tra_ve_event_sau_moc() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let session_id = SessionId::from("ses_test");

    let events: Vec<Event> = (1..=5)
        .map(|seq| {
            event(
                &session_id,
                seq,
                EventKind::Warning {
                    message: format!("cảnh báo {seq}"),
                },
            )
        })
        .collect();
    store.append_events(&events).expect("append");

    let replayed = store.events_since(&session_id, 2).expect("replay");
    assert_eq!(
        replayed.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
}

#[tokio::test]
async fn prune_events_giu_lai_cuoi() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let session_id = SessionId::from("ses_test");

    let events: Vec<Event> = (1..=5)
        .map(|seq| {
            event(
                &session_id,
                seq,
                EventKind::Warning {
                    message: format!("cảnh báo {seq}"),
                },
            )
        })
        .collect();
    store.append_events(&events).expect("append");

    let removed = store.prune_events(&session_id, 2).expect("prune");
    assert_eq!(removed, 3);
    assert_eq!(
        store
            .events_since(&session_id, 0)
            .expect("replay")
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        vec![4, 5]
    );
}

#[tokio::test]
async fn save_turn_with_events_rollback_khi_event_loi() {
    let store = SqliteStore::open_in_memory().expect("mở store");
    let session_id = SessionId::from("ses_test");
    let turn = turn_with_tool_call();

    // Hai event cùng seq vi phạm PRIMARY KEY (session_id, seq): cả transaction phải rollback.
    let duplicate = vec![
        event(
            &session_id,
            7,
            EventKind::Warning {
                message: "một".into(),
            },
        ),
        event(
            &session_id,
            7,
            EventKind::Warning {
                message: "hai".into(),
            },
        ),
    ];

    assert!(store
        .save_turn_with_events(&session_id, &turn, &duplicate)
        .is_err());

    assert!(
        store.load_turns(&session_id).await.expect("nạp turn").is_empty(),
        "turn phải bị rollback cùng event"
    );
    assert!(
        store.events_since(&session_id, 0).expect("replay").is_empty(),
        "không được có event ma"
    );
}

#[tokio::test]
async fn turn_awaiting_tool_phuc_hoi_sau_restart() {
    let path = temp_db("restart");
    let session_id;

    {
        let store = SqliteStore::open(&path).expect("mở store");
        let session = Session::new("kilo-auto/free").with_title("phiên thử");
        session_id = session.id.clone();
        store.save_session(&session).expect("lưu session");

        let mut turn = turn_with_tool_call();
        turn.status = TurnStatus::AwaitingTool;
        store.save_turn(&session_id, &turn).await.expect("lưu turn");
    }

    // Store cũ đã drop: tương đương process chết rồi mở lại cùng data dir.
    let store = SqliteStore::open(&path).expect("mở lại store");

    let session = store.load_session(&session_id).expect("nạp session");
    assert_eq!(session.title, "phiên thử");
    assert_eq!(session.model, "kilo-auto/free");

    let turns = store.load_turns(&session_id).await.expect("nạp turn");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].status, TurnStatus::AwaitingTool);

    let pending = turns[0].pending_tool_calls();
    assert_eq!(pending.len(), 1, "turn phải khôi phục được lời gọi tool");
    assert_eq!(pending[0].1, "read_file");

    cleanup(&path);
}

#[tokio::test]
async fn checkpoint_va_background_prune_hoat_dong() {
    let path = temp_db("checkpoint_prune");
    let retention = ananke_store::RetentionConfig {
        max_events_per_session: 3,
        max_event_age_days: 30,
        prune_interval_secs: 1,
        enable_auto_prune: false,
    };

    let mut store = SqliteStore::new_with_config(&path, retention).expect("mở store với config");
    let session = Session::new("model-test");
    let session_id = session.id.clone();
    store.save_session(&session).expect("save session");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let events: Vec<Event> = (1..=6)
        .map(|seq| Event {
            seq,
            session_id: session_id.clone(),
            at: now_ms + seq,
            kind: EventKind::Warning {
                message: format!("chk {seq}"),
            },
        })
        .collect();
    store.append_events(&events).expect("append events");

    // Test checkpoint WAL
    assert!(store.checkpoint().is_ok(), "checkpoint WAL phải thành công");

    // Test prune_old_events theo config (max 3 events)
    let removed = store.prune_old_events().expect("prune old events");
    assert_eq!(removed, 3, "phải dọn 3 event cũ vượt ngưỡng");

    let remaining = store.events_since(&session_id, 0).expect("events since 0");
    assert_eq!(remaining.len(), 3);
    assert_eq!(remaining[0].seq, 4);
    assert_eq!(remaining[2].seq, 6);

    store.shutdown();
    cleanup(&path);
}
