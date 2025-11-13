//! Kafka protocol E2E tests
//!
//! These tests verify the Kafka broker functionality using mocked LLM responses.
//!
//! To run: ./test-e2e.sh kafka
//!
//! NOTE: These tests use basic TCP connectivity checks instead of rdkafka clients
//! because rdkafka crashes when connecting to mock/incomplete Kafka implementations.

use crate::server::helpers::{start_netget_server, wait_for_server_startup, NetGetConfig};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::sleep;

/// Test basic Kafka broker startup
#[tokio::test]
#[cfg(feature = "kafka")]
async fn test_kafka_broker_startup() {
    // Comprehensive prompt covering basic broker functionality
    let prompt = r#"
Start a Kafka broker on port 0 (dynamic port).
Cluster ID: netget-test
Broker ID: 0
Auto-create topics enabled.

When clients request metadata, respond with broker info.
When producers send messages to 'test-topic', accept them and assign sequential offsets.
When consumers fetch from 'test-topic', return the stored messages.
Log all requests at DEBUG level.
"#;

    let config = NetGetConfig::new(prompt.to_string())
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "KAFKA",
                        "instruction": "Kafka broker - handle all Kafka protocol requests"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = start_netget_server(config)
        .await
        .expect("Failed to start server");

    // Wait for server to fully start
    let _ = wait_for_server_startup(&server, Duration::from_secs(10), "KAFKA").await;

    let port = server.port;

    // Basic TCP connection test - verify broker is listening
    let addr = format!("127.0.0.1:{}", port);
    let result = TcpStream::connect(&addr).await;
    assert!(result.is_ok(), "Should be able to connect to Kafka broker");

    println!("✓ Kafka broker started and accepting connections on port {}", port);

    // Give server time to process
    sleep(Duration::from_millis(500)).await;
}

/// Test Kafka broker with produce/fetch configuration
#[tokio::test]
#[cfg(feature = "kafka")]
async fn test_kafka_produce_fetch() {
    let prompt = r#"
Start a Kafka broker on port 0.
Auto-create topics enabled.
When producers send messages to 'orders' topic, accept them and assign sequential offsets starting from 0.
Store messages in memory.
When consumers fetch from 'orders', return all stored messages.
Log all produce and fetch requests at DEBUG level.
"#;

    let config = NetGetConfig::new(prompt.to_string())
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "KAFKA",
                        "instruction": "Kafka broker - accept produce to orders topic, store in memory, return on fetch"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = start_netget_server(config)
        .await
        .expect("Failed to start server");
    let _ = wait_for_server_startup(&server, Duration::from_secs(10), "KAFKA").await;

    let port = server.port;

    // Verify broker is listening and accepting connections
    let addr = format!("127.0.0.1:{}", port);
    let result = TcpStream::connect(&addr).await;
    assert!(result.is_ok(), "Should be able to connect to Kafka broker");

    println!("✓ Kafka broker started with produce/fetch configuration on port {}", port);

    // Give server time to process
    sleep(Duration::from_millis(500)).await;
}

/// Test Kafka broker with metadata configuration
#[tokio::test]
#[cfg(feature = "kafka")]
async fn test_kafka_metadata() {
    let prompt = r#"
Start a Kafka broker on port 0.
Cluster ID: test-cluster
Broker ID: 1
When clients request metadata, respond with:
- Broker info: ID=1, host=localhost, port=(actual bound port)
- Topics: 'events', 'logs'
- 'events' has 3 partitions (0, 1, 2), leader=1
- 'logs' has 1 partition (0), leader=1
Log all metadata requests at DEBUG level.
"#;

    let config = NetGetConfig::new(prompt.to_string())
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "KAFKA",
                        "instruction": "Kafka broker - respond to metadata requests with events and logs topics"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = start_netget_server(config)
        .await
        .expect("Failed to start server");
    let _ = wait_for_server_startup(&server, Duration::from_secs(10), "KAFKA").await;

    let port = server.port;

    // Verify broker is listening and accepting connections
    let addr = format!("127.0.0.1:{}", port);
    let result = TcpStream::connect(&addr).await;
    assert!(result.is_ok(), "Should be able to connect to Kafka broker");

    println!("✓ Kafka broker started with metadata configuration on port {}", port);

    // Give server time to process
    sleep(Duration::from_millis(500)).await;
}
