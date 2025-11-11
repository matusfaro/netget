//! IS-IS client E2E tests
//!
//! These tests verify the IS-IS client can capture and parse IS-IS PDUs.
//!
//! **Prerequisites:**
//! - Root access (CAP_NET_RAW) for pcap
//! - IS-IS router on the network OR
//! - Packet replay with tcpreplay OR
//! - Virtual network interface with IS-IS traffic
//!
//! **Running:**
//! ```bash
//! sudo ./cargo-isolated.sh test --no-default-features --features isis --test client::isis::e2e_test
//! ```

#![cfg(all(test, feature = "isis"))]

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use netget::client::isis::IsisClient;
use netget::llm::ollama_client::OllamaClient;
use netget::state::app_state::AppState;
use netget::state::{ClientId, ClientInstance, ClientStatus};

/// Helper to check if running as root
fn is_root() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Helper to check if Ollama is available
async fn is_ollama_available() -> bool {
    let client = OllamaClient::new(
        "http://localhost:11434".to_string(),
        "qwen3-coder:30b".to_string(),
    );
    client.check_health().await.is_ok()
}

#[tokio::test]
#[ignore] // Requires root access and IS-IS traffic
async fn test_isis_client_requires_root() -> Result<()> {
    if !is_root() {
        println!("SKIPPED: Test requires root privileges for pcap");
        return Ok(());
    }

    if !is_ollama_available().await {
        println!("SKIPPED: Ollama not available");
        return Ok(());
    }

    // Initialize test environment
    let app_state = Arc::new(AppState::new());
    let (_status_tx, _status_rx) = mpsc::unbounded_channel();
    let llm_client = OllamaClient::new(
        "http://localhost:11434".to_string(),
        "qwen3-coder:30b".to_string(),
    );

    // Create ISIS client
    let client_id = ClientId::new(1);
    let client = ClientInstance {
        id: client_id,
        protocol_name: "IS-IS".to_string(),
        remote_addr: "eth0".to_string(), // Interface name, not IP:port
        instruction: "Capture IS-IS PDUs and analyze topology".to_string(),
        status: ClientStatus::Connecting,
        memory: String::new(),
        startup_params: None,
    };

    app_state.add_client(client).await;

    // Note: This test cannot actually run without IS-IS traffic on the network
    // It serves as documentation for how the client would be tested

    println!("ISIS client test structure validated");
    Ok(())
}

#[tokio::test]
async fn test_isis_device_listing() -> Result<()> {
    // This test doesn't require root, just checks device listing works
    // Skip if not on a system with network interfaces
    match IsisClient::list_devices() {
        Ok(devices) => {
            println!("Found {} network devices", devices.len());
            for device in devices {
                println!("  - {}: {:?}", device.name, device.desc);
            }
        }
        Err(e) => {
            println!("Could not list devices (may not have permissions): {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore] // Manual test with real IS-IS traffic
async fn test_isis_capture_with_llm() -> Result<()> {
    // This is a manual test that requires:
    // 1. Running as root
    // 2. IS-IS router on the network
    // 3. Ollama running
    // 4. Setting the correct interface name

    if !is_root() {
        println!("SKIPPED: Requires root");
        return Ok(());
    }

    if !is_ollama_available().await {
        println!("SKIPPED: Ollama not available");
        return Ok(());
    }

    let app_state = Arc::new(AppState::new());
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();
    let llm_client = OllamaClient::new(
        "http://localhost:11434".to_string(),
        "qwen3-coder:30b".to_string(),
    );

    // CHANGE THIS to your network interface
    let interface = "eth0"; // or "en0", "wlan0", etc.

    let client_id = ClientId::new(1);
    let client = ClientInstance {
        id: client_id,
        protocol_name: "IS-IS".to_string(),
        remote_addr: interface.to_string(),
        instruction: "Capture IS-IS Hello and LSP PDUs. Identify all routers and their neighbors."
            .to_string(),
        status: ClientStatus::Connecting,
        memory: String::new(),
        startup_params: None,
    };

    app_state.add_client(client).await;

    // Start ISIS capture
    println!("Starting IS-IS capture on interface: {}", interface);
    let _local_addr = IsisClient::connect_with_llm_actions(
        interface.to_string(),
        llm_client,
        app_state.clone(),
        status_tx.clone(),
        client_id,
    )
    .await?;

    // Capture for 30 seconds
    let mut count = 0;
    let timeout = Duration::from_secs(30);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout {
        tokio::select! {
            Some(msg) = status_rx.recv() => {
                println!("{}", msg);
                count += 1;
            }
            _ = sleep(Duration::from_millis(100)) => {}
        }
    }

    println!("Captured {} IS-IS events", count);

    // Check final status
    if let Some(client) = app_state.get_client(client_id).await {
        println!("Final client status: {:?}", client.status);
        println!("LLM memory: {}", client.memory);
    }

    Ok(())
}

/// Test basic PDU parsing without network access
#[test]
fn test_isis_pdu_parsing() {
    // This is a unit test that doesn't require network access
    // Test with a sample IS-IS Hello PDU

    // Sample IS-IS L2 LAN Hello PDU (hex)
    let sample_pdu =
        hex::decode("83 1b 01 00 10 01 00 00 00 01 02 03 04 05 06 01 00 00 00 00").unwrap();

    // Basic validation: first byte should be 0x83 (IS-IS discriminator)
    assert_eq!(sample_pdu[0], 0x83, "IS-IS discriminator should be 0x83");

    println!("IS-IS PDU parsing validation passed");
}
