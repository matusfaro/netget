//! Direct output tests for /docs command formatting
//!
//! These tests verify the colorized output format of the docs module.

use netget::docs::{list_all_protocols, show_protocol_docs};

#[path = "snapshot_util.rs"]
mod snapshot_util;

const SNAPSHOT_DIR: &str = "tests/docs_output/snapshots";

#[test]
fn test_docs_list_all_protocols_output() {
    let output = list_all_protocols();

    // Save snapshot
    snapshot_util::assert_snapshot("list_all_protocols", SNAPSHOT_DIR, &output);

    // Sanity checks
    assert!(output.contains("Available Protocols") || output.contains("Core Protocols"));
    assert!(output.contains("TCP") || output.contains("tcp"));
    assert!(output.contains("HTTP") || output.contains("http"));
    assert!(output.contains("SSH") || output.contains("ssh"));
}

#[test]
fn test_docs_bgp_protocol_output() {
    let output = show_protocol_docs("bgp").expect("BGP docs should exist");

    // Save snapshot
    snapshot_util::assert_snapshot("bgp_protocol", SNAPSHOT_DIR, &output);

    // Sanity checks - check for colorized box drawing
    assert!(output.contains("╭") || output.contains("Protocol"));
    assert!(output.contains("╰") || output.contains("Bgp"));

    // Check for ANSI color codes (cyan)
    assert!(output.contains("\x1b[36m") || output.contains("\x1b[96m"));

    // Check for section headers
    assert!(output.contains("Stack:"));
    assert!(output.contains("Status:"));
    assert!(output.contains("Description:"));

    // Check for event types or actions
    assert!(output.contains("Event") || output.contains("Action"));
}

#[test]
fn test_docs_ssh_protocol_output() {
    let output = show_protocol_docs("ssh").expect("SSH docs should exist");

    // Save snapshot
    snapshot_util::assert_snapshot("ssh_protocol", SNAPSHOT_DIR, &output);

    // Check for ANSI formatting
    assert!(output.contains("\x1b["));  // Some ANSI code

    // Check structure
    assert!(output.contains("SSH") || output.contains("Ssh"));
    assert!(output.contains("TCP>SSH"));
}

#[test]
fn test_docs_tcp_protocol_output() {
    let output = show_protocol_docs("tcp").expect("TCP docs should exist");

    // Save snapshot
    snapshot_util::assert_snapshot("tcp_protocol", SNAPSHOT_DIR, &output);

    // Check for color codes and structure
    assert!(output.contains("╭") || output.contains("─"));  // Box drawing
    assert!(output.contains("\x1b["));  // ANSI codes
    assert!(output.contains("Stack:"));
}

#[test]
fn test_docs_unknown_protocol() {
    let result = show_protocol_docs("nonexistent_protocol");

    assert!(result.is_err());
    let error_msg = result.unwrap_err();

    // Should have colored error message
    assert!(error_msg.contains("Unknown protocol") || error_msg.contains("nonexistent_protocol"));
    assert!(error_msg.contains("\x1b[31m"));  // Red color code
}
