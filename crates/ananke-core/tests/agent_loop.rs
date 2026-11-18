use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ananke_core::{
    AllowAll, Approver, Decision, Engine, EventKind, InMemoryStore, MemoryEventSink, Message,
    ModelProvider, Part, PartKind, PermissionMode, PermissionRequest, ProviderError, ProviderEvent,
    ProviderRequest, ProviderStream, Role, Session, SessionStore, Tool, ToolCallId, ToolOutput,
    ToolRegistry, Turn, TurnStatus, Usage,
};
use async_trait::async_trait;
use serde_json::{json, Value};

struct ScriptedProvider {
    scripts: Mutex<VecDeque<Vec<ProviderEvent>>>,
}

impl ScriptedProvider {
    fn new(scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            scripts: Mutex::new(scripts.into()),
        }
    }
}

#[async_trait]
impl ModelProvider for ScriptedProvider {
    async fn stream(&self, _request: ProviderRequest) -> Result<ProviderStream, ProviderError> {
        let events = self
            .scripts
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default();
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

struct EchoTool {
    mutating: bool,
}

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Trả lại tham số text"
    }

    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": { "text": { "type": "string" } } })
    }

    fn mutating(&self) -> bool {
        self.mutating
    }

    async fn execute(&self, arguments: Value) -> ToolOutput {
        ToolOutput::ok(
            arguments
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        )
    }
}

struct DenyAll;

#[async_trait]
impl Approver for DenyAll {
    async fn decide(&self, _request: &PermissionRequest) -> Decision {
        Decision::Deny
    }
}

fn engine_with(
    provider: ScriptedProvider,
    tool: Option<Arc<dyn Tool>>,
    approver: Arc<dyn Approver>,
    permission: PermissionMode,
    events: Arc<MemoryEventSink>,
) -> Engine {
    let mut registry = ToolRegistry::new();
    if let Some(tool) = tool {
        registry.register(tool).expect("đăng ký tool");
    }
    Engine::new(
        Arc::new(provider),
        Arc::new(registry),
        approver,
        events,
        Arc::new(InMemoryStore::new()),
    )
    .with_permission(permission)
}

#[tokio::test]
async fn stream_text_hoan_tat_turn() {
    let events = Arc::new(MemoryEventSink::new());
    let engine = engine_with(
        ScriptedProvider::new(vec![vec![
            ProviderEvent::TextDelta("Xin ".into()),
            ProviderEvent::TextDelta("chào".into()),
            ProviderEvent::Done {
                usage: Usage {
                    input_tokens: 3,
                    output_tokens: 2,
                },
            },
        ]]),
        None,
        Arc::new(AllowAll),
        PermissionMode::WorkspaceWrite,
        events.clone(),
    );

    let mut session = Session::new("mock");
    let turn_id = engine
        .run_turn(&mut session, vec![Message::text(Role::User, "hi")])
        .await
        .expect("chạy turn");

    let turn = session.turn(&turn_id).expect("turn tồn tại");
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(turn.text(), "Xin chào");
    assert_eq!(turn.usage.output_tokens, 2);

    // user + assistant
    assert_eq!(session.messages().len(), 2);
    assert_eq!(session.messages()[1].role, Role::Assistant);

    let snapshot = events.snapshot();
    assert!(snapshot.iter().any(|event| matches!(
        event.kind,
        EventKind::PartStarted {
            kind: PartKind::Text,
            ..
        }
    )));
    assert_eq!(
        snapshot
            .iter()
            .filter(|event| matches!(event.kind, EventKind::PartDelta { .. }))
            .count(),
        2
    );
    assert!(snapshot
        .iter()
        .any(|event| matches!(event.kind, EventKind::TurnCompleted { .. })));
    // seq toàn phần, tăng dần trong session
    assert!(snapshot.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    // part.delta không được persist (AD-08)
    assert!(snapshot
        .iter()
        .all(|event| event.kind.is_persistent() == !matches!(event.kind, EventKind::PartDelta { .. })));
}

#[tokio::test]
async fn tool_call_duoc_chay_va_ket_qua_quay_lai_model() {
    let events = Arc::new(MemoryEventSink::new());
    let call_id = ToolCallId::new();
    let engine = engine_with(
        ScriptedProvider::new(vec![
            vec![
                ProviderEvent::TextDelta("Đang chạy ".into()),
                ProviderEvent::ToolCall {
                    call_id: call_id.clone(),
                    tool: "echo".into(),
                    arguments: json!({ "text": "kết quả" }),
                },
                ProviderEvent::Done {
                    usage: Usage::default(),
                },
            ],
            vec![
                ProviderEvent::TextDelta("xong".into()),
                ProviderEvent::Done {
                    usage: Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                    },
                },
            ],
        ]),
        Some(Arc::new(EchoTool { mutating: true })),
        Arc::new(AllowAll),
        PermissionMode::WorkspaceWrite,
        events.clone(),
    );

    let mut session = Session::new("mock");
    let turn_id = engine
        .run_turn(&mut session, vec![Message::text(Role::User, "chạy echo")])
        .await
        .expect("chạy turn");

    let turn = session.turn(&turn_id).expect("turn tồn tại");
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(turn.text(), "Đang chạy xong");
    assert!(turn.pending_tool_calls().is_empty());

    let (result_call_id, ok, output) = turn
        .parts
        .iter()
        .find_map(|part| match part {
            Part::ToolResult {
                call_id,
                ok,
                output,
                ..
            } => Some((call_id, *ok, output.as_str())),
            _ => None,
        })
        .expect("có ToolResult");
    assert_eq!(result_call_id, &call_id);
    assert!(ok);
    assert_eq!(output, "kết quả");

    let snapshot = events.snapshot();
    assert!(snapshot
        .iter()
        .any(|event| matches!(event.kind, EventKind::TurnAwaitingTool { .. })));
    assert!(snapshot
        .iter()
        .any(|event| matches!(event.kind, EventKind::PermissionRequested { .. })));
    assert!(snapshot
        .iter()
        .any(|event| matches!(event.kind, EventKind::PermissionResolved { .. })));
}

#[tokio::test]
async fn tu_choi_permission_thi_tool_khong_chay() {
    let engine = engine_with(
        ScriptedProvider::new(vec![vec![
            ProviderEvent::ToolCall {
                call_id: ToolCallId::new(),
                tool: "echo".into(),
                arguments: json!({ "text": "kết quả" }),
            },
            ProviderEvent::Done {
                usage: Usage::default(),
            },
        ]]),
        Some(Arc::new(EchoTool { mutating: true })),
        Arc::new(DenyAll),
        PermissionMode::WorkspaceWrite,
        Arc::new(MemoryEventSink::new()),
    );

    let mut session = Session::new("mock");
    let turn_id = engine
        .run_turn(&mut session, vec![Message::text(Role::User, "chạy echo")])
        .await
        .expect("chạy turn");

    let turn = session.turn(&turn_id).expect("turn tồn tại");
    let (ok, output) = turn
        .parts
        .iter()
        .find_map(|part| match part {
            Part::ToolResult { ok, output, .. } => Some((*ok, output.as_str())),
            _ => None,
        })
        .expect("có ToolResult");
    assert!(!ok);
    assert!(output.contains("từ chối"));
}

#[tokio::test]
async fn loi_provider_lam_turn_failed() {
    let events = Arc::new(MemoryEventSink::new());
    let engine = engine_with(
        ScriptedProvider::new(vec![vec![ProviderEvent::Failed {
            reason: "hết quota".into(),
        }]]),
        None,
        Arc::new(AllowAll),
        PermissionMode::WorkspaceWrite,
        events.clone(),
    );

    let mut session = Session::new("mock");
    let turn_id = engine
        .run_turn(&mut session, vec![Message::text(Role::User, "hi")])
        .await
        .expect("engine không trả lỗi cứng");

    let turn = session.turn(&turn_id).expect("turn tồn tại");
    match &turn.status {
        TurnStatus::Failed { reason } => assert!(reason.contains("hết quota")),
        other => panic!("mong đợi Failed, nhận {other:?}"),
    }
    // Không ghi assistant message dở dang vào lịch sử
    assert_eq!(session.messages().len(), 1);
    assert!(events
        .snapshot()
        .iter()
        .any(|event| matches!(event.kind, EventKind::TurnFailed { .. })));
}

#[tokio::test]
async fn resume_turn_hoan_tat_turn_awaiting_tool() {
    let store = Arc::new(InMemoryStore::new());
    let events = Arc::new(MemoryEventSink::new());
    let mut registry = ToolRegistry::new();
    registry
        .register(Arc::new(EchoTool { mutating: false }))
        .expect("đăng ký tool");
    let engine = Engine::new(
        Arc::new(ScriptedProvider::new(vec![vec![
            ProviderEvent::TextDelta("hoàn tất".into()),
            ProviderEvent::Done {
                usage: Usage::default(),
            },
        ]])),
        Arc::new(registry),
        Arc::new(AllowAll),
        events.clone(),
        store.clone(),
    );

    // Mô phỏng process chết khi turn đã persist ở AwaitingTool (AD-17).
    let mut session = Session::new("mock");
    let mut turn = Turn::new();
    let turn_id = turn.id.clone();
    turn.status = TurnStatus::AwaitingTool;
    turn.parts.push(Part::tool_call(
        ToolCallId::new(),
        "echo",
        json!({ "text": "khôi phục" }),
    ));
    session.push_turn(turn.clone());
    store
        .save_turn(&session.id, &turn)
        .await
        .expect("persist turn");

    engine
        .resume_turn(&mut session, &turn_id)
        .await
        .expect("khôi phục turn");

    let turn = session.turn(&turn_id).expect("turn tồn tại");
    assert_eq!(turn.status, TurnStatus::Completed);
    assert!(turn
        .parts
        .iter()
        .any(|part| matches!(part, Part::ToolResult { ok: true, .. })));
    assert_eq!(turn.text(), "hoàn tất");

    let saved = store.load_turns(&session.id).await.expect("đọc lại turn");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].status, TurnStatus::Completed);
}