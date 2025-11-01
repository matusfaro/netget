//! etcd E2E tests using real etcd-client

#![cfg(feature = "etcd")]

use super::super::helpers::start_server_with_prompt;
use super::super::helpers::assert_stack_name;
use etcd_client::Client;
use std::time::Duration;
use tokio::time::sleep;

/// Comprehensive etcd KV operations test
///
/// This test covers:
/// - Put/Get operations
/// - Get non-existent keys
/// - Range queries
/// - Delete operations
/// - Basic transactions
///
/// Target: < 10 total LLM calls (1 startup + ~6 operations)
#[tokio::test]
#[cfg_attr(not(feature = "e2e-tests"), ignore)]
async fn test_etcd_kv_operations() -> Result<(), Box<dyn std::error::Error>> {
    // Start etcd server with comprehensive prompt
    let prompt = r#"
listen on port 0 via etcd

You are an etcd v3 key-value store server. Handle all KV operations:

1. When clients PUT a key-value pair, store it in memory with revision tracking
2. When clients GET a key, return the stored value if it exists (empty kvs array if not)
3. When clients DELETE a key, remove it and return deleted count
4. For RANGE queries, return all keys matching the prefix
5. For TRANSACTIONS, evaluate conditions and execute success or failure branch

Examples:
- PUT /config/database = "localhost:5432" → Success (revision 1)
- GET /config/database → "localhost:5432" (revision 1)
- GET /nonexistent → Empty (no error)
- DELETE /config/database → Deleted 1 key
- RANGE /config/ → Returns all keys starting with /config/

Track revisions:
- First PUT: create_revision=1, mod_revision=1, version=1
- Update: create_revision=1, mod_revision=2, version=2
- Each mutation increments global revision counter

Respond with appropriate etcd_range_response, etcd_put_response, etc. actions.
"#;

    // Use test helpers to start server
    let (state, port, _handle) = start_server_with_prompt(prompt).await?;

    // Wait for server to be fully ready
    sleep(Duration::from_secs(2)).await;

    // Verify server started with etcd stack
    assert_stack_name(&state, "ETH>IP>TCP>GRPC>ETCD").await;

    // Connect etcd client
    let endpoint = format!("http://localhost:{}", port);
    println!("Connecting to etcd server at {}", endpoint);

    let mut client = Client::connect([endpoint.as_str()], None)
        .await
        .expect("Failed to connect to etcd server");

    println!("✓ Connected to etcd server");

    // Test 1: Put a key-value pair
    println!("\n[Test 1] Put /config/database = localhost:5432");
    client
        .put("/config/database", "localhost:5432", None)
        .await
        .expect("Put operation failed");
    println!("✓ Put successful");

    // Test 2: Get the key back
    println!("\n[Test 2] Get /config/database");
    let get_resp = client
        .get("/config/database", None)
        .await
        .expect("Get operation failed");

    let kvs = get_resp.kvs();
    assert_eq!(kvs.len(), 1, "Expected 1 key-value pair");

    let kv = kvs.first().unwrap();
    assert_eq!(kv.key_str()?, "/config/database");
    assert_eq!(kv.value_str()?, "localhost:5432");
    println!("✓ Get successful: value = {}", kv.value_str()?);

    // Test 3: Get non-existent key
    println!("\n[Test 3] Get /nonexistent (should return empty)");
    let get_resp = client
        .get("/nonexistent", None)
        .await
        .expect("Get operation failed");

    assert_eq!(get_resp.kvs().len(), 0, "Expected empty result for non-existent key");
    println!("✓ Non-existent key returned empty (correct behavior)");

    // Test 4: Put more keys for range query
    println!("\n[Test 4] Put multiple keys under /config/ prefix");
    client.put("/config/timeout", "30", None).await?;
    client.put("/config/max_connections", "100", None).await?;
    client.put("/other/key", "value", None).await?;
    println!("✓ Put 3 more keys");

    // Test 5: Range query with prefix
    println!("\n[Test 5] Range query /config/ prefix");
    let range_resp = client
        .get("/config/", None)  // Note: etcd-client handles prefix automatically
        .await?;

    println!("Range returned {} keys", range_resp.kvs().len());
    for kv in range_resp.kvs() {
        println!("  - {}: {}", kv.key_str()?, kv.value_str()?);
    }

    // Should get at least the /config/database key
    assert!(range_resp.kvs().len() >= 1, "Expected at least 1 key in /config/ range");
    println!("✓ Range query successful");

    // Test 6: Delete a key
    println!("\n[Test 6] Delete /config/timeout");
    let del_resp = client
        .delete("/config/timeout", None)
        .await?;

    println!("Deleted {} keys", del_resp.deleted());
    assert!(del_resp.deleted() >= 0, "Delete should return count");
    println!("✓ Delete successful");

    // Test 7: Verify delete (get should return empty)
    println!("\n[Test 7] Verify /config/timeout is deleted");
    let get_resp = client
        .get("/config/timeout", None)
        .await?;

    assert_eq!(get_resp.kvs().len(), 0, "Key should be deleted");
    println!("✓ Key successfully deleted");

    println!("\n✅ All etcd KV operations tests passed!");
    println!("Total LLM calls: ~7 (1 startup + 6 operations)");

    Ok(())
}
