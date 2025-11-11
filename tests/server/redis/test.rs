//! End-to-end Redis tests for NetGet
//!
//! These tests spawn the actual NetGet binary with Redis prompts
//! and validate the responses using Redis protocol clients.

#![cfg(feature = "redis")]

// Helper module imported from parent

use super::super::super::helpers::{self, E2EResult, ServerConfig};
use redis::AsyncCommands;
use std::time::Duration;

#[tokio::test]
async fn test_redis_ping() -> E2EResult<()> {
    println!("\n=== E2E Test: Redis PING ===");

    // PROMPT: Tell the LLM to act as a Redis server
    let prompt = "Open Redis on port {AVAILABLE_PORT}. For any command (PING, CLIENT, etc), use redis_simple_string \
        action with value='PONG' for PING, value='OK' for all others. For GET commands, use redis_bulk_string with value='hello'. \
        For SET commands, use redis_simple_string value='OK'.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and execute PING using redis client
    println!("Connecting to Redis server...");

    let redis_url = format!("redis://127.0.0.1:{}", server.port);
    let client = redis::Client::open(redis_url.as_str())?;

    let mut con = match tokio::time::timeout(
        Duration::from_secs(10),
        client.get_multiplexed_async_connection(),
    )
    .await
    {
        Ok(Ok(con)) => {
            println!("✓ Redis connected");
            con
        }
        Ok(Err(e)) => {
            println!("✗ Redis connection error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ Redis connection timeout");
            return Err("Connection timeout".into());
        }
    };

    // Execute PING command
    println!("Executing PING...");
    let pong: String = match tokio::time::timeout(
        Duration::from_secs(10),
        redis::cmd("PING").query_async(&mut con),
    )
    .await
    {
        Ok(Ok(pong)) => pong,
        Ok(Err(e)) => {
            println!("✗ PING error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ PING timeout");
            return Err("PING timeout".into());
        }
    };

    println!("✓ Received: {}", pong);
    assert_eq!(pong, "PONG", "Expected PING to return PONG");

    println!("✓ Redis PING test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_redis_get_set() -> E2EResult<()> {
    println!("\n=== E2E Test: Redis GET/SET ===");

    let prompt = "Open Redis on port {AVAILABLE_PORT}. For PING/CLIENT commands, use redis_simple_string value='OK'. \
        For SET commands, use redis_simple_string value='OK'. \
        For GET key commands, use redis_bulk_string value='test_value'.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    println!("Connecting to Redis server...");
    let redis_url = format!("redis://127.0.0.1:{}", server.port);
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;
    println!("✓ Redis connected");

    // Test SET command
    println!("Executing SET mykey myvalue...");
    let result: String = con.set("mykey", "myvalue").await?;
    println!("✓ SET result: {}", result);
    assert_eq!(result, "OK", "Expected SET to return OK");

    // Test GET command
    println!("Executing GET mykey...");
    let value: String = con.get("mykey").await?;
    println!("✓ GET result: {}", value);
    assert_eq!(value, "test_value", "Expected GET to return test_value");

    println!("✓ Redis GET/SET test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_redis_integer_response() -> E2EResult<()> {
    println!("\n=== E2E Test: Redis Integer Response ===");

    let prompt = "Open Redis on port {AVAILABLE_PORT}. For PING/CLIENT commands, use redis_simple_string value='OK'. \
        For INCR commands, use redis_integer value=42. \
        For DEL commands, use redis_integer value=1.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    println!("Connecting to Redis server...");
    let redis_url = format!("redis://127.0.0.1:{}", server.port);
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;
    println!("✓ Redis connected");

    // Test INCR command (returns integer)
    println!("Executing INCR counter...");
    let result: i64 = con.incr("counter", 1).await?;
    println!("✓ INCR result: {}", result);
    assert_eq!(result, 42, "Expected INCR to return 42");

    println!("✓ Redis integer response test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_redis_array_response() -> E2EResult<()> {
    println!("\n=== E2E Test: Redis Array Response ===");

    let prompt = "Open Redis on port {AVAILABLE_PORT}. For PING/CLIENT commands, use redis_simple_string value='OK'. \
        For MGET commands, use redis_array values=['value1','value2','value3']. \
        For KEYS commands, use redis_array values=['key1','key2','key3'].";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    println!("Connecting to Redis server...");
    let redis_url = format!("redis://127.0.0.1:{}", server.port);
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;
    println!("✓ Redis connected");

    // Test KEYS command (returns array)
    println!("Executing KEYS *...");
    let keys: Vec<String> = redis::cmd("KEYS").arg("*").query_async(&mut con).await?;
    println!("✓ KEYS result: {} keys", keys.len());
    for key in &keys {
        println!("  - {}", key);
    }
    assert!(!keys.is_empty(), "Expected at least one key");

    println!("✓ Redis array response test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_redis_null_response() -> E2EResult<()> {
    println!("\n=== E2E Test: Redis Null Response ===");

    let prompt = "Open Redis on port {AVAILABLE_PORT}. For PING/CLIENT commands, use redis_simple_string value='OK'. \
        For GET nonexistent commands, use redis_null. \
        For other GET commands, use redis_bulk_string value='exists'.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    println!("Connecting to Redis server...");
    let redis_url = format!("redis://127.0.0.1:{}", server.port);
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;
    println!("✓ Redis connected");

    // Test GET for nonexistent key (should return nil)
    println!("Executing GET nonexistent...");
    let value: Option<String> = con.get("nonexistent").await?;
    println!("✓ GET result: {:?}", value);
    assert_eq!(value, None, "Expected GET nonexistent to return None");

    println!("✓ Redis null response test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_redis_error_response() -> E2EResult<()> {
    println!("\n=== E2E Test: Redis Error Response ===");

    let prompt = "Open Redis on port {AVAILABLE_PORT}. For PING/CLIENT commands, use redis_simple_string value='OK'. \
        For commands containing 'INVALID', use redis_error message='ERR unknown command'. \
        For other commands, use redis_simple_string value='OK'.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    println!("Connecting to Redis server...");
    let redis_url = format!("redis://127.0.0.1:{}", server.port);
    let client = redis::Client::open(redis_url.as_str())?;
    let mut con = client.get_multiplexed_async_connection().await?;
    println!("✓ Redis connected");

    // Test invalid command (should return error)
    println!("Executing INVALID command...");
    let result: Result<String, redis::RedisError> =
        redis::cmd("INVALID").query_async(&mut con).await;

    match result {
        Ok(_) => {
            println!("✗ Expected error but command succeeded");
            return Err("Expected error response".into());
        }
        Err(e) => {
            println!("✓ Received error as expected: {}", e);
            let err_str = e.to_string();
            assert!(
                err_str.contains("ERR") || err_str.contains("unknown"),
                "Error message should contain expected text"
            );
        }
    }

    println!("✓ Redis error response test passed\n");
    Ok(())
}
