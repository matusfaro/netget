# MQTT Protocol E2E Tests

## Test Overview

End-to-end tests for MQTT (Message Queuing Telemetry Transport) broker functionality. These tests verify that the MQTT protocol is properly registered in NetGet's architecture.

**Current Status**: Placeholder tests only. Full MQTT broker implementation is pending.

**Protocols Tested**: MQTT v3.1.1 and v5.0 (planned)

## Test Strategy

**Placeholder Validation**: Since MQTT broker is not yet implemented, current tests verify:
1. Protocol is registered and detectable
2. Proper error messages returned when broker spawn is attempted
3. Keyword detection works for "mqtt", "mosquitto", etc.

**Future Strategy** (post-implementation):
- Use `rumqttc` client library for real MQTT connections
- Test publish/subscribe message routing
- Validate QoS levels (0, 1, 2)
- Test retained messages and wildcards
- Black-box protocol testing

## LLM Call Budget

### Current Tests (Placeholder)

1. **`test_mqtt_placeholder_registered`**: Attempts to start broker, expects error = **1 LLM call**
2. **`test_mqtt_keyword_detection`**: Tests 4 keyword variations = **4 LLM calls**

**Current Total: 5 LLM calls**

### Future Tests (Post-Implementation)

When full MQTT broker is implemented, target **< 10 total LLM calls**:

1. **Basic Connection Test**: 1 server startup + 1 connect = **2 LLM calls**
2. **Publish/Subscribe Test**: 1 server startup + 1 publish + 1 subscribe = **3 LLM calls**
3. **QoS Levels Test**: 1 server startup + 3 publishes (QoS 0/1/2) = **4 LLM calls**

**Optimization Strategy**:
- Consolidate tests: Single comprehensive server with multiple operations
- Use scripting mode: Topic routing is deterministic, ideal for scripts
- With scripting: 1 server startup (generates script) + 0 LLM calls per message = **1-2 total calls**

## Scripting Usage

**Not Yet Applicable**: MQTT broker not implemented.

**Future Potential**: **Excellent candidate for scripting**
- Topic routing rules are deterministic
- Pub/sub matching is algorithmic (wildcards +, #)
- LLM defines routing policy once during server startup
- Script handles all message routing without LLM calls
- Example: "Route messages from 'devices/+/temp' to subscribers of 'devices/#'"

**Expected Performance with Scripting**:
- Server startup: 1 LLM call (generates routing script)
- All publish/subscribe operations: 0 LLM calls (handled by script)
- Throughput: Thousands of messages per second (CPU-bound, not LLM-bound)

## Client Library

**rumqttc v0.24** - Pure Rust MQTT client
- Same ecosystem as rumqttd broker
- Async/sync APIs with tokio
- Supports MQTT v3.1.1 and v5.0
- Full feature set: QoS 0/1/2, retained messages, wildcards

**Usage Example** (for future tests):
```rust
use rumqttc::{AsyncClient, MqttOptions, QoS};

let mut mqttoptions = MqttOptions::new("client_id", "127.0.0.1", port);
mqttoptions.set_keep_alive(Duration::from_secs(5));
let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

// Publish
client.publish("topic", QoS::AtMostOnce, false, b"payload").await?;

// Subscribe
client.subscribe("topic/#", QoS::AtMostOnce).await?;

// Event loop
while let Ok(notification) = eventloop.poll().await {
    println!("Event: {:?}", notification);
}
```

## Expected Runtime

**Current Tests** (placeholder): ~15-20 seconds
- `test_mqtt_placeholder_registered`: ~3-5 seconds (1 LLM call)
- `test_mqtt_keyword_detection`: ~12-15 seconds (4 LLM calls)

**Future Tests** (post-implementation, without scripting): ~30-40 seconds
- With scripting: ~5-10 seconds (1-2 LLM calls total)

**Model**: qwen3-coder:30b (default)
**LLM latency**: ~2-5 seconds per call
**MQTT message latency**: <1ms (very fast binary protocol)

## Failure Rate

**Current Tests**: **N/A** (placeholder tests, not yet validating MQTT protocol)

**Expected Future Failure Rate**: **Low** (~2-5%)
- MQTT is simpler than HTTP/SSH
- Binary protocol reduces ambiguity
- Pub/sub model is straightforward
- Topic matching is deterministic

**Potential Failure Modes** (future):
1. LLM incorrectly routes messages to subscribers
2. Wildcard matching errors (+ vs # confusion)
3. QoS handshake issues (PUBACK, PUBREC timing)
4. Retained message not delivered to late subscribers

## Test Cases Covered

### Current Tests (Placeholder)

1. **Protocol Registration** (`test_mqtt_placeholder_registered`)
   - Attempts to start MQTT broker
   - Verifies error message mentions "not yet implemented" or "placeholder"
   - Confirms protocol is registered in NetGet architecture

2. **Keyword Detection** (`test_mqtt_keyword_detection`)
   - Tests various MQTT-related prompts:
     - "Start an MQTT broker on port 1883"
     - "Create a mosquitto server for IoT devices"
     - "Listen via MQTT on port 0"
     - "Set up message queue telemetry transport on port 1883"
   - Verifies MQTT protocol is detected (not "unknown protocol")

### Future Tests (Commented Out, Awaiting Implementation)

3. **Basic Connection** (`test_mqtt_basic_connect` - ignored)
   - Connect rumqttc client to broker
   - Validate CONNACK received
   - Tests client registration

4. **Publish/Subscribe** (`test_mqtt_publish_subscribe` - ignored)
   - Publisher sends message to "test/topic"
   - Subscriber receives message via "test/#" wildcard
   - Tests message routing

5. **QoS Levels** (`test_mqtt_qos_levels` - ignored)
   - Publish with QoS 0 (at most once)
   - Publish with QoS 1 (at least once, requires PUBACK)
   - Publish with QoS 2 (exactly once, requires PUBREC/PUBREL/PUBCOMP)
   - Tests QoS handshakes

6. **Retained Messages** (`test_mqtt_retained_messages` - ignored)
   - Publish retained message to topic
   - Late subscriber connects
   - Validates retained message delivered immediately
   - Tests retained message storage

7. **Wildcard Subscriptions** (`test_mqtt_wildcard_subscriptions` - ignored)
   - Subscribe to "devices/+/temp" (single-level wildcard)
   - Publish to "devices/sensor1/temp" (should match)
   - Publish to "devices/sensor2/temp" (should match)
   - Publish to "devices/sensor1/humidity" (should NOT match)
   - Tests wildcard matching logic

### Coverage Gaps (Future)

**Not Yet Tested**:
- Multi-level wildcard (#) subscriptions
- Last will and testament messages
- Clean session vs persistent session
- Client reconnection with session resume
- Topic ACL (access control lists)
- Maximum message size handling
- Connection keep-alive and timeout
- TLS/SSL encrypted connections (port 8883)
- WebSocket transport (ws://, wss://)
- MQTT v5.0 specific features:
  - User properties
  - Topic aliases
  - Request/response pattern
  - Shared subscriptions
  - Subscription identifiers

## Test Infrastructure

### Helper Functions (Planned)

```rust
/// Build MQTT CONNECT packet manually (if needed for low-level tests)
fn build_mqtt_connect_packet(client_id: &str) -> Vec<u8>;

/// Build MQTT PUBLISH packet with topic and payload
fn build_mqtt_publish_packet(topic: &str, payload: &[u8], qos: u8) -> Vec<u8>;

/// Parse MQTT CONNACK response
fn parse_mqtt_connack(data: &[u8]) -> Result<u8>; // Returns return code
```

Most tests will use `rumqttc` library rather than manual packet construction.

### Test Execution Pattern (Future)

```rust
// 1. Start MQTT broker
let config = ServerConfig::new(
    "Start an MQTT broker on port 0. Accept all connections."
)
.with_log_level("debug");
let test_state = start_netget_server(config).await?;

// 2. Wait for server ready
tokio::time::sleep(Duration::from_millis(500)).await;

// 3. Create MQTT client
let mut mqttoptions = MqttOptions::new("test_client", "127.0.0.1", test_state.port);
mqttoptions.set_keep_alive(Duration::from_secs(5));
let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

// 4. Spawn event loop
tokio::spawn(async move {
    while let Ok(event) = eventloop.poll().await {
        // Handle events
    }
});

// 5. Perform MQTT operations
client.subscribe("test/#", QoS::AtMostOnce).await?;
client.publish("test/hello", QoS::AtMostOnce, false, b"world").await?;

// 6. Validate results
// ...

// 7. Cleanup
test_state.stop().await?;
```

## Known Issues

### Test Harness Limitations

**Issue**: `start_netget_server()` expects server to output port information
**Impact**: Placeholder implementation doesn't start, returns generic error
**Mitigation**: Tests check for error message content rather than specific format

### Build Requirements

**Critical**: Must build NetGet with MQTT feature before running tests:
```bash
./cargo-isolated.sh build --release --features mqtt
```

Without this, E2E tests will fail with "Server did not output startup information" because the binary doesn't include MQTT support.

## Running Tests

```bash
# Build NetGet with MQTT support first (REQUIRED)
./cargo-isolated.sh build --release --all-features

# Run current placeholder tests
./cargo-isolated.sh test --features mqtt --test server test_mqtt_placeholder

# Run all MQTT tests when implemented
./cargo-isolated.sh test --features mqtt --test server mqtt

# Run specific test
./cargo-isolated.sh test --features mqtt --test server test_mqtt_basic_connect

# Run with output
./cargo-isolated.sh test --features mqtt --test server mqtt -- --nocapture

# Run ignored tests (future, when MQTT is implemented)
./cargo-isolated.sh test --features mqtt --test server mqtt -- --ignored
```

## Future Test Additions

1. **Authentication Tests**: Username/password validation
2. **Authorization Tests**: Topic-level ACL enforcement
3. **Performance Tests**: Measure messages/sec with and without scripting
4. **Stress Tests**: 100+ concurrent clients publishing rapidly
5. **Error Handling**: Invalid packets, malformed topics, oversized payloads
6. **Persistence Tests**: Session persistence across broker restart
7. **Bridge Tests**: Connect two brokers (if bridge feature added)
8. **WebSocket Tests**: Connect via ws:// transport

## Migration Path

**Current State**: Tests verify protocol registration only.

**Phase 1** (Basic Broker): Enable `test_mqtt_basic_connect`, validate CONNACK

**Phase 2** (Pub/Sub): Enable `test_mqtt_publish_subscribe`, validate message routing

**Phase 3** (QoS): Enable `test_mqtt_qos_levels`, validate handshakes

**Phase 4** (Advanced): Enable retained messages, wildcards, authentication tests

**Phase 5** (Optimization): Add scripting mode, measure performance improvement

Each phase: Remove `#[ignore]` attribute, run tests, fix issues, iterate.
