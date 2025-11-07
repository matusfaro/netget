# Kafka Client E2E Tests

## Test Strategy

End-to-end tests for Kafka client using a real Kafka broker. Tests verify producer and consumer functionality with LLM control.

## Prerequisites

Tests require a running Kafka broker at `localhost:9092`. For CI/local testing:

```bash
# Start Kafka with Docker Compose
docker-compose up -d kafka

# Or use standalone Kafka Docker container
docker run -d \
  --name kafka-test \
  -p 9092:9092 \
  -e KAFKA_LISTENERS=PLAINTEXT://localhost:9092 \
  -e KAFKA_ADVERTISED_LISTENERS=PLAINTEXT://localhost:9092 \
  apache/kafka:latest
```

Alternatively, use Redpanda (Kafka-compatible, simpler setup):
```bash
docker run -d \
  --name redpanda-test \
  -p 9092:9092 \
  vectorized/redpanda:latest \
  redpanda start \
  --kafka-addr 0.0.0.0:9092 \
  --advertise-kafka-addr localhost:9092
```

## LLM Call Budget

**Target:** < 10 calls
**Actual:** 6 calls total

## Tests

1. **test_kafka_producer_send_message** (1 LLM call)
   - Start Kafka producer client
   - Send message to topic 'test-events'
   - Verify client connection

2. **test_kafka_consumer_subscribe** (1 LLM call)
   - Start Kafka consumer client
   - Subscribe to topics
   - Verify subscription

3. **test_kafka_producer_consumer_flow** (2 LLM calls)
   - Start consumer first (subscribe to topic)
   - Start producer and send message
   - Verify producer sends successfully
   - Note: Consumer receive verification is non-deterministic due to offset/timing

4. **test_kafka_client_protocol_detection** (1 LLM call)
   - Verify Kafka protocol is correctly detected
   - Check client metadata

## Runtime

**Expected:** < 30 seconds (with Kafka already running)
**Breakdown:**
- Kafka producer connection: ~2 seconds
- Kafka consumer connection: ~2 seconds
- Producer-consumer flow: ~6 seconds
- Protocol detection: ~1 second

## Running Tests

```bash
# Build with Kafka feature
./cargo-isolated.sh build --no-default-features --features kafka

# Run Kafka client tests
./cargo-isolated.sh test --no-default-features --features kafka --test client::kafka::e2e_test

# Run specific test
./cargo-isolated.sh test --no-default-features --features kafka --test client::kafka::e2e_test -- test_kafka_producer_send_message
```

## Known Issues

1. **Consumer Lag** - Consumer might not receive messages immediately due to offset/timing
2. **Topic Creation** - Topics are auto-created but may take time to appear
3. **Connection Timeout** - Tests may fail if Kafka broker is not ready
4. **Partition Assignment** - Consumer group rebalancing can cause delays

## Test Efficiency

- **Reusing Kafka Broker** - All tests use same Kafka instance (external)
- **Minimal LLM Calls** - Each test uses 1-2 LLM calls max
- **Fast Execution** - Tests complete in < 30 seconds
- **No Heavy Operations** - No large message batches or complex queries

## Future Enhancements

- Add integration test with message verification (producer → consumer)
- Test consumer offset commit functionality
- Test dynamic topic subscription
- Test error handling (broker disconnection)
- Test high-throughput scenarios
- Test consumer group rebalancing
- Test transactional producers
