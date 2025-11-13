//! E2E tests for TCP client
//!
//! These tests verify TCP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "tcp"))]
mod tcp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test TCP client connection to a local server with data exchange
    /// LLM calls: 4 (server startup, server data received, client startup, client connected)
    #[tokio::test]
    async fn test_tcp_client_connect_to_server() -> E2EResult<()> {
        // Start a TCP server listening on an available port with mocks
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via TCP. Accept one connection, echo received data back.")
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup (user command)
                    .on_instruction_containing("Listen on port")
                    .and_instruction_containing("TCP")
                    .and_instruction_containing("echo")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "TCP",
                            "instruction": "Echo server - respond with exactly what is received"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Server receives data (tcp_data_received event)
                    .on_event("tcp_data_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_tcp_data",
                            "data": "48454c4c4f" // "HELLO" in hex
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a TCP client that connects to this server with mocks
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via TCP. Send 'HELLO' and wait for response.",
            server.port
        ))
            .with_mock(|mock| {
                mock
                    // Mock 1: Client startup (user command)
                    .on_instruction_containing("Connect to")
                    .and_instruction_containing("TCP")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": format!("127.0.0.1:{}", server.port),
                            "protocol": "TCP",
                            "instruction": "Send HELLO and wait for echo"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Client connected (tcp_connected event)
                    .on_event("tcp_connected")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_tcp_data",
                            "data": "48454c4c4f" // "HELLO" in hex
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: Client receives echo response (tcp_data_received event)
                    .on_event("tcp_data_received")
                    .and_event_data_contains("data", "48454c4c4f")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give client time to connect and exchange data
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ TCP client connected to server and exchanged data successfully");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test TCP client can be controlled via prompts with mocks
    /// LLM calls: 4 (server startup, client startup, connection, data received)
    #[tokio::test]
    async fn test_tcp_client_command_via_prompt() -> E2EResult<()> {
        // Start a simple TCP server with mocks
        let server_config =
            NetGetConfig::new("Listen on port {AVAILABLE_PORT} via TCP. Log all incoming data.")
            .with_mock(|mock| {
                mock
                    // Mock: Server startup
                    .on_instruction_containing("TCP")
                    .and_instruction_containing("Log")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "TCP",
                            "instruction": "Log all data"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock: Server receives data
                    .on_event("tcp_data_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that sends specific data based on LLM instruction with mocks
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via TCP and send the string 'TEST_DATA' then disconnect.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock: Client startup
                .on_instruction_containing("TCP")
                .and_instruction_containing("TEST_DATA")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "TCP",
                        "instruction": "Send TEST_DATA then disconnect"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock: Client connected
                .on_event("tcp_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_tcp_data",
                        "data": "544553545f44415441" // "TEST_DATA" in hex
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client initiated the connection
        assert_eq!(client.protocol, "TCP", "Client should be TCP protocol");

        println!("✅ TCP client responded to LLM instruction");

        // Verify mocks
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
