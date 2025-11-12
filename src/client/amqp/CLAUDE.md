# AMQP Client Implementation

## Overview

AMQP 0.9.1 client for connecting to RabbitMQ and other AMQP brokers. Uses lapin library with LLM control.

## Library Choices

**lapin v2.6** - Async AMQP client
- **Rationale**: Mature, actively maintained Rust AMQP 0.9.1 client
- **Features**: Full protocol support, async/await, Tokio integration
- **Compatibility**: Works with RabbitMQ, Azure Service Bus, Apache Qpid
- **Alternatives**: amqprs (newer but less mature)

## Architecture

### Connection Management
- **lapin::Connection**: Main connection to AMQP broker
- **lapin::Channel**: Multiplexed channels for operations
- **Properties**: Default connection properties (locale, heartbeat)

### LLM Integration Points

**Connection Events**:
```rust
AMQP_CLIENT_CONNECTED_EVENT
- Fired when connected to broker
- LLM decides initial actions (open channel, declare resources)
```

**Channel Events**:
```rust
AMQP_CLIENT_CHANNEL_OPENED_EVENT
- Fired when channel created
- LLM can declare queues/exchanges, bind queues, start consumers
```

**Message Events**:
```rust
AMQP_CLIENT_MESSAGE_RECEIVED_EVENT
- Fired when message arrives from queue
- LLM processes message content and decides action (ack, nack, etc.)
```

### Available Actions

**Async Actions** (User-triggered):
- `open_channel`: Create new channel for operations
- `declare_queue`: Declare queue with options (durable, exclusive, auto_delete)
- `declare_exchange`: Declare exchange with type (direct, fanout, topic, headers)
- `bind_queue`: Bind queue to exchange with routing key
- `publish_message`: Publish message to exchange
- `start_consumer`: Start consuming from queue

**Sync Actions** (Network event responses):
- `ack_message`: Acknowledge message delivery
- `nack_message`: Negative acknowledge (reject and optionally requeue)

### State Management

**No Storage** - Following project philosophy:
- LLM tracks which queues/exchanges are declared
- LLM remembers bindings and routing rules
- LLM generates message content as needed
- lapin handles connection/channel state internally

### Dual Logging

All operations logged via:
- **Tracing macros**: `info!`, `debug!`, `trace!` → `netget.log`
- **status_tx channel**: → TUI for real-time display

## Limitations

1. **No Local Address**: lapin doesn't expose TCP local address (uses placeholder)
2. **No Transactions**: tx.select/commit/rollback not exposed to LLM
3. **Basic QoS**: Only basic Quality of Service settings
4. **No Publisher Confirms**: Publish confirmations not exposed
5. **Limited TLS Config**: Basic TLS only, no custom certificates via LLM
6. **Single Connection**: One connection per client instance

## Example LLM Interactions

**Connect and Declare**:
```
User: Connect to RabbitMQ at localhost:5672 and declare a queue named "events"
→ Client connects
→ Event: amqp_connected
LLM Action: {
  "type": "open_channel"
}
→ Event: amqp_channel_opened
LLM Action: {
  "type": "declare_queue",
  "queue_name": "events",
  "durable": true
}
```

**Publish Message**:
```
User: Publish "Task complete" to exchange "work" with routing key "completed"
LLM Action: {
  "type": "publish_message",
  "exchange_name": "work",
  "routing_key": "completed",
  "message_body": "Task complete"
}
```

**Consume Messages**:
```
User: Start consuming from queue "tasks"
LLM Action: {
  "type": "start_consumer",
  "queue_name": "tasks"
}
→ Messages arrive
→ Event: amqp_message_received (for each message)
LLM processes and decides to ack:
LLM Action: {
  "type": "ack_message",
  "delivery_tag": 123
}
```

## Testing Approach

See `tests/client/amqp/CLAUDE.md` for testing strategy.

## Future Enhancements

1. **TLS Configuration**: Allow LLM to configure TLS certificates
2. **Publisher Confirms**: Expose confirm mode to LLM
3. **QoS Control**: Allow LLM to set prefetch limits
4. **Transaction Support**: Expose tx methods for atomic operations
5. **Dead Letter Exchanges**: Support DLX configuration
6. **Message TTL**: Support per-message and per-queue TTL

## References

- [lapin Documentation](https://docs.rs/lapin/)
- [RabbitMQ Tutorials](https://www.rabbitmq.com/getstarted.html)
- [AMQP 0.9.1 Spec](https://www.rabbitmq.com/resources/specs/amqp0-9-1.pdf)
