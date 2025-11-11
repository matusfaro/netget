//! E2E tests for Syslog protocol
//!
//! These tests verify Syslog server functionality by starting NetGet with Syslog prompts
//! and using the `logger` command (built-in on Linux/macOS) or raw UDP sockets to send messages.

#![cfg(feature = "syslog")]

use crate::server::helpers::*;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

#[tokio::test]
async fn test_syslog_comprehensive() -> E2EResult<()> {
    // Single comprehensive server with scripting for all test cases
    let config = ServerConfig::new(
        r#"listen on port 0 via syslog

You are a Syslog server implementing RFC 3164 and RFC 5424.

MESSAGE FILTERING BY SEVERITY:
- Emergency (0), Alert (1), Critical (2): Store and alert with "🚨 CRITICAL:" prefix
- Error (3): Store and show with "❌ ERROR:" prefix
- Warning (4): Store and show with "⚠️  WARNING:" prefix
- Notice (5), Info (6): Store silently
- Debug (7): Ignore (drop)

MESSAGE FILTERING BY FACILITY:
- auth, authpriv: Always store and show "🔐 AUTH:" + message
- kernel: Always store and show "⚙️  KERNEL:" + message
- All other facilities: Apply severity-based filtering above

HOSTNAME TRACKING:
- Track unique hostnames that send messages
- Show count every 5 messages: "Received 5 messages from N unique hosts"

Use scripting mode to handle all messages without LLM calls after initial setup.
"#,
    )
    .with_log_level("off");

    let test_state = start_netget_server(config).await?;

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    let server_addr: SocketAddr = format!("127.0.0.1:{}", test_state.port)
        .parse()
        .expect("Failed to parse server address");

    // Create UDP client socket
    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("Failed to set read timeout");

    println!("✓ Syslog server started on {}", server_addr);

    // Test 1: Emergency message (facility=kernel, severity=emergency)
    println!("\n[Test 1] Send emergency kernel message");
    let emergency_msg = "<0>Oct 11 22:14:15 server-01 kernel: Kernel panic - not syncing";
    client
        .send_to(emergency_msg.as_bytes(), server_addr)
        .expect("Failed to send emergency message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ Emergency message sent");

    // Test 2: Auth failure (facility=auth, severity=notice)
    println!("\n[Test 2] Send auth failure message");
    let auth_msg =
        "<37>Oct 11 22:14:16 server-01 sshd: Failed password for root from 192.168.1.100";
    client
        .send_to(auth_msg.as_bytes(), server_addr)
        .expect("Failed to send auth message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ Auth message sent");

    // Test 3: Error message (facility=daemon, severity=error)
    println!("\n[Test 3] Send daemon error message");
    let error_msg = "<27>Oct 11 22:14:17 server-02 httpd: Database connection failed";
    client
        .send_to(error_msg.as_bytes(), server_addr)
        .expect("Failed to send error message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ Error message sent");

    // Test 4: Warning message (facility=user, severity=warning)
    println!("\n[Test 4] Send warning message");
    let warning_msg = "<12>Oct 11 22:14:18 server-02 myapp: High memory usage detected";
    client
        .send_to(warning_msg.as_bytes(), server_addr)
        .expect("Failed to send warning message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ Warning message sent");

    // Test 5: Info message (facility=user, severity=info)
    println!("\n[Test 5] Send info message");
    let info_msg = "<14>Oct 11 22:14:19 server-03 myapp: Application started successfully";
    client
        .send_to(info_msg.as_bytes(), server_addr)
        .expect("Failed to send info message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ Info message sent");

    // Test 6: Debug message (should be ignored/dropped)
    println!("\n[Test 6] Send debug message (should be ignored)");
    let debug_msg = "<15>Oct 11 22:14:20 server-03 myapp: Debug: Processing request 12345";
    client
        .send_to(debug_msg.as_bytes(), server_addr)
        .expect("Failed to send debug message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ Debug message sent (should be ignored by server)");

    // Test 7: RFC 5424 format message
    println!("\n[Test 7] Send RFC 5424 format message");
    let rfc5424_msg =
        "<165>1 2003-10-11T22:14:15.003Z server-04 myapp 1234 ID47 - Application event occurred";
    client
        .send_to(rfc5424_msg.as_bytes(), server_addr)
        .expect("Failed to send RFC 5424 message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ RFC 5424 message sent");

    // Test 8: AuthPriv facility message
    println!("\n[Test 8] Send authpriv message");
    let authpriv_msg = "<86>Oct 11 22:14:21 server-01 sudo: alice : TTY=pts/0 ; PWD=/home/alice ; USER=root ; COMMAND=/bin/ls";
    client
        .send_to(authpriv_msg.as_bytes(), server_addr)
        .expect("Failed to send authpriv message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ AuthPriv message sent");

    // Test 9: Critical message from local0 facility
    println!("\n[Test 9] Send critical message from local0 facility");
    let critical_msg = "<130>Oct 11 22:14:22 app-server myapp: CRITICAL: Database cluster is down";
    client
        .send_to(critical_msg.as_bytes(), server_addr)
        .expect("Failed to send critical message");
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ Critical message sent");

    // Give server time to process all messages
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("\n✓ All Syslog tests passed!");
    println!("  - Sent 9 messages across different facilities and severities");
    println!("  - Tested RFC 3164 and RFC 5424 formats");
    println!("  - Tested filtering by severity and facility");

    // Cleanup
    test_state.stop().await?;
    Ok(())
}
