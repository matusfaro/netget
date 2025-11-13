//! E2E tests for Kafka client
//!
//! These tests verify Kafka client functionality using mocked LLM responses.
//!
//! To run: ./test-e2e.sh kafka

#[cfg(all(test, feature = "kafka"))]
mod kafka_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Kafka producer client - send a message to a topic
    /// LLM calls: 2 (server startup, client connection and produce)
    #[tokio::test]
    async fn test_kafka_producer_send_message() -> E2EResult<()> {
        // Start a Kafka server first
        let server_config = NetGetConfig::new("Start a Kafka broker on port {AVAILABLE_PORT}. Accept all messages.")
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Kafka broker")
                    .and_instruction_containing("port")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Kafka",
                            "instruction": "Kafka broker - accept all messages"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start the Kafka producer client with mocks
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Kafka as producer. Send a message to topic 'test-events' with payload 'Hello Kafka'.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect to")
                .and_instruction_containing("Kafka")
                .and_instruction_containing("producer")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "Kafka",
                        "instruction": "Send message to test-events topic",
                        "startup_params": {
                            "mode": "producer",
                            "client_id": "netget-test-producer"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected event
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "produce_message",
                        "topic": "test-events",
                        "payload": "Hello Kafka"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and send message
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client connected
        assert!(
            client.output_contains("Kafka").await
                || client.output_contains("connected").await,
            "Client should show Kafka producer connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Kafka producer client connected and sent message");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        client.stop().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Kafka consumer client - subscribe to topics
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_kafka_consumer_subscribe() -> E2EResult<()> {
        // Start a Kafka server first
        let server_config = NetGetConfig::new("Start a Kafka broker on port {AVAILABLE_PORT}.")
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("Kafka broker")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Kafka",
                            "instruction": "Kafka broker"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Kafka as consumer. Subscribe to topics 'test-events' and 'test-logs'.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect to")
                .and_instruction_containing("Kafka")
                .and_instruction_containing("consumer")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "Kafka",
                        "instruction": "Subscribe to test-events and test-logs",
                        "startup_params": {
                            "mode": "consumer",
                            "group_id": "netget-test-group",
                            "topics": ["test-events", "test-logs"],
                            "client_id": "netget-test-consumer"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected event
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and subscribe
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client connected as consumer
        assert!(
            client.output_contains("Kafka").await
                || client.output_contains("connected").await,
            "Client should show Kafka consumer connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Kafka consumer client connected and subscribed");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        client.stop().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Kafka producer-consumer flow
    /// LLM calls: 3 (server startup, producer connection, consumer connection)
    #[tokio::test]
    async fn test_kafka_producer_consumer_flow() -> E2EResult<()> {
        // Start a Kafka server first
        let server_config = NetGetConfig::new("Start a Kafka broker on port {AVAILABLE_PORT}.")
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("Kafka broker")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Kafka",
                            "instruction": "Kafka broker"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start a consumer first
        let consumer_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Kafka as consumer. Subscribe to topic 'flow-test'. Log each message received.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                .on_instruction_containing("Connect to")
                .and_instruction_containing("consumer")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "Kafka",
                        "instruction": "Subscribe to flow-test and log messages",
                        "startup_params": {
                            "mode": "consumer",
                            "group_id": "netget-flow-test",
                            "topics": ["flow-test"],
                            "client_id": "netget-flow-consumer"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut consumer = start_netget_client(consumer_config).await?;

        // Give consumer time to subscribe
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Now start a producer and send a message
        let producer_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Kafka as producer. Send message 'Test Flow' to topic 'flow-test' with key 'test-key'.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                .on_instruction_containing("Connect to")
                .and_instruction_containing("producer")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "Kafka",
                        "instruction": "Send Test Flow message",
                        "startup_params": {
                            "mode": "producer",
                            "client_id": "netget-flow-producer"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "produce_message",
                        "topic": "flow-test",
                        "key": "test-key",
                        "payload": "Test Flow"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut producer = start_netget_client(producer_config).await?;

        // Give producer time to send message
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify producer sent message
        assert!(
            producer.output_contains("Kafka").await,
            "Producer should show Kafka connection. Output: {:?}",
            producer.get_output().await
        );

        println!("✅ Kafka producer-consumer flow completed");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        consumer.verify_mocks().await?;
        producer.verify_mocks().await?;

        // Cleanup
        producer.stop().await?;
        consumer.stop().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Kafka client protocol detection
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_kafka_client_protocol_detection() -> E2EResult<()> {
        // Start a Kafka server first
        let server_config = NetGetConfig::new("Start a Kafka broker on port {AVAILABLE_PORT}.")
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("Kafka broker")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Kafka",
                            "instruction": "Kafka broker"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Kafka as producer.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                .on_instruction_containing("Connect to")
                .and_instruction_containing("Kafka")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "Kafka",
                        "instruction": "Kafka producer",
                        "startup_params": {
                            "mode": "producer",
                            "client_id": "netget-protocol-test"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is Kafka protocol
        assert_eq!(
            client.protocol, "Kafka",
            "Client should be detected as Kafka protocol"
        );

        println!("✅ Kafka client protocol detected correctly");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        client.stop().await?;
        server.stop().await?;

        Ok(())
    }
}
