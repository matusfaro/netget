# Kafka Protocol Implementation

## Overview
Apache Kafka broker implementing core message broker functionality. The LLM can control message routing, topic management, and consumer coordination through structured actions.

**Status**: Alpha
**Specification**: Apache Kafka Wire Protocol
**Port**: 9092 (TCP)
**Version Support**: Kafka 4.1.0 protocol (via kafka-protocol crate)

## Library Choices
- **kafka-protocol** (v0.13) - Code-generated Kafka protocol parsing and serialization
  - Parses all Kafka API request/response messages
  - Handles protocol versioning automatically
  - Covers entire Kafka API surface (produce, fetch, metadata, offsets, etc.)
  - Generated from official Kafka schema definitions

**Rationale**: There is no mature Kafka server implementation in Rust. The kafka-protocol crate provides wire format handling, allowing us to focus on broker logic and LLM integration. Manual server implementation is required, but the library eliminates binary protocol complexity.

## Architecture Decisions

### 1. Manual Server Implementation
No existing Rust Kafka server library exists, so we implement broker logic from scratch:
- Topic storage: HashMap<topic_name, Vec<partition_records>>
- In-memory storage (no disk persistence for MVP)
- Single-node broker (no replication)
- Simplified state management

### 2. Action-Based LLM Control
The LLM controls broker behavior through semantic actions:
- **Sync Actions** (respond to network events): produce_response, fetch_response, metadata_response, offset_commit_response, error_response
- **Async Actions** (user-triggered): publish_message, create_topic, delete_topic, set_retention

### 3. Core API Support (MVP Scope)
Implemented Kafka APIs:
- **ApiVersions** - Client capability negotiation (no LLM, auto-response)
- **Metadata** - Topic/partition/broker discovery (LLM-controlled)
- **Produce** - Accept records from producers (LLM-controlled)
- **Fetch** - Serve records to consumers (LLM-controlled)
- **OffsetCommit** - Track consumer positions (LLM-controlled)

Not implemented (future):
- Consumer group coordination (JoinGroup, SyncGroup, Heartbeat, LeaveGroup)
- Transactions (InitProducerId, AddPartitionsToTxn, EndTxn, etc.)
- Admin APIs (CreateTopics, DeleteTopics, AlterConfigs, etc.)
- Log compaction, retention enforcement
- Inter-broker replication

### 4. Connection Management
- TCP-based, connection-oriented protocol
- Each connection tracked in ServerInstance
- ProtocolConnectionInfo::Kafka with recent_requests list
- Multiple concurrent client connections supported
- No persistent session state (stateless request-response per API call)

### 5. Wire Protocol Handling
Kafka wire protocol structure:
```
[4 bytes: message_size] [message_bytes]
message_bytes = [request_header] [request_body]
```

Flow:
1. Read 4-byte size prefix (big-endian i32)
2. Read message_size bytes
3. Parse RequestHeader (API key, version, correlation ID, client ID)
4. Dispatch based on API key
5. Call LLM for decision (for Produce/Fetch/Metadata/OffsetCommit)
6. Execute LLM action
7. Build response message with ResponseHeader
8. Send [4 bytes: response_size] [response_bytes]

### 6. Dual Logging
- **TRACE**: Full hex dumps of Kafka wire protocol messages (request + response)
- **DEBUG**: Request summaries ("Kafka request: API=Produce, correlation_id=123")
- **INFO**: High-level events (client connected, topic created, message produced)
- **ERROR**: Protocol errors, unsupported APIs, LLM failures
- All logs go to both netget.log and TUI Status panel

## LLM Integration

### Event Types

#### `kafka_produce_request`
Triggered when producer sends records to topic.

Event parameters:
- `topic` (string) - Topic name
- `partition` (number) - Partition number
- `record_count` (number) - Number of records in batch
- `first_key` (string, optional) - Key of first record
- `first_value_preview` (string) - Preview of first record value

#### `kafka_fetch_request`
Triggered when consumer requests records from topic.

Event parameters:
- `topic` (string) - Topic name
- `partition` (number) - Partition number
- `fetch_offset` (number) - Offset to fetch from
- `max_bytes` (number) - Maximum bytes to return

#### `kafka_metadata_request`
Triggered when client requests cluster/topic metadata.

Event parameters:
- `requested_topics` (array of strings) - Topics client wants metadata for (empty = all topics)

#### `kafka_offset_commit_request`
Triggered when consumer commits offsets.

Event parameters:
- `group_id` (string) - Consumer group ID
- `topic` (string) - Topic name
- `partition` (number) - Partition number
- `offset` (number) - Committed offset

### Sync Actions

#### `produce_response`
Respond to produce request with assigned offset.

Parameters:
- `topic` (required) - Topic name
- `partition` (required) - Partition number
- `offset` (required) - Assigned offset for the record
- `error_code` (optional, default 0) - Kafka error code (0 = success)

Example:
```json
{
  "type": "produce_response",
  "topic": "orders",
  "partition": 0,
  "offset": 42,
  "error_code": 0
}
```

#### `fetch_response`
Respond to fetch request with records.

Parameters:
- `topic` (required) - Topic name
- `partition` (required) - Partition number
- `records` (required) - Array of records [{offset, key, value}]

Example:
```json
{
  "type": "fetch_response",
  "topic": "orders",
  "partition": 0,
  "records": [
    {"offset": 40, "key": "order123", "value": "{\"item\": \"laptop\"}"},
    {"offset": 41, "key": "order124", "value": "{\"item\": \"mouse\"}"}
  ]
}
```

#### `metadata_response`
Respond with cluster and topic metadata.

Parameters:
- `brokers` (required) - Array of broker info [{id, host, port}]
- `topics` (required) - Array of topics [{name, partitions: [{partition, leader, replicas}]}]

Example:
```json
{
  "type": "metadata_response",
  "brokers": [{"id": 0, "host": "localhost", "port": 9092}],
  "topics": [
    {
      "name": "orders",
      "partitions": [{"partition": 0, "leader": 0, "replicas": [0]}]
    }
  ]
}
```

#### `offset_commit_response`
Acknowledge offset commit.

Parameters:
- `topic` (required) - Topic name
- `partition` (required) - Partition number
- `error_code` (optional, default 0) - Kafka error code (0 = success)

#### `error_response`
Respond with error.

Parameters:
- `error_code` (required) - Kafka error code (3 = Unknown topic, 6 = Invalid partition, etc.)
- `error_message` (optional) - Human-readable error

### Async Actions

#### `publish_message`
LLM publishes message to topic.

Parameters:
- `topic` (required) - Topic name
- `key` (optional) - Message key
- `value` (required) - Message value
- `partition` (optional) - Target partition

#### `create_topic`
Create new topic.

Parameters:
- `topic` (required) - Topic name
- `partitions` (optional, default 1) - Partition count
- `replication_factor` (optional, default 1) - Replication factor

#### `delete_topic`
Delete topic.

Parameters:
- `topic` (required) - Topic name

#### `set_retention`
Set retention policy.

Parameters:
- `topic` (required) - Topic name
- `retention_hours` (required) - Retention time in hours

## Startup Parameters

Configurable via `open_server` action:
- `cluster_id` (string, default: "netget-kafka-1") - Cluster identifier
- `broker_id` (number, default: 0) - Broker ID
- `auto_create_topics` (boolean, default: true) - Auto-create topics on first produce
- `default_partitions` (number, default: 1) - Partition count for auto-created topics
- `log_retention_hours` (number, default: 168) - Log retention time

Example:
```json
{
  "type": "open_server",
  "stack": "kafka",
  "port": 9092,
  "params": {
    "cluster_id": "netget-kafka-1",
    "broker_id": 0,
    "auto_create_topics": true,
    "default_partitions": 3,
    "log_retention_hours": 72
  }
}
```

## Known Limitations

### 1. No Replication
- Single-node broker only
- No inter-broker communication
- Replication factor always 1

### 2. In-Memory Storage
- All messages stored in memory
- No WAL (write-ahead log)
- No segment files
- Data lost on server restart

### 3. No Consumer Groups
- No group coordinator
- No partition rebalancing
- Offset commits tracked but not enforced

### 4. No Transactions
- No exactly-once semantics
- No transactional producers
- No isolation levels

### 5. Limited API Support
- Core APIs only (ApiVersions, Metadata, Produce, Fetch, OffsetCommit)
- Missing: Admin APIs, Coordinator APIs, Transaction APIs

### 6. No Authentication/Authorization
- No SASL/SCRAM
- No ACLs
- Open to all clients

### 7. Simplified Record Format
- Basic record structure (offset, key, value, timestamp)
- No headers
- No compression
- No batch CRC validation

## Example Prompts

### Basic Kafka Broker
```
listen on port 9092 via kafka
Create a topic called 'orders' with 1 partition.
Accept all produce requests and assign sequential offsets.
When consumers fetch, return the last 10 messages.
```

### Smart Message Routing
```
start a kafka broker on port 9092
Auto-create topics as needed.
For messages to 'transactions' topic containing "fraud", also publish to 'alerts' topic.
Track consumer offsets for group 'analytics'.
```

### Testing/Honeypot Broker
```
listen on port 9092 as kafka broker
Log all produce requests with full message content.
Accept all messages but don't store them.
Track which topics clients try to use.
```

### Multi-Topic Broker
```
run kafka broker on port 9092
Create topics: 'events', 'metrics', 'logs'
Store last 1000 messages per topic in memory.
Count messages per topic and show stats every 100 messages.
```

## Performance Characteristics

### Latency
- **With Scripting**: Sub-100ms per request (script handles protocol)
- **Without Scripting**: 2-5 seconds per request (one LLM call)
- kafka-protocol parsing: ~100-500 microseconds per message
- kafka-protocol serialization: ~100-500 microseconds per message

### Throughput
- **With Scripting**: Hundreds of requests/sec (CPU-bound)
- **Without Scripting**: Limited by LLM (~0.2-0.5 requests/sec)
- Concurrent connections processed in parallel
- Ollama lock serializes LLM calls

### Scripting Compatibility
Kafka protocol has moderate scripting potential:
- Repetitive produce/fetch patterns are scriptable
- Metadata responses can be cached
- Complex routing logic better suited for LLM

When scripting enabled:
- Server startup generates script (1 LLM call)
- Simple operations handled by script (0 LLM calls)
- Complex decisions still use LLM

## References
- [Apache Kafka Protocol Guide](https://kafka.apache.org/protocol)
- [Kafka Wire Protocol Documentation](https://kafka.apache.org/24/protocol.html)
- [kafka-protocol Rust Crate](https://docs.rs/kafka-protocol/)
- [Kafka Error Codes](https://kafka.apache.org/protocol.html#protocol_error_codes)
