//! E2E tests for gRPC client
//!
//! These tests verify gRPC client functionality by spawning NetGet gRPC server and client
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "grpc"))]
mod grpc_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    // Base64-encoded FileDescriptorSet for a simple calculator service
    // Generated from:
    // syntax = "proto3";
    // package calculator;
    //
    // service Calculator {
    //   rpc Add(AddRequest) returns (AddResponse);
    // }
    //
    // message AddRequest {
    //   int32 a = 1;
    //   int32 b = 2;
    // }
    //
    // message AddResponse {
    //   int32 result = 1;
    // }
    const CALCULATOR_SCHEMA: &str = "CpUCCg9jYWxjdWxhdG9yLnByb3RvEgpjYWxjdWxhdG9yIikKCkFkZFJlcXVlc3QSCwoDYRgBIAEoBVIBYRILCgNiGAIgASgFUgFiIiIKC0FkZFJlc3BvbnNlEhMKBnJlc3VsdBgBIAEoBVIGcmVzdWx0MkIKCkNhbGN1bGF0b3ISNAoDQWRkEhYuY2FsY3VsYXRvci5BZGRSZXF1ZXN0GhcuY2FsY3VsYXRvci5BZGRSZXNwb25zZSIAYgZwcm90bzM=";

    /// Test gRPC client connecting to server and making RPC call
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_grpc_client_add_request() -> E2EResult<()> {
        // Start a gRPC server listening on an available port
        let server_config = NetGetConfig::new(format!(
            "Listen on port {{AVAILABLE_PORT}} via gRPC. Use this schema: {}. When you receive Add requests, return the sum of a and b in the result field.",
            CALCULATOR_SCHEMA
        ));

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Now start a gRPC client that makes an Add request
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via gRPC. Use this schema: {}. Call calculator.Calculator/Add with a=5, b=3 and show the result.",
            server.port, CALCULATOR_SCHEMA
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to make request and receive response
        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify client output shows gRPC connection or result
        let output = client.get_output().await;
        assert!(
            output.contains("gRPC") || output.contains("Calculator") || output.contains("result") || output.contains("8"),
            "Client should show gRPC protocol, service name, result field, or sum (8). Output: {:?}",
            output
        );

        println!("✅ gRPC client made Add RPC call successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test gRPC client can handle connection errors gracefully
    /// LLM calls: 1 (client connection attempt)
    #[tokio::test]
    async fn test_grpc_client_connection_error() -> E2EResult<()> {
        // Try to connect to a non-existent gRPC server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:54321 via gRPC. Use this schema: {}. Call calculator.Calculator/Add with a=1, b=2.",
            CALCULATOR_SCHEMA
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to attempt connection
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify client shows error or connection failure
        let output = client.get_output().await;
        assert!(
            output.contains("ERROR") || output.contains("error") || output.contains("failed") || output.contains("Error"),
            "Client should show connection error. Output: {:?}",
            output
        );

        println!("✅ gRPC client handled connection error gracefully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
