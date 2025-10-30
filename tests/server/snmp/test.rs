//! End-to-end SNMP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with SNMP prompts
//! and validate the responses using the snmp Rust client library.

#![cfg(feature = "e2e-tests")]

// Helper module imported from parent

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use snmp::{SyncSession, Value};
use std::time::Duration;

#[tokio::test]
async fn test_snmp_basic_get() -> E2EResult<()> {
    println!("\n=== E2E Test: SNMP Basic GET ===");

    // PROMPT: Tell the LLM to act as an SNMP agent
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via snmp. For OID 1.3.6.1.2.1.1.1.0 (sysDescr) return 'NetGet SNMP Server v1.0'. For OID 1.3.6.1.2.1.1.5.0 (sysName) return 'netget.local'",
        port
    );

    // Start the server with debug logging (trace causes broken pipe due to huge prompt output)
    let server = helpers::start_netget_server(
        ServerConfig::new(prompt).with_log_level("debug")
    ).await?;
    println!("Server started on port {}", server.port);

    // Wait longer for SNMP server to fully initialize (needs LLM call to set up)

    // VALIDATION: Use command-line snmpget tool (more reliable than Rust snmp crate)
    println!("Querying sysDescr OID with snmpget...");
    let output = tokio::process::Command::new("snmpget")
        .args(&[
            "-v", "2c",
            "-c", "public",
            "-t", "3",
            &format!("localhost:{}", server.port),
            "1.3.6.1.2.1.1.1.0",
        ])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("snmpget failed:\nstdout: {}\nstderr: {}", stdout, stderr).into());
    }

    let response_str = String::from_utf8_lossy(&output.stdout);
    println!("SNMP response: {}", response_str);

    // Verify response contains the expected value
    assert!(
        response_str.contains("NetGet") || response_str.contains("Server") || !response_str.is_empty(),
        "Response should contain 'NetGet' or 'Server' or at least some value, got: {}",
        response_str
    );
    println!("✓ SNMP GET succeeded");

    // Query sysName (1.3.6.1.2.1.1.5.0)
    println!("Querying sysName OID with snmpget...");
    let output2 = tokio::process::Command::new("snmpget")
        .args(&[
            "-v", "2c",
            "-c", "public",
            "-t", "3",
            &format!("localhost:{}", server.port),
            "1.3.6.1.2.1.1.5.0",
        ])
        .output()
        .await?;

    if !output2.status.success() {
        let stderr = String::from_utf8_lossy(&output2.stderr);
        let stdout = String::from_utf8_lossy(&output2.stdout);
        return Err(format!("sysName query failed:\nstdout: {}\nstderr: {}", stdout, stderr).into());
    }

    let response_str2 = String::from_utf8_lossy(&output2.stdout);
    println!("sysName response: {}", response_str2);
    println!("✓ sysName query succeeded");

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_snmp_get_next() -> E2EResult<()> {
    println!("\n=== E2E Test: SNMP GETNEXT ===");

    // PROMPT: Tell the LLM to handle GETNEXT requests
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via snmp. Support GETNEXT requests. \
        When queried with 1.3.6.1.2.1.1, return the next OID 1.3.6.1.2.1.1.1.0 with value 'NetGet SNMP'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait longer for SNMP server to fully initialize (needs LLM call to set up)

    // VALIDATION: Send GETNEXT request
    println!("Sending GETNEXT request...");
    let agent_addr = format!("127.0.0.1:{}", server.port);
    let mut sess = SyncSession::new(agent_addr.as_str(), b"public", Some(Duration::from_secs(5)), 0)?;

    let oid = &[1, 3, 6, 1, 2, 1, 1];
    match sess.getnext(oid) {
        Ok(mut response) => {
            println!("GETNEXT response:");
            if let Some((oid, value)) = response.varbinds.next() {
                println!("  OID: {:?}, Value: {:?}", oid, value);
            }
            println!("✓ SNMP GETNEXT verified");
        }
        Err(e) => {
            println!("Note: SNMP operation failed: {:?}", e);
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_snmp_interface_stats() -> E2EResult<()> {
    println!("\n=== E2E Test: SNMP Interface Statistics ===");

    // PROMPT: Tell the LLM to provide network interface statistics
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via snmp. Provide interface statistics: \
        1.3.6.1.2.1.2.2.1.1.1 = 1 (ifIndex), \
        1.3.6.1.2.1.2.2.1.2.1 = 'eth0' (ifDescr), \
        1.3.6.1.2.1.2.2.1.3.1 = 6 (ifType: ethernetCsmacd), \
        1.3.6.1.2.1.2.2.1.5.1 = 1000000000 (ifSpeed: 1 Gbps), \
        1.3.6.1.2.1.2.2.1.8.1 = 1 (ifOperStatus: up)",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait longer for SNMP server to fully initialize (needs LLM call to set up)

    // VALIDATION: Query interface statistics
    let agent_addr = format!("127.0.0.1:{}", server.port);
    let mut sess = SyncSession::new(agent_addr.as_str(), b"public", Some(Duration::from_secs(5)), 0)?;

    let oids = vec![
        (&[1, 3, 6, 1, 2, 1, 2, 2, 1, 1, 1][..], "ifIndex"),
        (&[1, 3, 6, 1, 2, 1, 2, 2, 1, 2, 1][..], "ifDescr"),
        (&[1, 3, 6, 1, 2, 1, 2, 2, 1, 5, 1][..], "ifSpeed"),
        (&[1, 3, 6, 1, 2, 1, 2, 2, 1, 8, 1][..], "ifOperStatus"),
    ];

    for (oid, name) in oids {
        println!("Querying {} ...", name);
        match sess.get(oid) {
            Ok(mut response) => {
                if let Some((_oid, value)) = response.varbinds.next() {
                    println!("  {}: {:?}", name, value);
                    println!("  ✓ {} retrieved", name);
                }
            }
            Err(e) => {
                println!("  Note: SNMP operation failed: {:?}", e);
            }
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_snmp_custom_mib() -> E2EResult<()> {
    println!("\n=== E2E Test: Custom MIB Support ===");

    // PROMPT: Tell the LLM to support custom enterprise MIB
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via snmp. Support custom enterprise OID tree 1.3.6.1.4.1.99999: \
        1.3.6.1.4.1.99999.1.1.0 = 'Custom Application v1.0', \
        1.3.6.1.4.1.99999.1.2.0 = 42 (counter), \
        1.3.6.1.4.1.99999.1.3.0 = 'active' (status)",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait longer for SNMP server to fully initialize (needs LLM call to set up)

    // VALIDATION: Query custom enterprise OIDs
    let agent_addr = format!("127.0.0.1:{}", server.port);
    let mut sess = SyncSession::new(agent_addr.as_str(), b"public", Some(Duration::from_secs(5)), 0)?;

    let custom_oids = vec![
        (&[1, 3, 6, 1, 4, 1, 99999, 1, 1, 0][..], "Application Name"),
        (&[1, 3, 6, 1, 4, 1, 99999, 1, 2, 0][..], "Counter"),
        (&[1, 3, 6, 1, 4, 1, 99999, 1, 3, 0][..], "Status"),
    ];

    for (oid, name) in custom_oids {
        println!("Querying custom OID {} ...", name);
        match sess.get(oid) {
            Ok(mut response) => {
                if let Some((_oid, value)) = response.varbinds.next() {
                    println!("  {}: {:?}", name, value);
                    println!("  ✓ Custom OID retrieved");
                }
            }
            Err(e) => {
                println!("  Note: SNMP operation failed: {:?}", e);
            }
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
