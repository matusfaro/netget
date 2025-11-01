# MQTT Protocol Implementation

## Overview
MQTT (Message Queuing Telemetry Transport) broker for IoT messaging. This is a **placeholder implementation** that registers the protocol but does not yet provide full broker functionality.

**Status**: Alpha (Application Protocol - Placeholder)
**RFC**: None (MQTT is documented in OASIS standards, not IETF RFCs)
**OASIS Standards**: MQTT v3.1.1 (ISO/IEC 20922), MQTT v5.0
**Port**: 1883 (default), 8883 (TLS)

## Current Implementation Status

### ✅ Completed
- Protocol registration in NetGet architecture
- Feature flag (`mqtt`) in Cargo.toml
- Module structure (`src/server/mqtt/`)
- Protocol trait implementation (`MqttProtocol`)
- Connection info enum variant (`ProtocolConnectionInfo::Mqtt`)
- Keyword-based protocol detection (`mqtt`, `mosquitto`, `message queue`)
- Metadata with Alpha status and explanatory notes

### ❌ Not Yet Implemented
- rumqttd broker integration
- MQTT packet parsing and handling
- Client connection management
- PUBLISH/SUBSCRIBE message routing
- Topic filtering and wildcards
- QoS levels (0, 1, 2)
- Retained messages
- LLM control points for authorization and message handling
- MQTT v5.0 features

## Planned Architecture

### Library Choices (Planned)
**rumqttd v0.20** - MQTT broker implementation
- Embeddable MQTT broker written in Rust
- Supports MQTT v3.1.1 (stable) and v5.0 (in progress)
- Provides `Broker` API and `Link` API for event subscription
- QoS 0, 1, and 2 support
- TLS, retained messages, last will
- Feature-rich configuration via `rumqttd::Config`

**rumqttc v0.24** - MQTT client (for E2E testing)
- Same ecosystem as rumqttd
- Async/sync APIs
- MQTT v3.1.1 and v5.0 support

**Rationale**: rumqttd is production-ready and embeddable, making it ideal for LLM-controlled broker. Alternative would be manual implementation using `mqtt-protocol` crate for parsing, but rumqttd provides complete broker logic.

### LLM Control Points (Planned)

The LLM will control broker behavior through actions:

**Startup Parameters** (`open_server` action):
- `port` - MQTT broker port (default: 1883)
- `max_clients` - Maximum concurrent clients (default: 100)
- `max_qos` - Maximum QoS level allowed (0, 1, or 2, default: 2)
- `allow_anonymous` - Allow clients without authentication (default: true)
- `enable_tls` - Enable TLS on port 8883

**Event Types** (planned):
- `mqtt_connect` - Client attempts to connect (includes client_id, username, clean_session flag)
- `mqtt_publish` - Client publishes to topic (includes client_id, topic, payload, qos, retain)
- `mqtt_subscribe` - Client subscribes to topic filter (includes client_id, topic, max_qos)
- `mqtt_disconnect` - Client disconnects gracefully

**Sync Actions** (network event triggered):
- `accept_connection` - Accept/reject CONNECT packet with reason code
- `authorize_publish` - Allow/deny PUBLISH to specific topic
- `authorize_subscribe` - Allow/deny SUBSCRIBE to topic filter
- `publish_message` - Broker-initiated publish to topic
- `set_retained_message` - Set/update retained message for topic
- `disconnect_client` - Forcibly disconnect client with reason code

**Async Actions** (user-triggered):
- `publish_to_topic` - Manually publish message from UI/user input
- `update_auth_policy` - Change authentication requirements at runtime
- `list_clients` - Get list of connected clients and their subscriptions

### Logging Strategy (Planned)

**ERROR**:
- Failed to start broker
- Critical broker crashes
- TLS certificate errors
- Fatal configuration errors

**WARN**:
- Client authentication failures
- Client connection rejected (quota/policy)
- Invalid MQTT packets
- Subscription to unauthorized topics
- QoS downgrade enforcement

**INFO**:
- Broker started/stopped
- Client connected/disconnected (client_id, IP)
- Client subscribed/unsubscribed to topics
- Retained message count changes

**DEBUG**:
- Connection details (client_id, username, clean_session flag)
- Publish summaries (client_id → topic, QoS, payload size)
- Subscribe summaries (client_id → topic filter, max QoS)
- Authorization decisions (allow/deny with reason)
- Message delivery confirmations

**TRACE**:
- Full MQTT packet dumps (CONNECT, CONNACK, PUBLISH, SUBSCRIBE, etc. in hex)
- Pretty-printed JSON payloads (if payload is JSON)
- Wildcard topic matching traces
- QoS handshake details (PUBACK, PUBREC, PUBREL, PUBCOMP)
- Retained message lookup traces

**Dual Logging Pattern**: All logs use both `debug!()/trace!()/etc.` macros AND `status_tx.send()` for TUI visibility.

## Known Limitations

### Current Limitations (Placeholder)
- **No broker functionality** - Protocol registered but returns error when spawned
- **No LLM integration** - Actions defined but not executable
- **No E2E tests** - Test infrastructure not yet created

### Future Limitations (Post-Implementation)
- **MQTT v5.0 incomplete** - rumqttd v5 support still in progress, v3.1.1 stable
- **No clustering** - Single-node broker only
- **No persistence** - Messages and subscriptions not persisted across restarts (unless rumqttd config provides this)
- **No bridge/federation** - Cannot bridge to other MQTT brokers
- **No authentication plugins** - Only LLM-controlled auth, no LDAP/database integration

## Example Prompts (Planned)

### Basic MQTT Broker
```
Listen on port 1883 via MQTT. Accept all client connections. Allow publishing and subscribing to any topic. Use QoS 0 for all messages.
```

### Authenticated Broker with Authorization
```
Start an MQTT broker on port 1883. Require authentication - accept username "sensor" with password "secret123". Allow this client to publish to "devices/+" topics and subscribe to "commands/+" topics. Reject all other clients or unauthorized topic access.
```

### IoT Temperature Monitoring
```
Create an MQTT broker on port 1883. Accept clients with client_id starting with "sensor_". Allow them to publish temperature readings to "home/room/temp" topics. Any client can subscribe to "home/#" to monitor all readings. When temperature exceeds 30°C, publish an alert to "alerts/high_temp" topic.
```

### Multi-Client Pub/Sub Test
```
Start MQTT broker on port 1883. Accept all connections. Allow any client to publish to "test/+" topics and subscribe to "test/#" wildcard. Support QoS 0, 1, and 2. Enable retained messages so late subscribers get last value.
```

## Implementation Roadmap

### Phase 1: Basic Broker (Priority)
- [ ] Integrate rumqttd `Broker::new()` and `Broker::start()`
- [ ] Subscribe to broker events via `broker.link()`
- [ ] Parse CONNECT packets and create `mqtt_connect` events
- [ ] Implement `accept_connection` action
- [ ] Track connected clients in `ServerInstance`
- [ ] Implement basic logging (INFO level for lifecycle)

### Phase 2: Pub/Sub Core
- [ ] Parse PUBLISH packets and create `mqtt_publish` events
- [ ] Implement `authorize_publish` action
- [ ] Implement `publish_message` action for broker-initiated publishes
- [ ] Parse SUBSCRIBE packets and create `mqtt_subscribe` events
- [ ] Implement `authorize_subscribe` action
- [ ] Track client subscriptions in connection info

### Phase 3: QoS and Retained Messages
- [ ] Implement QoS 1 and 2 handshakes
- [ ] Implement retained message support
- [ ] Add `set_retained_message` action
- [ ] Test with QoS-aware MQTT clients

### Phase 4: Advanced Features
- [ ] TLS support (port 8883)
- [ ] Last will and testament
- [ ] Session persistence (clean_session=false)
- [ ] Scripting mode for deterministic routing
- [ ] WebSocket transport (ws:// and wss://)

### Phase 5: Testing and Documentation
- [ ] Create E2E tests with rumqttc client
- [ ] Test authorization scenarios
- [ ] Test QoS levels 0, 1, 2
- [ ] Test retained messages and wildcards
- [ ] Create `tests/server/mqtt/CLAUDE.md` documentation
- [ ] Target < 10 LLM calls for test suite

## References
- [MQTT v3.1.1 Specification (OASIS)](https://docs.oasis-open.org/mqtt/mqtt/v3.1.1/os/mqtt-v3.1.1-os.html)
- [MQTT v5.0 Specification (OASIS)](https://docs.oasis-open.org/mqtt/mqtt/v5.0/os/mqtt-v5.0-os.html)
- [rumqttd Documentation](https://docs.rs/rumqttd/latest/rumqttd/)
- [rumqttc Documentation](https://docs.rs/rumqttc/latest/rumqttc/)
- [MQTT Essentials (HiveMQ)](https://www.hivemq.com/mqtt-essentials/)
- [mosquitto (Reference MQTT Broker)](https://mosquitto.org/)

## Notes for Future Implementer

1. **rumqttd Link API** - Use `broker.link(client_id)` to subscribe to all broker events. Events arrive as `Link` enum variants (Connect, Publish, Subscribe, etc.).

2. **Client Tracking** - Each MQTT client should have a `ProtocolConnectionInfo::Mqtt` entry with `client_id` and `subscriptions` list. Update subscriptions when client subscribes/unsubscribes.

3. **Topic Wildcards** - MQTT supports `+` (single level) and `#` (multi-level) wildcards. LLM should understand these for authorization decisions.

4. **QoS Semantics**:
   - QoS 0: At most once (fire and forget)
   - QoS 1: At least once (requires PUBACK)
   - QoS 2: Exactly once (requires PUBREC/PUBREL/PUBCOMP handshake)
   - rumqttd handles QoS handshakes automatically; LLM just authorizes publish/subscribe

5. **Scripting Potential** - MQTT is excellent for scripting mode. Topic routing rules are deterministic. Server startup can generate Python/JavaScript script to handle all routing without LLM calls.

6. **Payload Encoding** - MQTT payloads are binary. LLM should receive base64-encoded payload if binary, UTF-8 string if text. Consider automatic JSON pretty-printing for DEBUG/TRACE logs.

7. **Performance** - MQTT is designed for high-throughput IoT scenarios. Scripting mode is essential for production use. Without scripting, each publish/subscribe triggers LLM call (2-5s latency).
