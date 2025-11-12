//! E2E tests for ZooKeeper server
#![cfg(all(test, feature = "zookeeper"))]

use netget::state::app_state::AppState;
use netget::state::{ProtocolConfig, ServerId, ServerInstance, StackName};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Helper to start ZooKeeper server and return port
async fn start_zookeeper_server(instruction: &str) -> (Arc<AppState>, ServerId, u16) {
    let state = Arc::new(AppState::new());

    // Create server with available port
    let port = crate::helpers::port_manager::get_available_port();

    let server = ServerInstance {
        id: ServerId::new(1),
        protocol_name: "ZooKeeper".to_string(),
        stack_name: StackName::Application,
        port,
        status: netget::state::server::ServerStatus::Stopped,
        instruction: Some(instruction.to_string()),
        config: ProtocolConfig::default(),
        scheduled_tasks: vec![],
    };

    let server_id = server.id;
    state.add_server(server).await;

    // Start server
    let (status_tx, _status_rx) = tokio::sync::mpsc::unbounded_channel();
    let llm_client = netget::llm::ollama_client::OllamaClient::new_for_tests_with_lock().await;

    netget::cli::server_startup::start_server_by_id(&state, server_id, &llm_client, &status_tx)
        .await
        .expect("Failed to start server");

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    (state, server_id, port)
}

#[tokio::test]
#[ignore] // Requires Ollama
async fn test_zookeeper_basic_connection() {
    let instruction = "Act as a ZooKeeper server. When clients connect, respond with session established.";

    let (_state, _server_id, port) = start_zookeeper_server(instruction).await;

    // Note: Full E2E test would require zookeeper-async client
    // For now, just verify server starts successfully
    println!("ZooKeeper server started on port {}", port);

    // In a full implementation:
    // let zk = zookeeper_async::ZooKeeper::connect(&format!("localhost:{}", port)).await?;
    // assert!(zk.is_connected());
}

#[tokio::test]
#[ignore] // Requires Ollama and full implementation
async fn test_zookeeper_get_data() {
    let instruction = r#"Act as a ZooKeeper server.
When clients read /test, return data "hello world" with version 1."#;

    let (_state, _server_id, port) = start_zookeeper_server(instruction).await;

    println!("ZooKeeper server started on port {}", port);

    // Full implementation would:
    // 1. Connect with zookeeper-async
    // 2. Call getData("/test")
    // 3. Verify response is "hello world" with version 1
}

#[tokio::test]
#[ignore] // Requires Ollama and full implementation
async fn test_zookeeper_create_node() {
    let instruction = r#"Act as a ZooKeeper server.
Allow creating znodes. When a znode is created, respond with success and zxid 100."#;

    let (_state, _server_id, port) = start_zookeeper_server(instruction).await;

    println!("ZooKeeper server started on port {}", port);

    // Full implementation would:
    // 1. Connect with zookeeper-async
    // 2. Create znode at /newnode with data "test"
    // 3. Verify creation success
    // 4. Call getData("/newnode") to verify
}

#[tokio::test]
#[ignore] // Requires Ollama and full implementation
async fn test_zookeeper_get_children() {
    let instruction = r#"Act as a ZooKeeper server.
/services has three children: web, api, db.
When clients call getChildren("/services"), return those three children."#;

    let (_state, _server_id, port) = start_zookeeper_server(instruction).await;

    println!("ZooKeeper server started on port {}", port);

    // Full implementation would:
    // 1. Connect with zookeeper-async
    // 2. Call getChildren("/services")
    // 3. Verify response contains ["web", "api", "db"]
}
