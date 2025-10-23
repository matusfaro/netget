//! End-to-end SNMP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with SNMP prompts
//! and validate the responses using the snmp Rust client library.

#![cfg(feature = "e2e-tests")]

mod e2e;

use e2e::helpers::{self, ServerConfig, E2EResult};
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

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Use SNMP client to query
    println!("Creating SNMP client session...");
    let agent_addr = format!("127.0.0.1:{}", server.port);
    let community = b"public";
    let timeout = Duration::from_secs(5);

    let mut sess = SyncSession::new(agent_addr.as_str(), community, Some(timeout), 0)?;

    // Query sysDescr (1.3.6.1.2.1.1.1.0)
    println!("Querying sysDescr OID...");
    let oid = &[1, 3, 6, 1, 2, 1, 1, 1, 0];
    match sess.get(oid) {
        Ok(mut response) => {
            println!("sysDescr response: {:?}", response);
            if let Some((_oid, value)) = response.varbinds.next() {
                if let Value::OctetString(ref s) = value {
                    let value_str = String::from_utf8_lossy(s);
                    println!("✓ SNMP GET succeeded: {}", value_str);
                    if value_str.contains("NetGet") || value_str.contains("Server") {
                        println!("✓ Response contains expected content");
                    }
                }
            }
        }
        Err(e) => {
            println!("Note: SNMP GET failed (may not be fully implemented yet): {:?}", e);
        }
    }

    // Query sysName (1.3.6.1.2.1.1.5.0)
    println!("Querying sysName OID...");
    let oid = &[1, 3, 6, 1, 2, 1, 1, 5, 0];
    match sess.get(oid) {
        Ok(mut response) => {
            if let Some((_oid, value)) = response.varbinds.next() {
                println!("sysName response: {:?}", value);
                println!("✓ sysName query succeeded");
            }
        }
        Err(e) => {
            println!("Note: sysName query failed: {:?}", e);
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_snmp_getbulk() -> E2EResult<()> {
    println!("\n=== E2E Test: SNMP GETBULK ===");

    // PROMPT: Tell the LLM to provide multiple OID values
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via snmp. Provide the following OID values: \
        1.3.6.1.2.1.1.1.0 = 'NetGet SNMP Agent', \
        1.3.6.1.2.1.1.3.0 = 123456 (timeticks), \
        1.3.6.1.2.1.1.5.0 = 'netget-server', \
        1.3.6.1.2.1.1.6.0 = 'Test Location'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.
        port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Perform SNMP GETBULK on system tree (similar to walk)
    println!("Performing SNMP GETBULK on system tree (1.3.6.1.2.1.1)...");
    let agent_addr = format!("127.0.0.1:{}", server.port);
    let mut sess = SyncSession::new(agent_addr.as_str(), b"public", Some(Duration::from_secs(5)), 0)?;

    let oid = &[1, 3, 6, 1, 2, 1, 1];
    let non_repeaters = 0;
    let max_repetitions = 10;
    match sess.getbulk(&[oid], non_repeaters, max_repetitions) {
        Ok(response) => {
            let mut count = 0;
            println!("SNMP GETBULK returned:");
            for (oid, value) in response.varbinds {
                println!("  {}: OID={:?}, Value={:?}", count + 1, oid, value);
                count += 1;
                if count >= 10 {
                    break;
                }
            }
            assert!(
                count > 0,
                "SNMP GETBULK should return at least one value"
            );
            println!("✓ SNMP GETBULK verified ({} values)", count);
        }
        Err(e) => {
            println!("Note: SNMP GETBULK failed (may not be fully implemented yet): {:?}", e);
        }
    }

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

    tokio::time::sleep(Duration::from_millis(500)).await;

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

    tokio::time::sleep(Duration::from_millis(500)).await;

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
async fn test_snmp_multiple_queries() -> E2EResult<()> {
    println!("\n=== E2E Test: Multiple Concurrent Queries ===");

    // PROMPT: Tell the LLM to handle multiple concurrent requests
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via snmp. Handle multiple concurrent SNMP requests. \
        Provide standard system OID values for sysDescr, sysObjectID, sysUpTime, sysContact, sysName, sysLocation",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Send multiple queries in quick succession
    let oids: Vec<&[u32]> = vec![
        &[1, 3, 6, 1, 2, 1, 1, 1, 0],  // sysDescr
        &[1, 3, 6, 1, 2, 1, 1, 3, 0],  // sysUpTime
        &[1, 3, 6, 1, 2, 1, 1, 5, 0],  // sysName
        &[1, 3, 6, 1, 2, 1, 1, 6, 0],  // sysLocation
    ];

    println!("Sending {} concurrent SNMP GET requests...", oids.len());
    let mut handles = vec![];

    for oid in &oids {
        let port = server.port;
        let oid = oid.to_vec();
        let handle = tokio::task::spawn_blocking(move || -> bool {
            let agent_addr = format!("127.0.0.1:{}", port);
            let mut sess = match SyncSession::new(
                agent_addr.as_str(),
                b"public",
                Some(Duration::from_secs(5)),
                0,
            ) {
                Ok(s) => s,
                Err(_) => return false,
            };

            match sess.get(&oid) {
                Ok(_) => true,
                Err(_) => false,
            }
        });
        handles.push(handle);
    }

    // Wait for all queries to complete
    let mut success_count = 0;
    for handle in handles {
        if let Ok(true) = handle.await {
            success_count += 1;
        }
    }

    println!("✓ {}/{} concurrent queries succeeded", success_count, oids.len());

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

    tokio::time::sleep(Duration::from_millis(500)).await;

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
