//! Test gọi **mạng thật** tới Kilo Gateway với model `kilo-auto/free` (ẩn danh).
//!
//! Chạy tay: `cargo test -p ananke-provider --test kilo_live -- --ignored --nocapture`
//! Các test này bị `#[ignore]` để `cargo test` thường không phụ thuộc mạng.

use std::sync::Arc;

use ananke_core::{
    AllowAll, Engine, InMemoryStore, MemoryEventSink, Message, ModelProvider, PermissionMode,
    ProviderEvent, ProviderRequest, Role, Session, ToolRegistry,
};
use ananke_provider::OpenAiCompatProvider;
use futures_util::StreamExt;

#[tokio::test]
#[ignore = "gọi mạng thật tới Kilo Gateway"]
async fn kilo_anonymous_tra_loi_van_ban() {
    let provider = OpenAiCompatProvider::kilo();
    let request = ProviderRequest {
        model: "kilo-auto/free".into(),
        messages: vec![Message::text(Role::User, "Answer with exactly one word: ping")],
        tools: vec![],
    };

    let mut stream = provider.stream(request).await.expect("mở stream");
    let mut text = String::new();
    let mut done = false;
    while let Some(event) = stream.next().await {
        match event {
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::ReasoningDelta(_) => {}
            ProviderEvent::Done { .. } => {
                done = true;
                break;
            }
            ProviderEvent::Failed { reason } => panic!("provider lỗi: {reason}"),
            ProviderEvent::ToolCall { .. } => {}
        }
    }

    assert!(done, "stream phải kết thúc bằng Done");
    assert!(!text.trim().is_empty(), "phải có text trả về");
    println!("kilo trả lời: {text}");
}

#[tokio::test]
#[ignore = "gọi mạng thật tới Kilo Gateway"]
async fn kilo_engine_chay_het_mot_turn() {
    let engine = Engine::new(
        Arc::new(OpenAiCompatProvider::kilo()),
        Arc::new(ToolRegistry::default()),
        Arc::new(AllowAll),
        Arc::new(MemoryEventSink::default()),
        Arc::new(InMemoryStore::default()),
    )
    .with_permission(PermissionMode::DangerFullAccess);

    let mut session = Session::new("kilo-auto/free");
    let turn_id = engine
        .run_turn(
            &mut session,
            vec![Message::text(Role::User, "Say hi in one short sentence.")],
        )
        .await
        .expect("run_turn");

    let turn = session.turn(&turn_id).expect("turn tồn tại");
    assert!(
        matches!(turn.status, ananke_core::TurnStatus::Completed),
        "turn phải Completed, nhận {:?}",
        turn.status
    );
    assert!(!turn.text().trim().is_empty(), "turn phải có text");
    println!("engine turn text: {}", turn.text());
}