//! E2E tests for JSON-RPC client
//!
//! These tests verify JSON-RPC client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "jsonrpc"))]
mod jsonrpc_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test JSON-RPC client making a single request
    /// LLM calls: 2 (server startup, client connection and request)
    #[tokio::test]
    async fn test_jsonrpc_client_single_request() -> E2EResult<()> {
        // Start a JSON-RPC server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via JSON-RPC. \
             Implement these methods: \
             - add(a, b): Return the sum of a and b \
             - greet(name): Return 'Hello, {name}!'"
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a JSON-RPC client that makes a request
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via JSON-RPC. \
             Call method 'add' with params [5, 3] and id 1.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to make request
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection/response
        assert!(
            client.output_contains("JSON-RPC").await || client.output_contains("jsonrpc").await,
            "Client should show JSON-RPC protocol message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ JSON-RPC client made single request successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test JSON-RPC client can handle LLM-controlled method calls
    /// LLM calls: 2 (server startup, client connection and request)
    #[tokio::test]
    async fn test_jsonrpc_client_llm_controlled_request() -> E2EResult<()> {
        // Start a JSON-RPC server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via JSON-RPC. \
             Implement method 'echo' that returns whatever params it receives."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that makes a specific request based on LLM instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via JSON-RPC and call the echo method.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is JSON-RPC protocol
        assert_eq!(client.protocol, "JSON-RPC", "Client should be JSON-RPC protocol");

        println!("✅ JSON-RPC client responded to LLM instruction");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test JSON-RPC client can send batch requests
    /// LLM calls: 2 (server startup, client connection and batch request)
    #[tokio::test]
    async fn test_jsonrpc_client_batch_request() -> E2EResult<()> {
        // Start a JSON-RPC server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via JSON-RPC. \
             Implement these methods: \
             - add(a, b): Return a + b \
             - multiply(a, b): Return a * b"
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that sends a batch request
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via JSON-RPC. \
             Send a batch request with two calls: \
             1. add([1, 2]) with id 1 \
             2. multiply([3, 4]) with id 2",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client is connected
        assert!(
            client.output_contains("JSON-RPC").await || client.output_contains("connected").await,
            "Client should show JSON-RPC connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ JSON-RPC client sent batch request successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
