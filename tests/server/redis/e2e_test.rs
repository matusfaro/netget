//! E2E tests for Redis server with mocks
//!
//! These tests verify Redis server functionality using mock LLM responses.
//! Test strategy: Mock Redis RESP protocol responses, < 10 LLM calls total.

#[cfg(all(test, feature = "redis"))]
mod redis_server_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Redis PING command with mocks
    /// LLM calls: 2 (server startup, redis_command event)
    #[tokio::test]
    async fn test_redis_ping_with_mocks() -> E2EResult<()> {
        // Start a Redis server with mocks
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Redis. Respond to PING with PONG."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Redis")
                .and_instruction_containing("PING")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Redis",
                        "instruction": "Respond to PING with PONG"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: PING command received
                .on_event("redis_command")
                .and_event_data_contains("command", "PING")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "redis_simple_string",
                        "value": "PONG"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Redis server responded to PING with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Redis GET/SET commands with mocks
    /// LLM calls: 3 (server startup, SET command, GET command)
    #[tokio::test]
    async fn test_redis_get_set_with_mocks() -> E2EResult<()> {
        // Start a Redis server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Redis. Handle GET and SET commands."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Redis")
                .and_instruction_containing("GET and SET")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Redis",
                        "instruction": "Handle GET and SET commands"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: SET command
                .on_event("redis_command")
                .and_event_data_contains("command", "SET")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "redis_simple_string",
                        "value": "OK"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: GET command
                .on_event("redis_command")
                .and_event_data_contains("command", "GET")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "redis_bulk_string",
                        "value": "test_value"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Redis server handled GET/SET commands with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Redis integer response with mocks
    /// LLM calls: 2 (server startup, INCR command)
    #[tokio::test]
    async fn test_redis_integer_response_with_mocks() -> E2EResult<()> {
        // Start a Redis server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Redis. Handle INCR command returning integer."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Redis")
                .and_instruction_containing("INCR")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Redis",
                        "instruction": "Handle INCR command"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: INCR command
                .on_event("redis_command")
                .and_event_data_contains("command", "INCR")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "redis_integer",
                        "value": 42
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Redis server returned integer response with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Redis array response with mocks
    /// LLM calls: 2 (server startup, KEYS command)
    #[tokio::test]
    async fn test_redis_array_response_with_mocks() -> E2EResult<()> {
        // Start a Redis server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Redis. Handle KEYS command returning array."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Redis")
                .and_instruction_containing("KEYS")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Redis",
                        "instruction": "Handle KEYS command"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: KEYS command
                .on_event("redis_command")
                .and_event_data_contains("command", "KEYS")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "redis_array",
                        "values": ["key1", "key2", "key3"]
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Redis server returned array response with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Redis null response with mocks
    /// LLM calls: 2 (server startup, GET nonexistent key)
    #[tokio::test]
    async fn test_redis_null_response_with_mocks() -> E2EResult<()> {
        // Start a Redis server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Redis. Return null for non-existent keys."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Redis")
                .and_instruction_containing("null")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Redis",
                        "instruction": "Return null for non-existent keys"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: GET non-existent key
                .on_event("redis_command")
                .and_event_data_contains("command", "GET")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "redis_null"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Redis server returned null response with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Redis error response with mocks
    /// LLM calls: 2 (server startup, invalid command)
    #[tokio::test]
    async fn test_redis_error_response_with_mocks() -> E2EResult<()> {
        // Start a Redis server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Redis. Return error for invalid commands."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Redis")
                .and_instruction_containing("error")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Redis",
                        "instruction": "Return error for invalid commands"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Invalid command
                .on_event("redis_command")
                .and_event_data_contains("command", "INVALID")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "redis_error",
                        "message": "ERR unknown command"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Redis server returned error response with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
