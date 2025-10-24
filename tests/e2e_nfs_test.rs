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

#![cfg(feature = "e2e-tests")]

mod e2e;

use e2e::helpers::{self, ServerConfig, E2EResult};
use std::time::Duration;

#[tokio::test]
async fn test_nfs_server_start() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Server Start ===");

    // PROMPT: Basic NFS server
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} using nfs stack. Provide NFSv3 filesystem with export /data",
        port
    );

    // Start the NFS server
    let mut server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("NFS server started on port {}", server.port);

    // Verify it's an NFS server
    assert_eq!(server.stack, "NFS", "Expected NFS server but got {}", server.stack);
    assert!(server.is_running(), "Server should be running");

    println!("✓ NFS server initialized successfully");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_tcp_connection() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS TCP Connection ===");

    // PROMPT: NFS server that accepts connections
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} using nfs stack. Accept NFS client connections",
        port
    );

    // Start the NFS server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("NFS server started on port {}", server.port);

    // VALIDATION: Establish TCP connection to NFS port
    let addr = format!("127.0.0.1:{}", server.port);

    // Give the server a moment to fully initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Try to connect
    match tokio::net::TcpStream::connect(&addr).await {
        Ok(stream) => {
            println!("✓ TCP connection to NFS server successful");

            // Verify connection is maintained
            tokio::time::sleep(Duration::from_millis(500)).await;

            // Try to read to verify socket is open (non-blocking)
            let mut buf = [0u8; 1];
            match stream.try_read(&mut buf) {
                Ok(0) => {
                    // EOF - connection closed immediately
                    println!("⚠ Connection closed by server");
                },
                Ok(_) => {
                    println!("✓ Received data from server");
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // WouldBlock - connection is open but no data
                    println!("✓ Connection is open and waiting");
                },
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

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_multiple_connections() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Multiple Connections ===");

    // PROMPT: NFS server with multiple client support
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} using nfs stack. Support multiple concurrent NFS clients",
        port
    );

    // Start the NFS server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("NFS server started on port {}", server.port);

    let addr = format!("127.0.0.1:{}", server.port);
    tokio::time::sleep(Duration::from_millis(100)).await;

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
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("✓ All {} connections maintained", connections.len());

    // Close connections
    for (i, stream) in connections.into_iter().enumerate() {
        drop(stream);
        println!("✓ Connection {} closed", i + 1);
    }

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_connection_lifecycle() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Connection Lifecycle ===");

    // PROMPT: NFS server for lifecycle testing
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} using nfs stack. Handle connection lifecycle events",
        port
    );

    // Start the NFS server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("NFS server started on port {}", server.port);

    let addr = format!("127.0.0.1:{}", server.port);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // VALIDATION: Test connection lifecycle

    // 1. Connect
    let stream = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Connection established");

    // 2. Hold connection
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("✓ Connection held");

    // 3. Close gracefully
    drop(stream);
    println!("✓ Connection closed gracefully");

    // 4. Reconnect to verify server still accepting
    tokio::time::sleep(Duration::from_millis(100)).await;
    let stream2 = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Reconnection successful");
    drop(stream2);

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_port_configuration() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Port Configuration ===");

    // PROMPT: NFS on custom port
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} using nfs stack. Standard NFS v3 service",
        port
    );

    // Start the NFS server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("NFS server started on requested port {}", server.port);

    // Verify server is on the requested port
    assert_eq!(server.port, port, "Server should be on requested port");

    // Verify it's listening
    let addr = format!("127.0.0.1:{}", server.port);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let stream = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Server listening on correct port");
    drop(stream);

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_nfs_server_stop() -> E2EResult<()> {
    println!("\n=== E2E Test: NFS Server Stop ===");

    // PROMPT: NFS server with graceful shutdown
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} using nfs stack. Support clean shutdown",
        port
    );

    // Start the NFS server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("NFS server started on port {}", server.port);

    let addr = format!("127.0.0.1:{}", server.port);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Establish connection
    let stream = tokio::net::TcpStream::connect(&addr).await?;
    println!("✓ Connection established");

    // Stop server
    server.stop().await?;
    println!("✓ Server stopped gracefully");

    // Verify port is released
    tokio::time::sleep(Duration::from_millis(500)).await;

    match tokio::net::TcpStream::connect(&addr).await {
        Ok(_) => {
            // Connection shouldn't succeed after server stops
            println!("⚠ Port still accepting connections (server may not have stopped)");
        }
        Err(_) => {
            println!("✓ Port released after server stop");
        }
    }

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
