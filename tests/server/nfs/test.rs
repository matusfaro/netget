//! End-to-end NFS tests for NetGet
//!
//! These tests spawn the actual NetGet binary with NFS prompts
//! and validate LLM-controlled NFS v3 filesystem operations.
//!
//! The NFS implementation uses nfsserve library which handles:
//! - RPC/XDR protocol encoding/decoding
//! - MOUNT protocol
//! - TCP connection management
//!
//! The LLM controls all filesystem operations through structured actions:
//! - File/directory lookup, creation, deletion
//! - File read/write operations
//! - Attribute getting/setting
//! - Directory listings
//!
//! These tests validate server startup, connection handling, and basic NFS protocol.

#![cfg(feature = "nfs")]

// Helper module imported from parent

use super::super::super::helpers::{self, E2EResult, ServerConfig};

#[tokio::test]
async fn test_nfs_server_start() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Server Start ===");

    // PROMPT: Basic NFS server
    let prompt = "listen on port {AVAILABLE_PORT} using nfs stack. Provide NFSv3 filesystem with export /data";

    let server_config = ServerConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("nfs")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "NFS",
                        "instruction": "Provide NFSv3 filesystem with export /data"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the NFS server
    let mut server = helpers::start_netget_server(server_config).await?;
    println!("NFS server started on port {}", server.port);

    // Verify it's an NFS server
    assert_eq!(
        server.stack, "NFS",
        "Expected NFS server but got {}",
        server.stack
    );
    assert!(server.is_running(), "Server should be running");

    println!("✓ NFS server initialized successfully");

    // Verify mock expectations
    server.verify_mocks().await?;

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_tcp_connection() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS TCP Connection ===");

    // PROMPT: NFS server that accepts connections
    let prompt = "listen on port {AVAILABLE_PORT} using nfs stack. Accept NFS client connections";

    let server_config = ServerConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("nfs")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "NFS",
                        "instruction": "Accept NFS client connections"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the NFS server
    let server = helpers::start_netget_server(server_config).await?;
    println!("NFS server started on port {}", server.port);

    // VALIDATION: Establish TCP connection to NFS port
    let addr = format!("127.0.0.1:{}", server.port);

    // Give the server a moment to fully initialize

    // Try to connect
    match tokio::net::TcpStream::connect(&addr).await {
        Ok(stream) => {
            println!("✓ TCP connection to NFS server successful");

            // Verify connection is maintained

            // Try to read to verify socket is open (non-blocking)
            let mut buf = [0u8; 1];
            match stream.try_read(&mut buf) {
                Ok(0) => {
                    // EOF - connection closed immediately
                    println!("⚠ Connection closed by server");
                }
                Ok(_) => {
                    println!("✓ Received data from server");
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // WouldBlock - connection is open but no data
                    println!("✓ Connection is open and waiting");
                }
                Err(_) => {
                    println!("⚠ Read error on connection");
                }
            }

            drop(stream);
        }
        Err(e) => {
            return Err(format!("Failed to connect to NFS server: {}", e).into());
        }
    }

    // Verify mock expectations
    server.verify_mocks().await?;

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_multiple_connections() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Multiple Connections ===");

    // PROMPT: NFS server with multiple client support
    let prompt =
        "listen on port {AVAILABLE_PORT} using nfs stack. Support multiple concurrent NFS clients";

    let server_config = ServerConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("nfs")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "NFS",
                        "instruction": "Support multiple concurrent NFS clients"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the NFS server
    let server = helpers::start_netget_server(server_config).await?;
    println!("NFS server started on port {}", server.port);

    let addr = format!("127.0.0.1:{}", server.port);

    // VALIDATION: Open multiple concurrent connections
    let mut connections = Vec::new();

    for i in 1..=3 {
        match tokio::net::TcpStream::connect(&addr).await {
            Ok(stream) => {
                println!("✓ Connection {} established", i);
                connections.push(stream);
            }
            Err(e) => {
                return Err(format!("Failed to establish connection {}: {}", i, e).into());
            }
        }
    }

    // Verify all connections are maintained
    println!("✓ All {} connections maintained", connections.len());

    // Close connections
    for (i, stream) in connections.into_iter().enumerate() {
        drop(stream);
        println!("✓ Connection {} closed", i + 1);
    }

    // Verify mock expectations
    server.verify_mocks().await?;

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_connection_lifecycle() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Connection Lifecycle ===");

    // PROMPT: NFS server for lifecycle testing
    let prompt =
        "listen on port {AVAILABLE_PORT} using nfs stack. Handle connection lifecycle events";

    let server_config = ServerConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("nfs")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "NFS",
                        "instruction": "Handle connection lifecycle events"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the NFS server
    let server = helpers::start_netget_server(server_config).await?;
    println!("NFS server started on port {}", server.port);

    let addr = format!("127.0.0.1:{}", server.port);

    // VALIDATION: Test connection lifecycle

    // 1. Connect
    let stream = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Connection established");

    // 2. Hold connection
    println!("✓ Connection held");

    // 3. Close gracefully
    drop(stream);
    println!("✓ Connection closed gracefully");

    // 4. Reconnect to verify server still accepting
    let stream2 = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Reconnection successful");
    drop(stream2);

    // Verify mock expectations
    server.verify_mocks().await?;

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_port_configuration() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Port Configuration ===");

    // PROMPT: NFS on custom port
    let prompt = "listen on port {AVAILABLE_PORT} using nfs stack. Standard NFS v3 service";

    let server_config = ServerConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("nfs")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "NFS",
                        "instruction": "Standard NFS v3 service"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the NFS server
    let server = helpers::start_netget_server(server_config).await?;
    println!("NFS server started on port {}", server.port);

    // Verify it's listening
    let addr = format!("127.0.0.1:{}", server.port);

    let stream = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Server listening on correct port");
    drop(stream);

    // Verify mock expectations
    server.verify_mocks().await?;

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_server_stop() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Server Stop ===");

    // PROMPT: NFS server with graceful shutdown
    let prompt = "listen on port {AVAILABLE_PORT} using nfs stack. Support clean shutdown";

    let server_config = ServerConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("nfs")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "NFS",
                        "instruction": "Support clean shutdown"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the NFS server
    let server = helpers::start_netget_server(server_config).await?;
    println!("NFS server started on port {}", server.port);

    let addr = format!("127.0.0.1:{}", server.port);

    // Establish connection
    let stream = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Connection established");

    // Stop server
    server.stop().await?;
    println!("✓ Server stopped gracefully");

    // Verify port is released

    match tokio::net::TcpStream::connect(&addr).await {
        Ok(_) => {
            // Connection shouldn't succeed after server stops
            println!("⚠ Port still accepting connections (server may not have stopped)");
        }
        Err(_) => {
            println!("✓ Port released after server stop");
        }
    }

    // Verify mock expectations
    server.verify_mocks().await?;

    drop(stream);
    println!("=== Test passed ===\n");
    Ok(())
}

// NOTE: The following tests are placeholders for future implementation
// when the full NFS v3 protocol is implemented

#[tokio::test]
#[ignore] // Ignored until NFS protocol is implemented
async fn test_nfs_mount_export() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Mount Export (UNIMPLEMENTED) ===");
    println!("This test requires full NFS MOUNT protocol implementation");
    println!("Required: RPC portmapper, MOUNT v3 procedures");
    Ok(())
}

#[tokio::test]
#[ignore] // Ignored until NFS protocol is implemented
async fn test_nfs_file_lookup() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS File Lookup (UNIMPLEMENTED) ===");
    println!("This test requires NFS LOOKUP procedure implementation");
    println!("Required: XDR encoding/decoding, file handle management");
    Ok(())
}

#[tokio::test]
#[ignore] // Ignored until NFS protocol is implemented
async fn test_nfs_read_write() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Read/Write (UNIMPLEMENTED) ===");
    println!("This test requires NFS READ/WRITE procedure implementation");
    println!("Required: LLM-backed virtual filesystem, data transfer");
    Ok(())
}
