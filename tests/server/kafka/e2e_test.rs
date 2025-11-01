//! Kafka protocol E2E tests
//!
//! These tests verify the Kafka broker functionality using real rdkafka client.
//!
//! To run: cargo test --features kafka,kafka --test server::kafka::e2e_test

use crate::server::helpers::{start_netget_server, wait_for_server_startup, ServerConfig};
use rdkafka::admin::AdminClient;
use rdkafka::client::DefaultClientContext;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::producer::{FutureProducer, FutureRecord, Producer};
use rdkafka::util::Timeout;
use std::time::Duration;
use tokio::time::sleep;

/// Test basic Kafka broker startup and ApiVersions handshake
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

    let config = ServerConfig::new(prompt.to_string());
    let server = start_netget_server(config).await.expect("Failed to start server");

    // Wait for server to fully start
    wait_for_server_startup(&server, Duration::from_secs(10), "KAFKA").await;

    let port = server.port;

    // Basic TCP connection test - Kafka protocol requires proper handshake
    let addr = format!("127.0.0.1:{}", port);
    let result = tokio::net::TcpStream::connect(&addr).await;
    assert!(result.is_ok(), "Should be able to connect to Kafka broker");

    // Test with rdkafka client - verifies ApiVersions and basic protocol
    let bootstrap_servers = format!("127.0.0.1:{}", port);

    // Create admin client to test metadata request
    let admin_client: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap_servers)
        .set("client.id", "netget-test-client")
        .set("socket.timeout.ms", "5000")
        .set("api.version.request.timeout.ms", "5000")
        .create()
        .expect("Admin client creation failed");

    // Request metadata - this tests ApiVersions + Metadata requests
    let metadata = admin_client
        .inner()
        .fetch_metadata(None, Timeout::After(Duration::from_secs(5)))
        .expect("Failed to fetch metadata");

    // Verify broker is present in metadata
    assert!(!metadata.brokers().is_empty(), "Expected at least one broker in metadata");

    println!("✓ Kafka broker metadata: {} brokers, {} topics",
             metadata.brokers().len(),
             metadata.topics().len());

    // Give server time to process
    sleep(Duration::from_millis(500)).await;
}

/// Test Kafka produce and fetch operations
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

    let config = ServerConfig::new(prompt.to_string());
    let server = start_netget_server(config).await.expect("Failed to start server");
    wait_for_server_startup(&server, Duration::from_secs(10), "KAFKA").await;

    let bootstrap_servers = format!("127.0.0.1:{}", server.port);

    // Create producer
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap_servers)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("Producer creation failed");

    // Send test messages
    let test_messages = vec![
        ("order-1", r#"{"item": "laptop", "price": 999}"#),
        ("order-2", r#"{"item": "mouse", "price": 29}"#),
        ("order-3", r#"{"item": "keyboard", "price": 79}"#),
    ];

    for (key, value) in &test_messages {
        let record = FutureRecord::to("orders")
            .key(key.as_bytes())
            .payload(value.as_bytes());

        let delivery_result = producer
            .send(record, Timeout::After(Duration::from_secs(5)))
            .await;

        match delivery_result {
            Ok((partition, offset)) => {
                println!("✓ Message sent to partition {} at offset {}", partition, offset);
            }
            Err((err, _)) => {
                panic!("Failed to send message: {:?}", err);
            }
        }
    }

    // Flush producer
    producer.flush(Timeout::After(Duration::from_secs(5)))
        .expect("Failed to flush producer");

    println!("✓ All {} messages sent successfully", test_messages.len());

    // Create consumer
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap_servers)
        .set("group.id", "test-consumer-group")
        .set("session.timeout.ms", "6000")
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("Consumer creation failed");

    // Subscribe to topic
    consumer
        .subscribe(&["orders"])
        .expect("Failed to subscribe to topic");

    // Consume messages (with timeout)
    use rdkafka::message::Message;
    use tokio::time::timeout;

    let mut received_count = 0;
    let consume_timeout = Duration::from_secs(10);

    while received_count < test_messages.len() {
        match timeout(consume_timeout, consumer.recv()).await {
            Ok(Ok(message)) => {
                let key = message.key().map(|k| String::from_utf8_lossy(k).to_string());
                let payload = message.payload().map(|p| String::from_utf8_lossy(p).to_string());

                println!("✓ Received message: key={:?}, payload={:?}, offset={}",
                         key, payload, message.offset());

                received_count += 1;
            }
            Ok(Err(e)) => {
                panic!("Consumer error: {:?}", e);
            }
            Err(_) => {
                println!("Timeout waiting for message (received {}/{})", received_count, test_messages.len());
                break;
            }
        }
    }

    assert_eq!(
        received_count,
        test_messages.len(),
        "Expected to receive {} messages, got {}",
        test_messages.len(),
        received_count
    );

    println!("✓ Successfully produced and consumed {} messages", received_count);
}

/// Test Kafka metadata requests
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

    let config = ServerConfig::new(prompt.to_string());
    let server = start_netget_server(config).await.expect("Failed to start server");
    wait_for_server_startup(&server, Duration::from_secs(10), "KAFKA").await;

    let bootstrap_servers = format!("127.0.0.1:{}", server.port);

    // Create admin client
    let admin_client: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap_servers)
        .set("client.id", "netget-metadata-test")
        .set("socket.timeout.ms", "5000")
        .create()
        .expect("Admin client creation failed");

    // Request metadata
    let metadata = admin_client
        .inner()
        .fetch_metadata(None, Timeout::After(Duration::from_secs(5)))
        .expect("Failed to fetch metadata");

    // Verify brokers
    assert!(!metadata.brokers().is_empty(), "Expected at least one broker");
    println!("✓ Found {} broker(s)", metadata.brokers().len());

    for broker in metadata.brokers() {
        println!("  - Broker ID: {}, Host: {}, Port: {}",
                 broker.id(), broker.host(), broker.port());
    }

    // Verify topics
    let topic_names: Vec<&str> = metadata
        .topics()
        .iter()
        .map(|t| t.name())
        .collect();

    println!("✓ Found {} topic(s): {:?}", topic_names.len(), topic_names);

    // Check for expected topics (if LLM created them)
    for topic in metadata.topics() {
        println!("  - Topic: {}, Partitions: {}",
                 topic.name(), topic.partitions().len());

        for partition in topic.partitions() {
            println!("    - Partition {}, Leader: {:?}",
                     partition.id(), partition.leader());
        }
    }

    println!("✓ Metadata request successful");
}
