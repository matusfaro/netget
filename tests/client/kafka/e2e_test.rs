//! E2E tests for the Kafka client
//!
//! # Every test here is `#[ignore]`d, and none of them can run today
//!
//! **The Kafka client is not compiled into any build.** `src/client/mod.rs` gates it on
//! `#[cfg(all(feature = "kafka", feature = "rdkafka"))]`, and nothing enables `rdkafka`:
//! `Cargo.toml` declares it as an optional dependency that no feature depends on, annotated
//! `# rdkafka removed - causes malloc crashes`, and lists it among the features excluded from
//! `all-protocols`. So `src/client/kafka/` is dead code, `client_registry` never registers it,
//! and `open_client` with `protocol: "Kafka"` fails at runtime with
//!
//! > Client protocol 'Kafka' exists but is not compiled into this build
//! > (rebuild with --features kafka)
//!
//! — a message that is itself misleading, since `--features kafka` is exactly what was used.
//!
//! Verified: `cargo test --no-default-features --features kafka --test client -- kafka` before
//! this change reported **1 passed, 3 failed**. The one that "passed",
//! `test_kafka_client_protocol_detection`, asserted only `client.protocol == "Kafka"` — a
//! string the test harness parses out of a log line NetGet prints *before* the connect fails.
//! It passed with the client entirely non-functional, which is why it is now written to assert
//! a real produce and is ignored with the others.
//!
//! A second, independent blocker applies to the two consumer tests: NetGet's Kafka broker
//! implements ApiVersions, Metadata, Produce, Fetch and OffsetCommit only
//! (`src/server/kafka/CLAUDE.md`). Consumer groups need FindCoordinator, JoinGroup, SyncGroup
//! and Heartbeat, none of which exist, so an rdkafka `StreamConsumer` with a `group_id` cannot
//! join. Those tests need manual partition assignment before they can pass, which is a change
//! to `src/client/kafka/`.
//!
//! Both fixes are outside this suite. The mocks below are written for the shape the tests
//! should have when the client returns, so re-enabling them is a matter of deleting the
//! `#[ignore]` attributes:
//!
//! * the server mock answers the broker's own events (`kafka_metadata_request`,
//!   `kafka_produce_request`) and its `expect_calls` is what proves a message crossed the wire;
//! * the client mock answers `kafka_connected` with a real Kafka *client* action.
//!   `wait_for_more`, which every one of these rules used to return, is not one — the client's
//!   vocabulary is `produce_message`, `subscribe_topics`, `commit_offset` and `disconnect`, so
//!   `mock_action_names` will panic on the old mocks the moment the client is compiled in.

#[cfg(all(test, feature = "kafka"))]
mod kafka_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Kafka producer client - the broker must actually receive the records.
    /// LLM calls: 4 (server startup + metadata + produce, client startup + connected)
    #[tokio::test]
    #[ignore = "Kafka client is not compiled into any build: src/client/mod.rs gates it on \
                feature `rdkafka`, which Cargo.toml declares as an optional dependency no \
                feature enables (annotated 'rdkafka removed - causes malloc crashes'). \
                open_client fails with \"Client protocol 'Kafka' exists but is not compiled \
                into this build\". Re-enable rdkafka, or drop src/client/kafka, then delete \
                this attribute."]
    async fn test_kafka_producer_send_message() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Start a Kafka broker on port {AVAILABLE_PORT}. Accept all messages.",
        )
        .with_mock(|mock| {
            mock.on_instruction_containing("Kafka broker")
                .and_instruction_containing("port")
                .respond_with_actions(serde_json::json!([{
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "Kafka",
                    "instruction": "Kafka broker - accept all messages"
                }]))
                .expect_calls(1)
                .and()
                // A producer asks for metadata before it can address a partition leader.
                // Omitting `brokers` advertises this NetGet server itself.
                .on_event("kafka_metadata_request")
                .respond_with_actions(serde_json::json!([{
                    "type": "metadata_response",
                    "topics": [{"name": "test-events", "partitions": [{"partition": 0}]}]
                }]))
                .expect_at_most(3)
                .and()
                // The assertion: this fires only if the client's records reached the broker.
                .on_event("kafka_produce_request")
                .and_event_data_contains("topic", "test-events")
                .respond_with_actions(serde_json::json!([{
                    "type": "produce_response",
                    "topic": "test-events",
                    "partition": 0,
                    "offset": 0,
                    "error_code": 0
                }]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let remote = format!("127.0.0.1:{}", server.port);
        let client_config = NetGetConfig::new(format!(
            "Connect to {remote} via Kafka as producer. Send a message to topic 'test-events' \
             with payload 'Hello Kafka'."
        ))
        .with_mock(move |mock| {
            mock.on_instruction_containing("Connect to")
                .and_instruction_containing("Kafka")
                .and_instruction_containing("producer")
                .respond_with_actions(serde_json::json!([{
                    "type": "open_client",
                    "remote_addr": remote,
                    "protocol": "Kafka",
                    "instruction": "Send message to test-events topic",
                    "startup_params": {
                        "mode": "producer",
                        "client_id": "netget-test-producer"
                    }
                }]))
                .expect_calls(1)
                .and()
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([{
                    "type": "produce_message",
                    "topic": "test-events",
                    "payload": "Hello Kafka"
                }]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        println!("✅ Kafka producer client sent a message the broker received");

        server.verify_mocks().await?;
        client.verify_mocks().await?;

        client.stop().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Kafka consumer client - the broker must actually see the fetch.
    /// LLM calls: 4 (server startup + metadata + fetch, client startup + connected)
    #[tokio::test]
    #[ignore = "Two blockers. (1) The Kafka client is not compiled into any build — \
                src/client/mod.rs gates it on feature `rdkafka`, which no feature enables. \
                (2) Even compiled, an rdkafka StreamConsumer with a group_id cannot join: \
                NetGet's broker implements ApiVersions/Metadata/Produce/Fetch/OffsetCommit \
                only, and consumer groups need FindCoordinator/JoinGroup/SyncGroup/Heartbeat \
                (src/server/kafka/CLAUDE.md). Needs manual partition assignment in \
                src/client/kafka."]
    async fn test_kafka_consumer_subscribe() -> E2EResult<()> {
        let server_config = NetGetConfig::new("Start a Kafka broker on port {AVAILABLE_PORT}.")
            .with_mock(|mock| {
                mock.on_instruction_containing("Kafka broker")
                    .respond_with_actions(serde_json::json!([{
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Kafka",
                        "instruction": "Kafka broker"
                    }]))
                    .expect_calls(1)
                    .and()
                    .on_event("kafka_metadata_request")
                    .respond_with_actions(serde_json::json!([{
                        "type": "metadata_response",
                        "topics": [
                            {"name": "test-events", "partitions": [{"partition": 0}]},
                            {"name": "test-logs", "partitions": [{"partition": 0}]}
                        ]
                    }]))
                    .expect_at_most(3)
                    .and()
                    // The assertion: a subscribed consumer fetches. An empty record list is a
                    // valid answer meaning "nothing new".
                    .on_event("kafka_fetch_request")
                    .respond_with_actions(serde_json::json!([{
                        "type": "fetch_response",
                        "topic": "test-events",
                        "partition": 0,
                        "records": []
                    }]))
                    .expect_at_most(10)
                    .and()
            });

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let remote = format!("127.0.0.1:{}", server.port);
        let client_config = NetGetConfig::new(format!(
            "Connect to {remote} via Kafka as consumer. Subscribe to topics 'test-events' and \
             'test-logs'."
        ))
        .with_mock(move |mock| {
            mock.on_instruction_containing("Connect to")
                .and_instruction_containing("Kafka")
                .and_instruction_containing("consumer")
                .respond_with_actions(serde_json::json!([{
                    "type": "open_client",
                    "remote_addr": remote,
                    "protocol": "Kafka",
                    "instruction": "Subscribe to test-events and test-logs",
                    "startup_params": {
                        "mode": "consumer",
                        "group_id": "netget-test-group",
                        "topics": ["test-events", "test-logs"],
                        "client_id": "netget-test-consumer"
                    }
                }]))
                .expect_calls(1)
                .and()
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([{
                    "type": "subscribe_topics",
                    "topics": ["test-events", "test-logs"]
                }]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        println!("✅ Kafka consumer client connected and subscribed");

        server.verify_mocks().await?;
        client.verify_mocks().await?;

        client.stop().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Kafka producer-consumer flow: a produced record reaches a consumer.
    /// LLM calls: 6 (server startup + metadata + produce + fetch, two client startups + two
    /// connected events)
    #[tokio::test]
    #[ignore = "Two blockers. (1) The Kafka client is not compiled into any build — \
                src/client/mod.rs gates it on feature `rdkafka`, which no feature enables. \
                (2) The consumer half additionally needs consumer-group APIs \
                (FindCoordinator/JoinGroup/SyncGroup/Heartbeat) that NetGet's broker does not \
                implement. Note also that this broker keeps no storage, so a produced record \
                is not replayed to a fetch: the fetch_response below has to restate the \
                payload, which is why the consumer assertion is the fetch arriving, not the \
                broker relaying."]
    async fn test_kafka_producer_consumer_flow() -> E2EResult<()> {
        let server_config = NetGetConfig::new("Start a Kafka broker on port {AVAILABLE_PORT}.")
            .with_mock(|mock| {
                mock.on_instruction_containing("Kafka broker")
                    .respond_with_actions(serde_json::json!([{
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Kafka",
                        "instruction": "Kafka broker"
                    }]))
                    .expect_calls(1)
                    .and()
                    .on_event("kafka_metadata_request")
                    .respond_with_actions(serde_json::json!([{
                        "type": "metadata_response",
                        "topics": [{"name": "flow-test", "partitions": [{"partition": 0}]}]
                    }]))
                    .expect_at_most(6)
                    .and()
                    // Proves the producer's record crossed the wire.
                    .on_event("kafka_produce_request")
                    .and_event_data_contains("topic", "flow-test")
                    .respond_with_actions(serde_json::json!([{
                        "type": "produce_response",
                        "topic": "flow-test",
                        "partition": 0,
                        "offset": 0,
                        "error_code": 0
                    }]))
                    .expect_calls(1)
                    .and()
                    // Proves the consumer polled. The broker has no storage, so the record is
                    // restated here rather than relayed.
                    .on_event("kafka_fetch_request")
                    .respond_with_actions(serde_json::json!([{
                        "type": "fetch_response",
                        "topic": "flow-test",
                        "partition": 0,
                        "records": [{"offset": 0, "key": "test-key", "value": "Test Flow"}]
                    }]))
                    .expect_at_most(10)
                    .and()
            });

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let remote = format!("127.0.0.1:{}", server.port);

        let consumer_config = NetGetConfig::new(format!(
            "Connect to {remote} via Kafka as consumer. Subscribe to topic 'flow-test'. Log \
             each message received."
        ))
        .with_mock({
            let remote = remote.clone();
            move |mock| {
                mock.on_instruction_containing("Connect to")
                    .and_instruction_containing("consumer")
                    .respond_with_actions(serde_json::json!([{
                        "type": "open_client",
                        "remote_addr": remote,
                        "protocol": "Kafka",
                        "instruction": "Subscribe to flow-test and log messages",
                        "startup_params": {
                            "mode": "consumer",
                            "group_id": "netget-flow-test",
                            "topics": ["flow-test"],
                            "client_id": "netget-flow-consumer"
                        }
                    }]))
                    .expect_calls(1)
                    .and()
                    .on_event("kafka_connected")
                    .respond_with_actions(serde_json::json!([{
                        "type": "subscribe_topics",
                        "topics": ["flow-test"]
                    }]))
                    .expect_calls(1)
                    .and()
                    // Fires only if the record produced by the other client came back through
                    // the broker's fetch_response.
                    .on_event("kafka_message_received")
                    .and_event_data_contains("payload", "Test Flow")
                    .respond_with_actions(serde_json::json!([{ "type": "commit_offset" }]))
                    .expect_calls(1)
                    .and()
            }
        });

        let consumer = start_netget_client(consumer_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        let producer_config = NetGetConfig::new(format!(
            "Connect to {remote} via Kafka as producer. Send message 'Test Flow' to topic \
             'flow-test' with key 'test-key'."
        ))
        .with_mock(move |mock| {
            mock.on_instruction_containing("Connect to")
                .and_instruction_containing("producer")
                .respond_with_actions(serde_json::json!([{
                    "type": "open_client",
                    "remote_addr": remote,
                    "protocol": "Kafka",
                    "instruction": "Send Test Flow message",
                    "startup_params": {
                        "mode": "producer",
                        "client_id": "netget-flow-producer"
                    }
                }]))
                .expect_calls(1)
                .and()
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([{
                    "type": "produce_message",
                    "topic": "flow-test",
                    "key": "test-key",
                    "payload": "Test Flow"
                }]))
                .expect_calls(1)
                .and()
        });

        let producer = start_netget_client(producer_config).await?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        println!("✅ Kafka producer-consumer flow completed");

        server.verify_mocks().await?;
        consumer.verify_mocks().await?;
        producer.verify_mocks().await?;

        producer.stop().await?;
        consumer.stop().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test Kafka protocol detection - and that the detected client actually reaches the broker.
    ///
    /// `client.protocol` on its own asserts nothing: the harness parses it from a log line
    /// NetGet emits before the connection is attempted, so it reads "Kafka" even when the
    /// client immediately fails with "not compiled into this build". The broker-side
    /// `kafka_metadata_request` is what makes this test able to fail.
    ///
    /// LLM calls: 3 (server startup + metadata, client startup)
    #[tokio::test]
    #[ignore = "Kafka client is not compiled into any build: src/client/mod.rs gates it on \
                feature `rdkafka`, which Cargo.toml declares as an optional dependency no \
                feature enables. This test used to pass while the client was completely \
                non-functional, because it only asserted the protocol label the harness parses \
                from NetGet's startup log; it now also requires the broker to see the client."]
    async fn test_kafka_client_protocol_detection() -> E2EResult<()> {
        let server_config = NetGetConfig::new("Start a Kafka broker on port {AVAILABLE_PORT}.")
            .with_mock(|mock| {
                mock.on_instruction_containing("Kafka broker")
                    .respond_with_actions(serde_json::json!([{
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Kafka",
                        "instruction": "Kafka broker"
                    }]))
                    .expect_calls(1)
                    .and()
                    // Any real Kafka client asks for metadata first. If the client never
                    // connected, this never fires.
                    .on_event("kafka_metadata_request")
                    .respond_with_actions(serde_json::json!([{
                        "type": "metadata_response",
                        "topics": [{"name": "detection", "partitions": [{"partition": 0}]}]
                    }]))
                    .expect_at_most(3)
                    .and()
            });

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let remote = format!("127.0.0.1:{}", server.port);
        let client_config = NetGetConfig::new(format!(
            "Connect to {remote} via Kafka as producer."
        ))
        .with_mock(move |mock| {
            mock.on_instruction_containing("Connect to")
                .and_instruction_containing("Kafka")
                .respond_with_actions(serde_json::json!([{
                    "type": "open_client",
                    "remote_addr": remote,
                    "protocol": "Kafka",
                    "instruction": "Kafka producer",
                    "startup_params": {
                        "mode": "producer",
                        "client_id": "netget-protocol-test"
                    }
                }]))
                .expect_calls(1)
                .and()
                .on_event("kafka_connected")
                .respond_with_actions(serde_json::json!([{
                    "type": "produce_message",
                    "topic": "detection",
                    "payload": "detected"
                }]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        assert_eq!(
            client.protocol, "Kafka",
            "Client should be detected as Kafka protocol"
        );

        println!("✅ Kafka client protocol detected and connected");

        server.verify_mocks().await?;
        client.verify_mocks().await?;

        client.stop().await?;
        server.stop().await?;

        Ok(())
    }
}
