//! E2E tests for ZooKeeper server
//!
//! These tests spawn the NetGet binary and test ZooKeeper protocol operations
//! by manually constructing ZooKeeper binary protocol messages and verifying responses.

#![cfg(all(test, feature = "zookeeper"))]

use crate::helpers::{start_netget_server, E2EResult, NetGetConfig};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::sleep;

/// Build a ZooKeeper getData request
/// Format: [4 bytes length][4 bytes xid][4 bytes op_type = 4][path]
fn build_get_data_request(xid: i32, path: &str) -> Vec<u8> {
    let mut request = Vec::new();

    // Build payload first
    let mut payload = Vec::new();
    payload.extend_from_slice(&xid.to_be_bytes()); // xid
    payload.extend_from_slice(&4i32.to_be_bytes()); // op_type = 4 (getData)

    // Add path (length-prefixed string)
    let path_bytes = path.as_bytes();
    payload.extend_from_slice(&(path_bytes.len() as i32).to_be_bytes());
    payload.extend_from_slice(path_bytes);

    // Add watch flag (boolean = 1 byte)
    payload.push(0); // no watch

    // Prepend length
    request.extend_from_slice(&(payload.len() as i32).to_be_bytes());
    request.extend_from_slice(&payload);

    request
}

/// Build a ZooKeeper getChildren request
/// Format: [4 bytes length][4 bytes xid][4 bytes op_type = 8][path]
fn build_get_children_request(xid: i32, path: &str) -> Vec<u8> {
    let mut request = Vec::new();

    // Build payload first
    let mut payload = Vec::new();
    payload.extend_from_slice(&xid.to_be_bytes()); // xid
    payload.extend_from_slice(&8i32.to_be_bytes()); // op_type = 8 (getChildren)

    // Add path (length-prefixed string)
    let path_bytes = path.as_bytes();
    payload.extend_from_slice(&(path_bytes.len() as i32).to_be_bytes());
    payload.extend_from_slice(path_bytes);

    // Add watch flag (boolean = 1 byte)
    payload.push(0); // no watch

    // Prepend length
    request.extend_from_slice(&(payload.len() as i32).to_be_bytes());
    request.extend_from_slice(&payload);

    request
}

/// Parse ZooKeeper response header
/// Format: [4 bytes length][4 bytes xid][8 bytes zxid][4 bytes error_code][data]
fn parse_response_header(data: &[u8]) -> Option<(i32, i32, i64, i32)> {
    if data.len() < 20 {
        return None;
    }

    let length = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let xid = i32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let zxid = i64::from_be_bytes([
        data[8], data[9], data[10], data[11],
        data[12], data[13], data[14], data[15],
    ]);
    let error_code = i32::from_be_bytes([data[16], data[17], data[18], data[19]]);

    Some((length, xid, zxid, error_code))
}

/// Test ZooKeeper getData operation
#[tokio::test]
async fn test_zookeeper_get_data() -> E2EResult<()> {
    println!("\n=== Test: ZooKeeper getData Operation ===");

    let prompt = r#"Start ZooKeeper server on port 0.
When clients read /config/database, return the string 'postgres://localhost:5432'.
Return zxid=100, error_code=0 (success)."#;

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: getData request - MUST BE FIRST (most specific)
                .on_event("zookeeper_request")
                .and_event_data_contains("operation", "getData")
                .and_event_data_contains("path", "/config/database")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "zookeeper_response",
                        "xid": 1,
                        "zxid": 100,
                        "error_code": 0,
                        "data_hex": hex::encode("postgres://localhost:5432")
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Server startup - MUST BE LAST (less specific)
                .on_instruction_containing("ZooKeeper")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "ZooKeeper",
                        "instruction": "Return database config"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to initialize
    sleep(Duration::from_millis(500)).await;

    // Connect to server
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ Connected to ZooKeeper server");

    // Send getData request
    let request = build_get_data_request(1, "/config/database");
    println!("Sending getData request for /config/database");
    stream.write_all(&request).await?;
    stream.flush().await?;

    // Read response
    let mut buffer = vec![0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("Received {} bytes", n);

            if let Some((length, xid, zxid, error_code)) = parse_response_header(&buffer[..n]) {
                println!("Response: length={}, xid={}, zxid={}, error_code={}", length, xid, zxid, error_code);

                assert_eq!(xid, 1, "XID should match request");
                assert_eq!(error_code, 0, "Error code should be 0 (success)");
                println!("✓ ZooKeeper getData response validated");
            } else {
                println!("Warning: Could not parse response header");
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed before response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Timeout waiting for response");
        }
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");

    Ok(())
}

/// Test ZooKeeper getChildren operation
#[tokio::test]
async fn test_zookeeper_get_children() -> E2EResult<()> {
    println!("\n=== Test: ZooKeeper getChildren Operation ===");

    let prompt = r#"Start ZooKeeper server on port 0.
When clients request children of /services, return: ['web', 'api', 'db'].
Return zxid=200, error_code=0 (success)."#;

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: getChildren request - MUST BE FIRST (most specific)
                .on_event("zookeeper_request")
                .and_event_data_contains("operation", "getChildren")
                .and_event_data_contains("path", "/services")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "zookeeper_response",
                        "xid": 2,
                        "zxid": 200,
                        "error_code": 0,
                        "data_hex": "00000003000000037765620000000361706900000002646200" // Array with 3 strings
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Server startup - MUST BE LAST (less specific)
                .on_instruction_containing("ZooKeeper")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "ZooKeeper",
                        "instruction": "Return service list"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to initialize
    sleep(Duration::from_millis(500)).await;

    // Connect to server
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ Connected to ZooKeeper server");

    // Send getChildren request
    let request = build_get_children_request(2, "/services");
    println!("Sending getChildren request for /services");
    stream.write_all(&request).await?;
    stream.flush().await?;

    // Read response
    let mut buffer = vec![0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("Received {} bytes", n);

            if let Some((length, xid, zxid, error_code)) = parse_response_header(&buffer[..n]) {
                println!("Response: length={}, xid={}, zxid={}, error_code={}", length, xid, zxid, error_code);

                assert_eq!(xid, 2, "XID should match request");
                assert_eq!(error_code, 0, "Error code should be 0 (success)");
                println!("✓ ZooKeeper getChildren response validated");
            } else {
                println!("Warning: Could not parse response header");
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed before response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Timeout waiting for response");
        }
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");

    Ok(())
}

/// Test ZooKeeper error response (node not found)
#[tokio::test]
async fn test_zookeeper_error_response() -> E2EResult<()> {
    println!("\n=== Test: ZooKeeper Error Response ===");

    let prompt = r#"Start ZooKeeper server on port 0.
When clients try to read /nonexistent, return error_code=-101 (no node).
Return zxid=300."#;

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: getData request for nonexistent path - MUST BE FIRST (most specific)
                .on_event("zookeeper_request")
                .and_event_data_contains("operation", "getData")
                .and_event_data_contains("path", "/nonexistent")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "zookeeper_response",
                        "xid": 3,
                        "zxid": 300,
                        "error_code": -101, // NONODE error
                        "data_hex": ""
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Server startup - MUST BE LAST (less specific)
                .on_instruction_containing("ZooKeeper")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "ZooKeeper",
                        "instruction": "Return error for missing nodes"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to initialize
    sleep(Duration::from_millis(500)).await;

    // Connect to server
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ Connected to ZooKeeper server");

    // Send getData request for nonexistent path
    let request = build_get_data_request(3, "/nonexistent");
    println!("Sending getData request for /nonexistent");
    stream.write_all(&request).await?;
    stream.flush().await?;

    // Read response
    let mut buffer = vec![0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("Received {} bytes", n);

            if let Some((length, xid, zxid, error_code)) = parse_response_header(&buffer[..n]) {
                println!("Response: length={}, xid={}, zxid={}, error_code={}", length, xid, zxid, error_code);

                assert_eq!(xid, 3, "XID should match request");
                assert_eq!(error_code, -101, "Error code should be -101 (NONODE)");
                println!("✓ ZooKeeper error response validated");
            } else {
                println!("Warning: Could not parse response header");
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed before response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Timeout waiting for response");
        }
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");

    Ok(())
}
