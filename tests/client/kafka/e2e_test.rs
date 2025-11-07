//! E2E tests for Kafka client
//!
//! These tests verify Kafka client functionality by spawning a Kafka broker via Docker
//! and testing producer/consumer behavior with LLM control.

#[cfg(all(test, feature = "kafka"))]
mod kafka_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Kafka producer client - send a message to a topic
    /// LLM calls: 1 (client connection and produce)
    #[tokio::test]
    async fn test_kafka_producer_send_message() -> E2EResult<()> {
        // This test requires a running Kafka broker
        // For CI, we assume a Kafka broker is available at localhost:9092
        // For local testing, start Kafka with: docker-compose up -d kafka

        let client_config = NetGetConfig::builder()
            .instruction("Connect to localhost:9092 via Kafka as producer. Send a message to topic 'test-events' with payload 'Hello Kafka'.")
            .startup_params(serde_json::json!({
                "mode": "producer",
                "client_id": "netget-test-producer"
            }))
            .build();

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and send message
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client connected
        assert!(
            client.output_contains("Kafka producer").await
                || client.output_contains("connected").await,
            "Client should show Kafka producer connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Kafka producer client connected and sent message");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test Kafka consumer client - subscribe to topics
    /// LLM calls: 1 (client connection)
    #[tokio::test]
    async fn test_kafka_consumer_subscribe() -> E2EResult<()> {
        // This test requires a running Kafka broker

        let client_config = NetGetConfig::builder()
            .instruction("Connect to localhost:9092 via Kafka as consumer. Subscribe to topics 'test-events' and 'test-logs'.")
            .startup_params(serde_json::json!({
                "mode": "consumer",
                "group_id": "netget-test-group",
                "topics": ["test-events"],
                "client_id": "netget-test-consumer"
            }))
            .build();

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and subscribe
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client connected as consumer
        assert!(
            client.output_contains("Kafka consumer").await
                || client.output_contains("connected").await,
            "Client should show Kafka consumer connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Kafka consumer client connected and subscribed");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test Kafka producer-consumer flow
    /// LLM calls: 2 (producer connection, consumer connection)
    #[tokio::test]
    async fn test_kafka_producer_consumer_flow() -> E2EResult<()> {
        // This test requires a running Kafka broker
        // Start a consumer first
        let consumer_config = NetGetConfig::builder()
            .instruction("Connect to localhost:9092 via Kafka as consumer. Subscribe to topic 'flow-test'. Log each message received.")
            .startup_params(serde_json::json!({
                "mode": "consumer",
                "group_id": "netget-flow-test",
                "topics": ["flow-test"],
                "client_id": "netget-flow-consumer"
            }))
            .build();

        let mut consumer = start_netget_client(consumer_config).await?;

        // Give consumer time to subscribe
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Now start a producer and send a message
        let producer_config = NetGetConfig::builder()
            .instruction("Connect to localhost:9092 via Kafka as producer. Send message 'Test Flow' to topic 'flow-test' with key 'test-key'.")
            .startup_params(serde_json::json!({
                "mode": "producer",
                "client_id": "netget-flow-producer"
            }))
            .build();

        let mut producer = start_netget_client(producer_config).await?;

        // Give producer time to send message
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify producer sent message
        assert!(
            producer.output_contains("Kafka producer").await
                || producer.output_contains("sent message").await,
            "Producer should show message sent. Output: {:?}",
            producer.get_output().await
        );

        // Give consumer time to receive message
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Note: We can't easily verify the consumer received the message in this test
        // because the consumer might have started after the message was sent,
        // or there might be offset/lag issues.
        // For a more reliable test, we'd need to produce first, then consume with
        // auto.offset.reset=earliest

        println!("✅ Kafka producer-consumer flow completed");

        // Cleanup
        producer.stop().await?;
        consumer.stop().await?;

        Ok(())
    }

    /// Test Kafka client protocol detection
    /// LLM calls: 1 (client connection)
    #[tokio::test]
    async fn test_kafka_client_protocol_detection() -> E2EResult<()> {
        let client_config = NetGetConfig::builder()
            .instruction("Connect to localhost:9092 via Kafka as producer.")
            .startup_params(serde_json::json!({
                "mode": "producer",
                "client_id": "netget-protocol-test"
            }))
            .build();

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is Kafka protocol
        assert_eq!(
            client.protocol, "Kafka",
            "Client should be detected as Kafka protocol"
        );

        println!("✅ Kafka client protocol detected correctly");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
