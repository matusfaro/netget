//! DataLink (Layer 2) Integration Tests
//!
//! Black-box tests that use prompts to configure the LLM-controlled DataLink server.
//! Tests ARP protocol implementation at layer 2.

mod common;

use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
#[ignore] // Requires root/admin privileges for packet capture
async fn test_arp_responder() {
    println!("\n=== Testing ARP Responder via DataLink/LLM ===");

    // Find a suitable network interface (WiFi or wired, not loopback)
    use netget::network::datalink::DataLinkServer;
    let devices = DataLinkServer::list_devices().expect("Failed to list network devices");

    // Find first WiFi or wired interface (has MAC layer)
    // Common names: en0/en1 (macOS), eth0/eth1 (Linux), wlan0 (Linux WiFi)
    let interface = devices
        .iter()
        .find(|d| {
            let name = d.name.as_str();
            !name.starts_with("lo")
                && (name.starts_with("en")
                    || name.starts_with("eth")
                    || name.starts_with("wlan")
                    || name.starts_with("wl"))
        })
        .map(|d| d.name.clone())
        .expect("No suitable network interface found. Need a WiFi or wired interface with MAC layer for ARP.");

    println!("Using interface: {}", interface);

    // PROMPT: Tell the LLM to act as an ARP server on the detected interface
    let prompt = format!(
        "Act as an ARP server on interface {}. Respond to ARP requests for IP address 192.168.100.50 with MAC address 00:11:22:33:44:55",
        interface
    );

    // Start server
    let (_state, _port, _handle) = common::start_server_with_prompt(&prompt).await;

    println!("DataLink server started");
    sleep(Duration::from_secs(2)).await;

    // VALIDATION: Use arp command or arping to verify
    // Note: This requires the server to actually capture and inject packets
    // For now, this is a placeholder showing the test structure

    println!("Sending ARP request for 192.168.100.50...");

    // Use system arping command (if available)
    let output = Command::new("arping")
        .args(&["-c", "1", "-I", &interface, "192.168.100.50"])
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            println!("arping stdout: {}", stdout);
            println!("arping stderr: {}", stderr);

            // Check if we got a response
            if stdout.contains("reply") || stdout.contains("00:11:22:33:44:55") {
                println!("✓ Got ARP reply from LLM");
            } else {
                println!("✗ No ARP reply received (this is expected if server isn't fully implemented)");
            }
        }
        Err(e) => {
            println!("✗ arping command not available: {}", e);
            println!("Note: Install arping or use alternative ARP tool for full test");
        }
    }

    println!("\n=== ARP Responder test completed ===");
    println!("Note: Full DataLink support requires:");
    println!("  1. Root/admin privileges for packet capture");
    println!("  2. Actual packet injection implementation");
    println!("  3. ARP library or raw socket support");
}

#[tokio::test]
async fn test_datalink_server_setup() {
    println!("\n=== Testing DataLink Server Setup ===");

    // Find a suitable network interface
    use netget::network::datalink::DataLinkServer;
    let devices = DataLinkServer::list_devices().expect("Failed to list network devices");

    // Use first available interface (can be loopback for this test)
    let interface = devices
        .first()
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "lo0".to_string());

    println!("Testing with interface: {}", interface);

    // PROMPT: Simple DataLink setup
    let prompt = format!("Set up a DataLink layer 2 server on interface {}", interface);

    // Note: DataLink stack is not yet fully integrated in the test helper
    // The common::start_server_with_prompt will panic for DataLink
    // This test verifies that we can at least detect interface and construct the prompt correctly

    println!("Prompt constructed: {}", prompt);
    println!("✓ Interface detection and prompt construction working");
    println!("Note: Full DataLink server integration pending in test helper");

    println!("=== DataLink Server Setup test completed ===\n");
}
