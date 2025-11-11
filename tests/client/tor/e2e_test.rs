//! End-to-end tests for Tor client
//!
//! These tests require:
//! - Internet connection
//! - Access to Tor network (not blocked by firewall/country)
//! - Ollama with LLM model loaded
//! - ~10-30 seconds for initial bootstrap (cached afterward)

#![cfg(all(test, feature = "tor-client"))]

use netget::llm::OllamaClient;
use netget::state::app_state::AppState;
use netget::state::ClientStatus;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Helper to create test app state with Tor client
async fn create_test_client(
    instruction: &str,
    remote_addr: &str,
) -> (
    Arc<AppState>,
    u32,
    OllamaClient,
    mpsc::UnboundedReceiver<String>,
) {
    let (status_tx, status_rx) = mpsc::unbounded_channel();
    let app_state = Arc::new(AppState::new(status_tx.clone()));

    // Create LLM client
    let llm_client = OllamaClient::new(
        "http://localhost:11434".to_string(),
        "qwen2.5-coder:3b".to_string(), // Use smaller model for faster tests
    );

    // Create Tor client
    let client_id = app_state
        .open_client(
            "Tor".to_string(),
            remote_addr.to_string(),
            instruction.to_string(),
            None,
        )
        .await;

    (app_state, client_id, llm_client, status_rx)
}

/// Test 1: Bootstrap Tor client and verify it initializes
///
/// This test verifies that arti can bootstrap and download consensus.
/// First run takes 10-30 seconds, subsequent runs use cached consensus.
#[tokio::test]
#[ignore] // Ignore by default due to Tor network requirement
async fn test_tor_bootstrap() {
    let (app_state, client_id, llm_client, mut status_rx) =
        create_test_client("Wait for connection", "check.torproject.org:80").await;

    let status_tx = app_state.get_status_tx();

    // Start the client (triggers bootstrap)
    let result = netget::cli::client_startup::start_client_by_id(
        &app_state,
        netget::state::ClientId::from_u32(client_id),
        &llm_client,
        &status_tx,
    )
    .await;

    assert!(result.is_ok(), "Client startup failed: {:?}", result.err());

    // Wait for bootstrap and connection (may take 30+ seconds first time)
    tokio::time::sleep(Duration::from_secs(40)).await;

    // Verify client is in Connected status (or at least not Error)
    let client = app_state
        .get_client(netget::state::ClientId::from_u32(client_id))
        .await
        .expect("Client not found");

    match client.status {
        ClientStatus::Connected => {
            println!("✓ Tor client successfully bootstrapped and connected");
        }
        ClientStatus::Connecting => {
            println!("⚠ Tor client still connecting (may need more time)");
        }
        ClientStatus::Error(ref e) => {
            panic!("Tor client failed: {}", e);
        }
        _ => {
            println!("Client status: {:?}", client.status);
        }
    }

    // Collect status messages
    let mut messages = Vec::new();
    while let Ok(msg) = status_rx.try_recv() {
        if !msg.starts_with("__") {
            messages.push(msg);
        }
    }

    println!("Status messages:");
    for msg in &messages {
        println!("  {}", msg);
    }

    // Verify we see bootstrapping messages
    let has_bootstrap_msg = messages.iter().any(|m| {
        m.contains("Tor client") && (m.contains("initializing") || m.contains("bootstrapped"))
    });
    assert!(has_bootstrap_msg, "Expected to see Tor bootstrap messages");
}

/// Test 2: Connect to check.torproject.org and verify Tor usage
///
/// This test verifies that connections go through Tor by fetching
/// the Tor check page which confirms Tor usage.
#[tokio::test]
#[ignore] // Ignore by default due to Tor network requirement
async fn test_tor_check_connection() {
    let (app_state, client_id, llm_client, mut status_rx) = create_test_client(
        "Send HTTP GET request for / and report if response mentions 'Tor'",
        "check.torproject.org:80",
    )
    .await;

    let status_tx = app_state.get_status_tx();

    // Start the client
    let result = netget::cli::client_startup::start_client_by_id(
        &app_state,
        netget::state::ClientId::from_u32(client_id),
        &llm_client,
        &status_tx,
    )
    .await;

    assert!(result.is_ok(), "Client startup failed: {:?}", result.err());

    // Wait for connection and HTTP exchange
    tokio::time::sleep(Duration::from_secs(50)).await;

    // Collect status messages
    let mut messages = Vec::new();
    while let Ok(msg) = status_rx.try_recv() {
        if !msg.starts_with("__") {
            messages.push(msg);
        }
    }

    println!("Status messages:");
    for msg in &messages {
        println!("  {}", msg);
    }

    // Verify we connected through Tor
    let has_connected = messages
        .iter()
        .any(|m| m.contains("Tor client") && m.contains("connected"));
    assert!(has_connected, "Expected to see Tor connection message");
}

/// Test 3: Connect to onion service (DuckDuckGo)
///
/// This test verifies that .onion addresses work correctly.
#[tokio::test]
#[ignore] // Ignore by default due to Tor network requirement
async fn test_tor_onion_service() {
    let (app_state, client_id, llm_client, mut status_rx) = create_test_client(
        "Send HTTP GET request for / and report status",
        "duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion:80",
    )
    .await;

    let status_tx = app_state.get_status_tx();

    // Start the client
    let result = netget::cli::client_startup::start_client_by_id(
        &app_state,
        netget::state::ClientId::from_u32(client_id),
        &llm_client,
        &status_tx,
    )
    .await;

    assert!(result.is_ok(), "Client startup failed: {:?}", result.err());

    // Wait for onion service connection (can take longer than regular connection)
    tokio::time::sleep(Duration::from_secs(60)).await;

    // Collect status messages
    let mut messages = Vec::new();
    while let Ok(msg) = status_rx.try_recv() {
        if !msg.starts_with("__") {
            messages.push(msg);
        }
    }

    println!("Status messages:");
    for msg in &messages {
        println!("  {}", msg);
    }

    // Verify we connected to onion service
    let has_onion_connected = messages
        .iter()
        .any(|m| m.contains("Tor client") && m.contains("connected") && m.contains(".onion"));
    assert!(
        has_onion_connected,
        "Expected to see onion service connection"
    );
}

/// Test 4: Error handling for invalid onion address
///
/// This test verifies graceful handling of connection failures.
#[tokio::test]
#[ignore] // Ignore by default due to Tor network requirement
async fn test_tor_connection_error() {
    let (app_state, client_id, llm_client, _status_rx) = create_test_client(
        "Connect and report",
        "invalidonionaddressthatshouldnotexist123456789012345678.onion:80",
    )
    .await;

    let status_tx = app_state.get_status_tx();

    // Start the client (should fail)
    let result = netget::cli::client_startup::start_client_by_id(
        &app_state,
        netget::state::ClientId::from_u32(client_id),
        &llm_client,
        &status_tx,
    )
    .await;

    // Wait for connection attempt
    tokio::time::sleep(Duration::from_secs(30)).await;

    // Verify client is in error state
    let client = app_state
        .get_client(netget::state::ClientId::from_u32(client_id))
        .await
        .expect("Client not found");

    match client.status {
        ClientStatus::Error(_) => {
            println!("✓ Tor client correctly reported error for invalid address");
        }
        other => {
            panic!(
                "Expected Error status for invalid onion address, got: {:?}",
                other
            );
        }
    }
}
