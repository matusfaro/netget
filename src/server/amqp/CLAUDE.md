# AMQP Server Implementation

## Overview

Simplified AMQP 0.9.1 broker implementation with LLM control. Provides basic message queuing capabilities compatible with RabbitMQ protocol.

## Library Choices

**No external AMQP server library** - Custom wire protocol implementation
- **Rationale**: No mature Rust AMQP 0.9.1 server library exists
- **Approach**: Manual frame parsing and basic protocol handling
- **Tokio**: Async runtime for TCP connections and frame I/O

## Architecture

### Wire Protocol
- **AMQP 0.9.1 framing**: Type + Channel + Size + Payload + End marker (0xCE)
- **Frame types**: Method (1), Content Header (2), Content Body (3), Heartbeat (8)
- **Connection flow**: Protocol header → Connection.Start → Client response → Channel operations

### LLM Integration Points

**Connection Events**:
- Client connects (protocol header received)
- Authentication completed
- Channel opened/closed
- Client disconnects

**Method Handlers** (LLM decides responses):
- **Queue operations**: Declare, Bind, Purge, Delete
- **Exchange operations**: Declare, Delete, Bind
- **Publishing**: Basic.Publish (LLM routes message)
- **Consuming**: Basic.Consume, Basic.Get (LLM generates messages)
- **Acknowledgments**: Basic.Ack, Basic.Nack, Basic.Reject

### State Management

**No Persistent Storage** - Following project philosophy:
- **Queues**: LLM tracks queue declarations in memory
- **Exchanges**: LLM tracks exchange types and bindings
- **Messages**: LLM generates message content on demand
- **Routing**: LLM decides message routing based on exchange type and routing key

Similar to MySQL protocol: No actual database, LLM answers all queries from memory/knowledge.

### Dual Logging

All operations logged via:
- **Tracing macros**: `info!`, `debug!`, `trace!` → `netget.log`
- **status_tx channel**: → TUI for real-time display

## Limitations

1. **Simplified Protocol**: Only core AMQP 0.9.1 methods implemented
2. **No Transactions**: No tx.select/commit/rollback support
3. **No Publisher Confirms**: Publish confirmations not implemented
4. **Basic Auth**: Only PLAIN authentication supported
5. **No Clustering**: Single-instance broker only
6. **No Persistence**: All state in memory (LLM tracks)
7. **Limited QoS**: No prefetch or flow control
8. **No SSL/TLS**: Plain TCP only

## Example LLM Interactions

**Queue Declaration**:
```
User: Create a durable queue named "tasks"
LLM Action: {
  "type": "declare_queue",
  "queue_name": "tasks",
  "durable": true,
  "exclusive": false,
  "auto_delete": false
}
```

**Message Publishing**:
```
User: Publish "Hello, World!" to exchange "logs" with routing key "info"
LLM Action: {
  "type": "publish_message",
  "exchange_name": "logs",
  "routing_key": "info",
  "message_body": "Hello, World!",
  "properties": {"delivery_mode": 2}
}
```

**Message Consumption**:
```
Event: {
  "type": "basic_consume",
  "queue_name": "tasks",
  "consumer_tag": "consumer-1"
}
LLM Action: {
  "type": "consume_message",
  "queue_name": "tasks",
  "consumer_tag": "consumer-1"
  // LLM generates message content
}
```

## Testing Approach

See `tests/server/amqp/CLAUDE.md` for testing strategy.

## Future Enhancements

1. **Full Method Coverage**: Implement remaining AMQP methods
2. **Exchange Types**: Complete fanout, topic, headers routing
3. **TLS Support**: Add SSL/TLS encryption
4. **Authentication**: Add more auth mechanisms (EXTERNAL, SCRAM)
5. **Management API**: Add RabbitMQ-style management interface
6. **Plugins**: Support RabbitMQ plugin-like extensions

## References

- [AMQP 0.9.1 Specification](https://www.rabbitmq.com/resources/specs/amqp0-9-1.pdf)
- [RabbitMQ Protocol Tutorial](https://www.rabbitmq.com/tutorials/amqp-concepts.html)
