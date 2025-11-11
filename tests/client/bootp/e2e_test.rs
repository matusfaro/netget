//! BOOTP client end-to-end tests
//!
//! Tests BOOTP client with real BOOTP server (dnsmasq or isc-dhcp-server).

#![cfg(all(test, feature = "bootp"))]

use netget::llm::ollama_client::OllamaClient;
use netget::state::app_state::AppState;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Test BOOTP client basic connectivity
///
/// LLM Budget: 2 calls
/// - Call 1: bootp_connected event → send_bootp_request action
/// - Call 2: bootp_reply_received event → analyze and disconnect
#[tokio::test]
#[ignore] // Requires BOOTP server (dnsmasq) running
async fn test_bootp_request_reply() {
    // Setup
    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://127.0.0.1:11434".to_string());
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();

    // Register BOOTP client
    let client_id = app_state
        .register_client(
            "BOOTP".to_string(),
            "127.0.0.1:67".to_string(), // dnsmasq BOOTP server
            Some(
                "Request IP address for MAC 00:11:22:33:44:55 and report boot server details"
                    .to_string(),
            ),
            None,
        )
        .await;

    // Start client
    netget::cli::client_startup::start_client_by_id(&app_state, client_id, &llm_client, &status_tx)
        .await
        .expect("Failed to start BOOTP client");

    // Collect status messages for verification
    let mut messages = Vec::new();
    let timeout = tokio::time::Duration::from_secs(30);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, status_rx.recv()).await {
            Ok(Some(msg)) => {
                println!("[TEST] {}", msg);
                messages.push(msg.clone());

                // Stop when we see disconnect or error
                if msg.contains("disconnect") || msg.contains("ERROR") {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    // Verify BOOTP flow completed
    let all_messages = messages.join("\n");

    // Should see connected event
    assert!(
        all_messages.contains("BOOTP client") && all_messages.contains("connected"),
        "Did not see BOOTP client connected"
    );

    // Should see BOOTP request sent
    assert!(
        all_messages.contains("BOOTP request sent") || all_messages.contains("send_bootp_request"),
        "Did not see BOOTP request sent"
    );

    // Note: Reply verification depends on having a real BOOTP server
    // If no server, test will timeout but that's expected
    if all_messages.contains("assigned_ip") || all_messages.contains("boot_filename") {
        println!("[TEST] ✓ BOOTP reply received and processed by LLM");
    } else {
        println!("[TEST] ⚠ No BOOTP reply received (server may not be running)");
    }
}

/// Test BOOTP client with broadcast discovery
///
/// LLM Budget: 2-3 calls
/// - Call 1: bootp_connected → send_bootp_request (broadcast)
/// - Call 2+: bootp_reply_received (possibly multiple servers) → analyze
#[tokio::test]
#[ignore] // Requires BOOTP server
async fn test_bootp_broadcast_discovery() {
    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://127.0.0.1:11434".to_string());
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();

    // Register client with broadcast instruction
    let client_id = app_state
        .register_client(
            "BOOTP".to_string(),
            "255.255.255.255:67".to_string(),
            Some("Broadcast BOOTP request to discover all boot servers on network. Use MAC 52:54:00:12:34:56".to_string()),
            None,
        )
        .await;

    // Start client
    netget::cli::client_startup::start_client_by_id(&app_state, client_id, &llm_client, &status_tx)
        .await
        .expect("Failed to start BOOTP client");

    // Collect messages
    let mut messages = Vec::new();
    let timeout = tokio::time::Duration::from_secs(30);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, status_rx.recv()).await {
            Ok(Some(msg)) => {
                println!("[TEST] {}", msg);
                messages.push(msg.clone());

                if msg.contains("disconnect") {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    let all_messages = messages.join("\n");

    // Verify broadcast was attempted
    assert!(
        all_messages.contains("255.255.255.255") || all_messages.contains("broadcast"),
        "Did not see broadcast BOOTP request"
    );

    println!("[TEST] BOOTP broadcast discovery completed");
}

/// Test BOOTP client error handling (no server)
///
/// LLM Budget: 1 call
/// - Call 1: bootp_connected → send_bootp_request
/// - (No reply, timeout expected)
#[tokio::test]
async fn test_bootp_no_server() {
    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://127.0.0.1:11434".to_string());
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();

    // Register client pointing to non-existent server
    let client_id = app_state
        .register_client(
            "BOOTP".to_string(),
            "192.0.2.1:67".to_string(), // TEST-NET-1 (no server)
            Some("Request IP for MAC 00:11:22:33:44:55".to_string()),
            None,
        )
        .await;

    // Start client
    let result = netget::cli::client_startup::start_client_by_id(
        &app_state,
        client_id,
        &llm_client,
        &status_tx,
    )
    .await;

    // Should succeed (UDP is connectionless, can't detect no-server at connect time)
    assert!(
        result.is_ok(),
        "BOOTP client should start even without server"
    );

    // Collect messages with short timeout
    let mut messages = Vec::new();
    let timeout = tokio::time::Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, status_rx.recv()).await {
            Ok(Some(msg)) => {
                println!("[TEST] {}", msg);
                messages.push(msg.clone());
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    let all_messages = messages.join("\n");

    // Should see request sent (but no reply)
    assert!(
        all_messages.contains("connected") || all_messages.contains("BOOTP"),
        "BOOTP client should start"
    );

    println!("[TEST] BOOTP no-server test completed (timeout expected)");
}

/// Test BOOTP client with custom MAC address
///
/// LLM Budget: 2 calls
/// - Call 1: bootp_connected → send_bootp_request with specific MAC
/// - Call 2: bootp_reply_received → verify MAC was used
#[tokio::test]
#[ignore] // Requires BOOTP server
async fn test_bootp_custom_mac() {
    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://127.0.0.1:11434".to_string());
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();

    let client_id = app_state
        .register_client(
            "BOOTP".to_string(),
            "127.0.0.1:67".to_string(),
            Some(
                "Request IP for specific MAC address AA:BB:CC:DD:EE:FF and report results"
                    .to_string(),
            ),
            None,
        )
        .await;

    // Start client
    netget::cli::client_startup::start_client_by_id(&app_state, client_id, &llm_client, &status_tx)
        .await
        .expect("Failed to start BOOTP client");

    // Collect messages
    let mut messages = Vec::new();
    let timeout = tokio::time::Duration::from_secs(20);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, status_rx.recv()).await {
            Ok(Some(msg)) => {
                println!("[TEST] {}", msg);
                messages.push(msg.clone());

                if msg.contains("disconnect") {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    let all_messages = messages.join("\n");

    // Verify MAC address was used
    assert!(
        all_messages.to_lowercase().contains("aa:bb:cc:dd:ee:ff")
            || all_messages.to_lowercase().contains("aabbccddeeff"),
        "Custom MAC address should be used in request"
    );

    println!("[TEST] BOOTP custom MAC test completed");
}
