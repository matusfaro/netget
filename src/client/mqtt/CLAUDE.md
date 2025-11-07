# MQTT Client Implementation

## Overview

The MQTT client implementation provides LLM-controlled MQTT pub/sub messaging capabilities using the `rumqttc` async library. This allows NetGet to connect to MQTT brokers and perform publish/subscribe operations under LLM control.

## Library Choice

**Primary**: `rumqttc` v0.24 (async MQTT client)

### Why rumqttc?

- **Async-first**: Built on tokio, perfect for our async architecture
- **Full MQTT 3.1.1 support**: Complete protocol implementation
- **QoS levels**: Supports QoS 0 (AtMostOnce), QoS 1 (AtLeastOnce), QoS 2 (ExactlyOnce)
- **Mature**: Well-tested, actively maintained
- **Clean API**: Simple EventLoop pattern for handling broker events
- **Built-in reconnection**: Handles connection drops gracefully

### Alternatives Considered

- **paho-mqtt**: C bindings, less idiomatic Rust
- **mqtt-async-client**: Less mature, smaller community
- **Custom implementation**: Too complex for the value gained

## Architecture

### Connection Model

```
┌─────────────┐
│ MqttClient  │
│   (NetGet)  │
└──────┬──────┘
       │ rumqttc::AsyncClient
       │ rumqttc::EventLoop
       │
       ▼
┌─────────────┐
│ MQTT Broker │
│ (Mosquitto, │
│  HiveMQ,    │
│  etc.)      │
└─────────────┘
```

### Event Loop Pattern

rumqttc uses an EventLoop that yields events from the broker:

1. **Connection**: Create `MqttOptions` with broker address, client ID, credentials
2. **AsyncClient**: Used to send actions (subscribe, publish, disconnect)
3. **EventLoop**: Polls for incoming events (ConnAck, Publish, SubAck, etc.)
4. **LLM Integration**: On each event, call LLM to decide actions

### State Machine

We use the same client state machine as other protocols:

- **Idle**: Waiting for events
- **Processing**: LLM is processing an event
- **Accumulating**: New events arrived while processing (queue for next cycle)

This prevents concurrent LLM calls on the same client.

## LLM Integration

### Events Sent to LLM

1. **mqtt_connected**: Fired when connection is established
   - Allows LLM to subscribe to initial topics
   - Parameters: `remote_addr`, `client_id`

2. **mqtt_message_received**: Fired when a message is published to a subscribed topic
   - Allows LLM to process message and potentially publish responses
   - Parameters: `topic`, `payload`, `qos`, `retain`

3. **mqtt_subscribed**: Fired when subscription is confirmed (optional, not currently used)
   - Could be used for complex subscription workflows
   - Parameters: `topics`

### Actions Available to LLM

**Async Actions** (user-initiated):
- `subscribe`: Subscribe to topic patterns (supports `+` and `#` wildcards)
- `publish`: Publish message to a topic with QoS and retain flag
- `unsubscribe`: Remove subscriptions
- `disconnect`: Close connection to broker

**Sync Actions** (in response to events):
- `publish`: Send response message based on received data
- `subscribe`: Dynamically subscribe to new topics

### Example LLM Flow

```
User: "Connect to MQTT broker and monitor temperature sensors"

1. Client connects → mqtt_connected event
2. LLM decides to subscribe("sensors/temperature/#", qos=1)
3. Message arrives on sensors/temperature/room1 → mqtt_message_received
4. LLM analyzes temperature, decides to publish("alerts/high_temp", "Room 1: 30°C")
5. Continue monitoring...
```

## Startup Parameters

The MQTT client supports the following startup parameters:

- **client_id**: MQTT client identifier (default: auto-generated `netget-{client_id}`)
- **username**: Optional authentication username
- **password**: Optional authentication password
- **keep_alive**: Keep-alive interval in seconds (default: 60)
- **clean_session**: Start with clean session (default: true)

Example:
```json
{
  "client_id": "netget-sensor-monitor",
  "username": "admin",
  "password": "secret",
  "keep_alive": 120,
  "clean_session": false
}
```

## Quality of Service (QoS)

MQTT supports three QoS levels:

- **QoS 0 (AtMostOnce)**: Fire and forget, no acknowledgment
- **QoS 1 (AtLeastOnce)**: At least one delivery, may duplicate
- **QoS 2 (ExactlyOnce)**: Exactly one delivery, highest overhead

The LLM can choose the appropriate QoS for each subscribe/publish action based on the use case.

## Topic Wildcards

MQTT supports topic wildcards in subscriptions:

- **+**: Single-level wildcard (e.g., `sensors/+/temperature` matches `sensors/room1/temperature`)
- **#**: Multi-level wildcard (e.g., `sensors/#` matches `sensors/room1/temperature` and `sensors/room2/humidity`)

Wildcards cannot be used in publish topics.

## Limitations

1. **No TLS support (yet)**: Currently plain TCP only. TLS support can be added via `rumqttc::MqttOptions::set_transport()`
2. **No will message**: Last Will and Testament not exposed to LLM (can be added)
3. **No manual acknowledgments**: QoS 1/2 acks are handled automatically by rumqttc
4. **Limited broker state**: No access to broker statistics or connection metrics
5. **Binary payloads**: Only UTF-8 string payloads are supported; binary data would need hex encoding

## Future Enhancements

1. **TLS/SSL support**: Add secure connections with certificate validation
2. **MQTT 5.0**: Upgrade to MQTT 5 for additional features (user properties, reason codes, etc.)
3. **Will messages**: Allow LLM to set Last Will and Testament
4. **Retained message handling**: Better visibility into retained messages
5. **Shared subscriptions**: Support for load balancing across multiple clients
6. **Message queuing**: Buffer messages during LLM processing instead of dropping

## Testing Strategy

See `tests/client/mqtt/CLAUDE.md` for detailed testing documentation.

## References

- rumqttc documentation: https://docs.rs/rumqttc/
- MQTT 3.1.1 specification: https://docs.oasis-open.org/mqtt/mqtt/v3.1.1/mqtt-v3.1.1.html
- Mosquitto broker: https://mosquitto.org/
