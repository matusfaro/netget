//! TUI snapshot tests for /docs command
//!
//! These tests capture the visual output of the /docs command to ensure
//! the formatting and colors remain consistent.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[path = "snapshot_util.rs"]
mod snapshot_util;

const SNAPSHOT_DIR: &str = "tests/docs_tui/snapshots";

/// Test /docs command (list all protocols)
#[test]
fn test_docs_list_all_protocols() {
    // Run netget with /docs command
    let mut child = Command::new("./target/release/netget")
        .env("COLUMNS", "120") // Set terminal width
        .env("LINES", "50") // Set terminal height
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn netget");

    // Send commands
    let stdin = child.stdin.as_mut().expect("Failed to open stdin");
    stdin
        .write_all(b"/docs\n/exit\n")
        .expect("Failed to write to stdin");

    // Wait for completion with timeout
    let output = match wait_with_timeout(child, Duration::from_secs(10)) {
        Ok(output) => output,
        Err((mut child, e)) => {
            let _ = child.kill();
            panic!("Command timed out: {}", e);
        }
    };

    // Combine stdout and stderr
    let full_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Extract just the /docs output (skip startup logs)
    let docs_output = extract_docs_section(&full_output, "/docs");

    // Save snapshot
    snapshot_util::assert_snapshot("docs_list_all", SNAPSHOT_DIR, &docs_output);

    // Sanity checks
    assert!(docs_output.contains("Core Protocols") || docs_output.contains("Available Protocols"));
    assert!(docs_output.contains("TCP") || docs_output.contains("tcp"));
    assert!(docs_output.contains("HTTP") || docs_output.contains("http"));
}

/// Test /docs bgp command (detailed protocol docs)
#[test]
fn test_docs_bgp_protocol() {
    // Run netget with /docs bgp command
    let mut child = Command::new("./target/release/netget")
        .env("COLUMNS", "120") // Set terminal width
        .env("LINES", "80") // Set terminal height (larger for detailed view)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn netget");

    // Send commands
    let stdin = child.stdin.as_mut().expect("Failed to open stdin");
    stdin
        .write_all(b"/docs bgp\n/exit\n")
        .expect("Failed to write to stdin");

    // Wait for completion with timeout
    let output = match wait_with_timeout(child, Duration::from_secs(10)) {
        Ok(output) => output,
        Err((mut child, e)) => {
            let _ = child.kill();
            panic!("Command timed out: {}", e);
        }
    };

    // Combine stdout and stderr
    let full_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Extract just the /docs output
    let docs_output = extract_docs_section(&full_output, "/docs bgp");

    // Save snapshot
    snapshot_util::assert_snapshot("docs_bgp_detailed", SNAPSHOT_DIR, &docs_output);

    // Sanity checks - check for colorized elements (ANSI codes)
    assert!(
        docs_output.contains("Protocol:")
            || docs_output.contains("BGP")
            || docs_output.contains("Bgp")
    );
    assert!(docs_output.contains("Stack:") || docs_output.contains("ETH>IP>TCP>BGP"));
    assert!(docs_output.contains("Status:") || docs_output.contains("Alpha"));

    // Check for section headers (may have box drawing or ANSI codes)
    assert!(docs_output.contains("Event") || docs_output.contains("Action"));
}

/// Test /docs ssh command (protocol with rich actions)
#[test]
fn test_docs_ssh_protocol() {
    // Run netget with /docs ssh command
    let mut child = Command::new("./target/release/netget")
        .env("COLUMNS", "120") // Set terminal width
        .env("LINES", "80") // Set terminal height
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn netget");

    // Send commands
    let stdin = child.stdin.as_mut().expect("Failed to open stdin");
    stdin
        .write_all(b"/docs ssh\n/exit\n")
        .expect("Failed to write to stdin");

    // Wait for completion with timeout
    let output = match wait_with_timeout(child, Duration::from_secs(10)) {
        Ok(output) => output,
        Err((mut child, e)) => {
            let _ = child.kill();
            panic!("Command timed out: {}", e);
        }
    };

    // Combine stdout and stderr
    let full_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Extract just the /docs output
    let docs_output = extract_docs_section(&full_output, "/docs ssh");

    // Save snapshot
    snapshot_util::assert_snapshot("docs_ssh_detailed", SNAPSHOT_DIR, &docs_output);

    // Sanity checks
    assert!(docs_output.contains("SSH") || docs_output.contains("Ssh"));
    assert!(docs_output.contains("Stack:") || docs_output.contains("TCP>SSH"));
}

/// Helper to extract the docs section from full output
fn extract_docs_section(full_output: &str, _command: &str) -> String {
    let lines: Vec<&str> = full_output.lines().collect();

    // Find where docs content starts (skip startup logs)
    let mut start_idx = 0;
    for (i, line) in lines.iter().enumerate() {
        // Look for box drawing chars, section headers, or protocol names
        if line.contains("╭")
            || line.contains("━━━")
            || line.contains("Protocol:")
            || line.contains("Available Protocols")
            || line.contains("# ")
        {
            start_idx = i;
            break;
        }
    }

    // Find where docs content ends (before exit or end of output)
    let mut end_idx = lines.len();
    for (i, line) in lines[start_idx..].iter().enumerate() {
        if line.contains("exit") && line.contains("Exiting") {
            end_idx = start_idx + i;
            break;
        }
    }

    // If we didn't find good boundaries, just take everything after startup logs
    if start_idx == 0 {
        // Skip past the startup INFO logs
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("INFO")
                && !line.contains("Starting NetGet")
                && !line.contains("Python:")
                && !line.contains("Node.js:")
                && !line.contains("Go:")
                && !line.trim().is_empty()
            {
                start_idx = i;
                break;
            }
        }
    }

    lines[start_idx..end_idx].join("\n")
}

/// Wait for child process with timeout
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, (std::process::Child, String)> {
    use std::thread;
    use std::time::Instant;

    let start = Instant::now();
    let mut exited = false;

    while start.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(_)) => {
                // Process exited
                exited = true;
                break;
            }
            Ok(None) => {
                // Still running, wait a bit
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err((child, format!("Error checking process: {}", e)));
            }
        }
    }

    if exited {
        // Get the output by consuming child
        child.wait_with_output().map_err(|e| {
            // This is a fatal error - can't return child since it's consumed
            panic!("Failed to get output from completed process: {}", e)
        })
    } else {
        Err((child, "Process did not complete within timeout".to_string()))
    }
}
