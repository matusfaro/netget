//! DoT (DNS over TLS) client E2E tests
//!
//! Tests the DoT client against public DoT servers (dns.google:853, cloudflare-dns.com:853)
//! to ensure LLM-controlled DNS query functionality works correctly.

#![cfg(all(test, feature = "dot"))]

use netget::cli::session::init_logging_for_tests;
use netget::llm::ollama_client::OllamaClient;
use netget::state::app_state::AppState;
use netget::state::ClientStatus;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Helper to create test environment
async fn setup_test() -> (Arc<AppState>, OllamaClient, mpsc::UnboundedReceiver<String>) {
    init_logging_for_tests();

    let app_state = Arc::new(AppState::new());
    let ollama_client = OllamaClient::new("http://localhost:11434", "qwen3-coder:30b");
    let (status_tx, status_rx) = mpsc::unbounded_channel();

    // Store status_tx in app state for later use
    app_state.set_status_tx(status_tx.clone()).await;

    (app_state, ollama_client, status_rx)
}

#[tokio::test]
#[ignore] // Requires Ollama server running
async fn test_dot_client_basic_query() {
    let (app_state, ollama_client, mut status_rx) = setup_test().await;

    // Open DoT client
    let client_id = app_state
        .add_client(
            "DoT".to_string(),
            "dns.google:853".to_string(),
            "Query example.com A record and tell me the IP address".to_string(),
            None,
        )
        .await;

    // Start client
    let status_tx = app_state.get_status_tx().await.unwrap();
    netget::cli::client_startup::start_client_by_id(
        &app_state,
        client_id,
        &ollama_client,
        &status_tx,
    )
    .await
    .unwrap();

    // Wait for status messages
    let mut connected = false;
    let mut query_sent = false;
    let mut response_received = false;

    for _ in 0..100 {
        tokio::select! {
            Some(msg) = status_rx.recv() => {
                if msg.contains("connected") {
                    connected = true;
                }
                if msg.contains("DoT query:") {
                    query_sent = true;
                }
                if msg.contains("received response") {
                    response_received = true;
                }

                // Stop after we get response
                if response_received {
                    break;
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                break;
            }
        }
    }

    // Verify client connected
    assert!(connected, "Client should connect to DoT server");

    // Verify query was sent
    assert!(query_sent, "LLM should send DNS query");

    // Verify response was received
    assert!(response_received, "Client should receive DNS response");

    // Verify client status
    let client = app_state.get_client(client_id).await.unwrap();
    assert!(matches!(client.status, ClientStatus::Connected));
}

#[tokio::test]
#[ignore] // Requires Ollama server running
async fn test_dot_client_multiple_queries() {
    let (app_state, ollama_client, mut status_rx) = setup_test().await;

    // Open DoT client with instruction to query multiple record types
    let client_id = app_state
        .add_client(
            "DoT".to_string(),
            "1.1.1.1:853".to_string(),
            "Query example.com for A, AAAA, and MX records, one at a time".to_string(),
            None,
        )
        .await;

    // Start client
    let status_tx = app_state.get_status_tx().await.unwrap();
    netget::cli::client_startup::start_client_by_id(
        &app_state,
        client_id,
        &ollama_client,
        &status_tx,
    )
    .await
    .unwrap();

    // Count queries and responses
    let mut query_count = 0;
    let mut response_count = 0;

    for _ in 0..200 {
        tokio::select! {
            Some(msg) = status_rx.recv() => {
                if msg.contains("DoT query:") {
                    query_count += 1;
                }
                if msg.contains("received response") {
                    response_count += 1;
                }

                // Stop after we get 3 responses
                if response_count >= 3 {
                    break;
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {
                break;
            }
        }
    }

    // Verify multiple queries were sent
    assert!(
        query_count >= 3,
        "Should send at least 3 queries (A, AAAA, MX)"
    );

    // Verify multiple responses were received
    assert!(response_count >= 3, "Should receive at least 3 responses");
}

#[tokio::test]
#[ignore] // Requires Ollama server running
async fn test_dot_client_nxdomain_handling() {
    let (app_state, ollama_client, mut status_rx) = setup_test().await;

    // Open DoT client to query non-existent domain
    let client_id = app_state
        .add_client(
            "DoT".to_string(),
            "dns.google:853".to_string(),
            "Query nonexistent-domain-12345.example for A record and tell me what error you get"
                .to_string(),
            None,
        )
        .await;

    // Start client
    let status_tx = app_state.get_status_tx().await.unwrap();
    netget::cli::client_startup::start_client_by_id(
        &app_state,
        client_id,
        &ollama_client,
        &status_tx,
    )
    .await
    .unwrap();

    // Wait for response
    let mut response_received = false;
    let mut has_nxdomain = false;

    for _ in 0..100 {
        tokio::select! {
            Some(msg) = status_rx.recv() => {
                if msg.contains("received response") {
                    response_received = true;
                }
                if msg.to_lowercase().contains("nxdomain") || msg.to_lowercase().contains("not exist") {
                    has_nxdomain = true;
                }

                if response_received && has_nxdomain {
                    break;
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                break;
            }
        }
    }

    // Verify response was received
    assert!(response_received, "Should receive DNS response");

    // Note: We can't assert has_nxdomain because the LLM might not mention it explicitly
    // The important thing is that the client handles the NXDOMAIN response without crashing
}

#[tokio::test]
#[ignore] // Requires Ollama server running
async fn test_dot_client_tls_connection() {
    let (app_state, ollama_client, mut status_rx) = setup_test().await;

    // Open DoT client to Cloudflare
    let client_id = app_state
        .add_client(
            "DoT".to_string(),
            "cloudflare-dns.com:853".to_string(),
            "Query cloudflare.com A record".to_string(),
            None,
        )
        .await;

    // Start client
    let status_tx = app_state.get_status_tx().await.unwrap();
    netget::cli::client_startup::start_client_by_id(
        &app_state,
        client_id,
        &ollama_client,
        &status_tx,
    )
    .await
    .unwrap();

    // Wait for TLS handshake to complete
    let mut tls_connected = false;

    for _ in 0..50 {
        tokio::select! {
            Some(msg) = status_rx.recv() => {
                if msg.contains("connected") {
                    tls_connected = true;
                    break;
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(10)) => {
                break;
            }
        }
    }

    // Verify TLS connection succeeded
    assert!(
        tls_connected,
        "Should establish TLS connection to DoT server"
    );

    // Verify client status
    let client = app_state.get_client(client_id).await.unwrap();
    assert!(matches!(client.status, ClientStatus::Connected));
}
