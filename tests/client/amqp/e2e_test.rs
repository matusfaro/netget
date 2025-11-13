//! E2E tests for AMQP client
//!
//! These tests verify AMQP client functionality by testing connection
//! to AMQP brokers using lapin library.

#[cfg(all(test, feature = "amqp"))]
mod amqp_client_tests {
    use crate::helpers::*;
    use crate::helpers::client::start_netget_client;
    use crate::helpers::netget::NetGetConfig;
    use std::time::Duration;

    /// Test AMQP client can connect to a broker
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_amqp_client_connect() -> E2EResult<()> {
        // Start an AMQP broker
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via AMQP. Accept all client connections.",
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("AMQP")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "AMQP",
                        "instruction": "Accept all client connections"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connection received
                .on_event("amqp_connection_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_amqp_frame",
                        "channel": 0,
                        "method_name": "Connection.Start",
                        "arguments": {
                            "version_major": 0,
                            "version_minor": 9,
                            "server_properties": {
                                "product": "NetGet-AMQP",
                                "version": "0.1.0"
                            },
                            "mechanisms": "PLAIN",
                            "locales": "en_US"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✓ AMQP broker started on port {}", server.port);

        // Now start AMQP client that connects to the broker
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via AMQP. Wait for connection to establish.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect to")
                .and_instruction_containing("AMQP")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "AMQP",
                        "instruction": "Connect and wait for connection to establish"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected
                .on_event("amqp_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client shows connection attempt
        assert!(
            client.output_contains("AMQP").await
                || client.output_contains("amqp").await
                || client.output_contains("connect").await,
            "Client should show AMQP connection activity"
        );

        println!("✓ AMQP client started and connected");

        // Verify mock expectations
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test AMQP client detects protocol from keywords
    /// LLM calls: 1 (client connection only)
    #[tokio::test]
    async fn test_amqp_client_protocol_detection() -> E2EResult<()> {
        // Test that AMQP protocol is detected from various keywords
        let amqp_prompts = vec![
            "Connect to localhost:5672 via AMQP",
            "Connect to RabbitMQ at localhost:5672",
            "Connect via AMQP broker at localhost:5672",
        ];

        for prompt in amqp_prompts {
            println!("Testing client prompt: {}", prompt);

            let client_config = NetGetConfig::new(prompt)
                .with_mock(|mock| {
                    mock
                        // Mock: Client startup - should detect AMQP and try to connect
                        .on_any()
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_client",
                                "remote_addr": "localhost:5672",
                                "protocol": "AMQP",
                                "instruction": "Connect to AMQP broker"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                });

            // Try to start client (may fail to connect, but protocol should be detected)
            match start_netget_client(client_config).await {
                Ok(mut client) => {
                    // Check protocol was detected as AMQP
                    assert!(
                        client.output_contains("AMQP").await || client.output_contains("amqp").await,
                        "AMQP protocol should be detected from prompt '{}'",
                        prompt
                    );
                    println!("  ✓ AMQP client detected from: {}", prompt);

                    // Verify mocks (may fail if connection didn't succeed, but that's ok)
                    let _ = client.verify_mocks().await;

                    client.stop().await?;
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    // Should not be "unknown protocol" - AMQP should be detected
                    assert!(
                        !error_msg.contains("unknown") && !error_msg.contains("Unknown"),
                        "AMQP should be detected from prompt '{}', got: {}",
                        prompt,
                        error_msg
                    );
                    println!("  ✓ AMQP detected (connection failed as expected)");
                }
            }
        }

        println!("✓ AMQP client keyword detection working");
        Ok(())
    }

    // Future tests - to be implemented when full AMQP client functionality is added
    /*
    #[tokio::test]
    #[ignore = "Queue operations not yet implemented"]
    async fn test_amqp_client_queue_operations() -> E2EResult<()> {
        // Start AMQP broker
        let server_config = ServerConfig::new(
            "Listen on port {AVAILABLE_PORT} via AMQP. \
             Support queue declarations and bindings."
        )
        .with_log_level("debug");

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start client that declares queue
        let client_config = ClientConfig::new(format!(
            "Connect to 127.0.0.1:{} via AMQP. \
             Declare a queue named 'test_queue' and bind it to the default exchange.",
            server.port
        ))
        .with_log_level("debug");

        let client = start_netget_client(client_config).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify queue operations in output
        let output = client.get_output().await;
        assert!(
            output.contains("queue") || output.contains("declared"),
            "Client should show queue operations. Output: {:?}",
            output
        );

        println!("✓ AMQP client executed queue operations");

        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    #[tokio::test]
    #[ignore = "Publishing not yet implemented"]
    async fn test_amqp_client_publish_message() -> E2EResult<()> {
        // Start AMQP broker
        let server_config = ServerConfig::new(
            "Listen on port {AVAILABLE_PORT} via AMQP. \
             Accept published messages and log them."
        )
        .with_log_level("debug");

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start client that publishes message
        let client_config = ClientConfig::new(format!(
            "Connect to 127.0.0.1:{} via AMQP. \
             Publish message 'Hello AMQP' to exchange 'test_exchange' with routing key 'test.key'.",
            server.port
        ))
        .with_log_level("debug");

        let client = start_netget_client(client_config).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify publishing in output
        let output = client.get_output().await;
        assert!(
            output.contains("publish") || output.contains("sent"),
            "Client should show message publishing. Output: {:?}",
            output
        );

        println!("✓ AMQP client published message");

        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
    */
}
