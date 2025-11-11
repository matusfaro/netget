# Kafka Client Implementation

## Overview

The Kafka client implementation provides LLM-controlled access to Apache Kafka broker clusters. The LLM can produce
messages to topics (producer mode) or consume messages from topics (consumer mode), with full control over message
routing, consumer groups, and offset management.

## Implementation Details

### Library Choice

- **rdkafka** - Rust wrapper for librdkafka (the official C/C++ Kafka client)
- High-performance, production-ready Kafka protocol implementation
- Supports all Kafka features: producer, consumer, consumer groups, offset management
- Asynchronous message delivery and consumption

### Architecture

```
┌────────────────────────────────────────┐
│  KafkaClient::connect_with_llm_actions │
│  - Parse startup parameters            │
│  - Determine mode (producer/consumer)  │
│  - Create rdkafka client               │
└────────────────────────────────────────┘
         │
         ├─► Producer Mode
         │   ├─ FutureProducer (async)
         │   ├─ Call LLM on connect
         │   ├─ Execute produce_message actions
         │   └─ Call LLM on delivery confirmation
         │
         └─► Consumer Mode
             ├─ StreamConsumer (async streaming)
             ├─ Subscribe to topics
             ├─ Spawn message polling loop
             │   ├─ Poll for messages
             │   ├─ Call LLM with message event
             │   └─ Execute actions (produce, commit, etc.)
             └─ Handle subscribe_topics, commit_offset
```

### Client Modes

#### Producer Mode

- Send messages to Kafka topics
- Async delivery with partition and offset confirmation
- LLM controls: topic, payload, key (for partitioning)

#### Consumer Mode

- Receive messages from subscribed topics
- Consumer group coordination
- Manual offset commit control
- LLM processes each message and decides actions

### LLM Control

**Startup Parameters** (required):

- `mode` - "producer" or "consumer" (required)
- `client_id` - Kafka client identifier (optional, default: "netget-kafka-client")
- `topics` - Array of topics to subscribe to (consumer mode only, optional)
- `group_id` - Consumer group ID (consumer mode only, optional, default: "netget-consumer-group")

**Async Actions** (user-triggered):

- `produce_message` - Produce message to topic (producer mode)
    - Parameters: topic (string), payload (string), key (string, optional)
- `subscribe_topics` - Subscribe to topics (consumer mode)
    - Parameters: topics (array of strings)
- `commit_offset` - Commit current consumer offset (consumer mode)
- `disconnect` - Close Kafka connection

**Sync Actions** (in response to Kafka events):

- `produce_message` - Produce message in response to received message
- `commit_offset` - Commit offset after processing message

**Events:**

- `kafka_connected` - Fired when connection established
    - Data: brokers, client_mode
- `kafka_message_received` - Fired when consumer receives message
    - Data: topic, partition, offset, key, payload, timestamp
- `kafka_message_delivered` - Fired when producer delivers message
    - Data: topic, partition, offset

### Message Format

Messages in Kafka are structured with:

- **Topic** - Logical channel for messages
- **Partition** - Ordered sequence within topic
- **Offset** - Unique ID for message in partition
- **Key** - Optional partitioning key
- **Payload** - Message content (string)
- **Timestamp** - Message timestamp (milliseconds)

### Structured Actions

```json
// Producer: Send message
{
  "type": "produce_message",
  "topic": "user-events",
  "payload": "{\"user_id\": 123, \"action\": \"login\"}",
  "key": "user-123"
}

// Consumer: Subscribe to topics
{
  "type": "subscribe_topics",
  "topics": ["user-events", "system-events"]
}

// Consumer: Commit offset
{
  "type": "commit_offset"
}

// Event: Message received
{
  "event_type": "kafka_message_received",
  "data": {
    "topic": "user-events",
    "partition": 0,
    "offset": 12345,
    "key": "user-123",
    "payload": "{\"user_id\": 123, \"action\": \"login\"}",
    "timestamp": 1704067200000
  }
}

// Event: Message delivered
{
  "event_type": "kafka_message_delivered",
  "data": {
    "topic": "user-events",
    "partition": 0,
    "offset": 12345
  }
}
```

### Dual Logging

```rust
info!("Kafka producer {} connected", client_id);           // → netget.log
status_tx.send("[CLIENT] Kafka producer connected");      // → TUI
```

## Limitations

- **No Transaction Support** - Kafka transactions not implemented
- **No Schema Registry** - No Avro/Protobuf schema validation
- **Single Client Instance** - No connection pooling
- **Manual Offset Commit** - Auto-commit disabled for LLM control
- **Text Payloads Only** - Binary payloads must be base64-encoded
- **No Exactly-Once Semantics** - At-least-once delivery only
- **No Admin Operations** - Cannot create/delete topics, partitions

## Usage Examples

### Producer: Send Message

**User**: "Connect to Kafka at localhost:9092 as producer and send a message to 'events' topic"

**Startup Parameters**:

```json
{
  "mode": "producer",
  "client_id": "netget-producer-1"
}
```

**LLM Action**:

```json
{
  "type": "produce_message",
  "topic": "events",
  "payload": "Hello Kafka",
  "key": "message-1"
}
```

### Consumer: Subscribe and Process

**User**: "Connect to Kafka at localhost:9092 as consumer, subscribe to 'events', and log each message"

**Startup Parameters**:

```json
{
  "mode": "consumer",
  "group_id": "netget-consumers",
  "topics": ["events"],
  "client_id": "netget-consumer-1"
}
```

**LLM receives messages automatically**:

```json
{
  "event_type": "kafka_message_received",
  "data": {
    "topic": "events",
    "partition": 0,
    "offset": 100,
    "payload": "Hello Kafka"
  }
}
```

**LLM Action (after processing)**:

```json
{
  "type": "commit_offset"
}
```

### Consumer: Dynamic Subscription

**User**: "Subscribe to topics 'logs' and 'metrics'"

**LLM Action**:

```json
{
  "type": "subscribe_topics",
  "topics": ["logs", "metrics"]
}
```

### Producer: Stream Processing

**User**: "For each message in 'input' topic, process it and send to 'output' topic"

**Setup**: Open consumer for 'input', producer for 'output'

**Consumer receives**:

```json
{
  "event_type": "kafka_message_received",
  "data": {
    "topic": "input",
    "payload": "{\"value\": 42}"
  }
}
```

**LLM produces to output** (requires separate producer client):

```json
{
  "type": "produce_message",
  "topic": "output",
  "payload": "{\"value\": 84}"
}
```

## Testing Strategy

See `tests/client/kafka/CLAUDE.md` for E2E testing approach.

## Future Enhancements

- **Transaction Support** - Exactly-once semantics
- **Schema Registry Integration** - Avro/Protobuf/JSON Schema
- **Admin API** - Create/delete topics, partitions, ACLs
- **Binary Payloads** - Native binary data handling
- **Compression** - Snappy, LZ4, GZIP compression
- **SSL/SASL Authentication** - Secure cluster connections
- **Partition Assignment Control** - Manual partition assignment
- **Exactly-Once Semantics** - Idempotent producer + transactions
- **Consumer Lag Monitoring** - Offset lag tracking
- **Header Support** - Kafka message headers

## Security Considerations

- **Plaintext by Default** - No encryption or authentication
- **Network Exposure** - Kafka traffic is unencrypted
- **Consumer Groups** - Shared group IDs can cause conflicts
- **Offset Management** - Manual commit prevents data loss but requires care

## Performance Notes

- **Async Producer** - Non-blocking message delivery with 30s timeout
- **Streaming Consumer** - Efficient async message polling
- **Batch Processing** - rdkafka handles internal batching
- **No Connection Pooling** - Each client is a single connection
- **Partition Assignment** - Consumer group rebalancing handled automatically
