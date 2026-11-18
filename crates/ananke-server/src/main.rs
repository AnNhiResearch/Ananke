//! Entry point binary `ananke-server`.

use std::sync::Arc;
use std::time::Duration;

use ananke_core::{Approver, Engine, EventSink, ModelProvider, SessionStore, ToolRegistry};
use ananke_protocol::LimitsDto;
use ananke_provider::{OpenAiCompatConfig, OpenAiCompatProvider};
use ananke_server::{
    approval::ApprovalBroker, config::Config, dto, hub::EventHub, router,
    state::{AppState, SessionRegistry},
};
use ananke_store::SqliteStore;
use ananke_tools::ToolPolicy;
use tokio::signal;
use tokio::sync::{broadcast, RwLock};

/// Thời gian chờ tối đa cho các turn đang chạy khi shutdown.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;

    let store = Arc::new(SqliteStore::open(&config.db_path)?);
    let hub = EventHub::new(Arc::clone(&store));
    let approval = ApprovalBroker::new(Arc::clone(&hub), Arc::clone(&store));

    // Có `ANANKE_BASE_URL` thì dùng endpoint đó; không thì Kilo Gateway ẩn danh
    // (không cần API key). Thiếu key không chặn khởi động — chỉ lỗi khi gọi model.
    let provider: Arc<dyn ModelProvider> = match config.base_url.clone() {
        Some(base_url) => Arc::new(OpenAiCompatProvider::new(OpenAiCompatConfig {
            base_url,
            api_key: config.api_key.clone(),
            headers: Vec::new(),
        })),
        None => Arc::new(OpenAiCompatProvider::kilo()),
    };

    // Policy của tool khớp permission mode mặc định của engine (`WorkspaceWrite`):
    // tool mutating sẽ đi qua approval, ghi file chỉ trong workspace.
    let base = std::env::current_dir()?;
    let mut tools = ToolRegistry::new();
    ananke_tools::register_builtin(
        &mut tools,
        ToolPolicy::workspace_write(base.clone(), vec![base]),
    )?;
    let tools = Arc::new(tools);

    let engine = Arc::new(Engine::new(
        provider,
        tools,
        Arc::clone(&approval) as Arc<dyn Approver>,
        Arc::clone(&hub) as Arc<dyn EventSink>,
        Arc::clone(&store) as Arc<dyn SessionStore>,
    ));

    let limits = Arc::new(RwLock::new(LimitsDto::for_profile(
        dto::device_profile_dto(ananke_sandbox::device_profile()),
    )));

    let state = AppState {
        config: Arc::new(config.clone()),
        engine,
        store: Arc::clone(&store),
        hub: Arc::clone(&hub),
        sessions: Arc::new(SessionRegistry::new()),
        approval,
        limits,
        meta: Arc::new(dto::meta_response()),
        running_tasks: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    };

    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;

    println!("ananke-server listening on {}", config.bind_addr);

    // Channel để signal shutdown
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);

    // Spawn signal handler
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("failed to listen for ctrl_c");
        println!("Received shutdown signal, starting graceful shutdown...");
        let _ = shutdown_tx_clone.send(());
    });

    // Run server with graceful shutdown
    let serve_future = axum::serve(listener, app);
    let graceful = serve_future.with_graceful_shutdown(async move {
        let _ = shutdown_rx.recv().await;
        
        // Wait for in-flight operations to complete
        println!("Waiting for in-flight turns to complete (max {}s)...", SHUTDOWN_TIMEOUT.as_secs());
        
        // Give some time for ongoing turns to finish
        tokio::time::sleep(SHUTDOWN_TIMEOUT).await;
        
        // Flush store (checkpoint WAL)
        println!("Flushing database...");
        if let Err(e) = store.checkpoint() {
            eprintln!("Failed to checkpoint database: {}", e);
        }
        
        println!("Graceful shutdown complete");
    });

    graceful.await?;
    Ok(())
}