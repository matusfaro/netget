//! E2E tests for etcd client
//!
//! Tests etcd client operations with a real etcd server

#![cfg(all(test, feature = "etcd"))]

use anyhow::Result;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;

/// Helper to start a Docker etcd server for testing
async fn start_etcd_server() -> Result<(String, std::process::Child)> {
    // Start etcd in Docker
    let child = Command::new("docker")
        .args(&[
            "run",
            "--rm",
            "-d",
            "-p", "2379:2379",
            "-p", "2380:2380",
            "--name", "netget-etcd-test",
            "quay.io/coreos/etcd:v3.5.17",
            "/usr/local/bin/etcd",
            "--advertise-client-urls", "http://0.0.0.0:2379",
            "--listen-client-urls", "http://0.0.0.0:2379",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Wait for etcd to be ready
    sleep(Duration::from_secs(3)).await;

    Ok(("localhost:2379".to_string(), child))
}

/// Helper to stop the etcd server
async fn stop_etcd_server() -> Result<()> {
    let _ = Command::new("docker")
        .args(&["stop", "netget-etcd-test"])
        .output();
    sleep(Duration::from_millis(500)).await;
    Ok(())
}

/// Test basic etcd client operations
#[tokio::test]
async fn test_etcd_client_basic_operations() -> Result<()> {
    // Start etcd server
    let (server_addr, _child) = start_etcd_server().await?;

    // Connect to etcd
    let client = etcd_client::Client::connect([&server_addr], None).await?;
    let mut client = client;

    // Test PUT operation
    client.put("/test/key1", "value1", None).await?;
    println!("✓ PUT /test/key1 = value1");

    // Test GET operation
    let resp = client.get("/test/key1", None).await?;
    assert_eq!(resp.kvs().len(), 1);
    let kv = &resp.kvs()[0];
    assert_eq!(kv.key(), b"/test/key1");
    assert_eq!(kv.value(), b"value1");
    println!("✓ GET /test/key1 returned correct value");

    // Test DELETE operation
    let resp = client.delete("/test/key1", None).await?;
    assert_eq!(resp.deleted(), 1);
    println!("✓ DELETE /test/key1 deleted 1 key");

    // Verify key is gone
    let resp = client.get("/test/key1", None).await?;
    assert_eq!(resp.kvs().len(), 0);
    println!("✓ GET /test/key1 returned empty (key deleted)");

    // Cleanup
    stop_etcd_server().await?;

    Ok(())
}

/// Test etcd client with LLM integration (simplified)
///
/// This test validates the etcd client can be used programmatically
/// without the full NetGet LLM integration layer
#[tokio::test]
async fn test_etcd_client_multiple_keys() -> Result<()> {
    // Start etcd server
    let (server_addr, _child) = start_etcd_server().await?;

    // Connect to etcd
    let client = etcd_client::Client::connect([&server_addr], None).await?;
    let mut client = client;

    // Put multiple keys
    client.put("/app/config/database", "postgresql://localhost:5432/mydb", None).await?;
    client.put("/app/config/timeout", "30", None).await?;
    client.put("/app/config/max_connections", "100", None).await?;
    println!("✓ PUT 3 config keys");

    // Get all keys
    let resp1 = client.get("/app/config/database", None).await?;
    assert_eq!(resp1.kvs().len(), 1);
    assert_eq!(resp1.kvs()[0].value(), b"postgresql://localhost:5432/mydb");

    let resp2 = client.get("/app/config/timeout", None).await?;
    assert_eq!(resp2.kvs().len(), 1);
    assert_eq!(resp2.kvs()[0].value(), b"30");

    let resp3 = client.get("/app/config/max_connections", None).await?;
    assert_eq!(resp3.kvs().len(), 1);
    assert_eq!(resp3.kvs()[0].value(), b"100");
    println!("✓ GET all 3 keys returned correct values");

    // Cleanup
    stop_etcd_server().await?;

    Ok(())
}

/// Test etcd client with nonexistent key
#[tokio::test]
async fn test_etcd_client_nonexistent_key() -> Result<()> {
    // Start etcd server
    let (server_addr, _child) = start_etcd_server().await?;

    // Connect to etcd
    let client = etcd_client::Client::connect([&server_addr], None).await?;
    let mut client = client;

    // Try to get a nonexistent key
    let resp = client.get("/does/not/exist", None).await?;
    assert_eq!(resp.kvs().len(), 0);
    println!("✓ GET nonexistent key returned empty");

    // Cleanup
    stop_etcd_server().await?;

    Ok(())
}
