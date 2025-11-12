# AMQP Server Testing

## Test Strategy

Black-box testing with real AMQP client (lapin) connecting to LLM-controlled server.

## LLM Call Budget

**Target**: < 10 LLM calls per test suite
- **Connection test**: 1-2 calls (protocol header, Connection.Start)
- **Queue operations**: 2-3 calls (declare, bind, query)
- **Message flow**: 3-4 calls (publish, consume, ack)
- **Total**: ~6-9 calls

## Budget Optimization Strategies

1. **Reuse Server**: Single server instance across tests
2. **Batch Operations**: Group related AMQP operations
3. **Static Responses**: Pre-configure common responses
4. **Scripting Mode**: Use scripting for repetitive operations

## Test Categories

### 1. Connection Tests
- Protocol header exchange
- Connection.Start/Start-Ok
- Channel open/close
- **LLM Calls**: 1-2

### 2. Queue Operations
- Queue.Declare with various options
- Queue.Bind to exchanges
- Queue.Purge, Queue.Delete
- **LLM Calls**: 2-3 (reuse channel)

### 3. Publishing
- Basic.Publish to exchanges
- Message properties (delivery_mode, content_type)
- Routing by key
- **LLM Calls**: 1-2

### 4. Consuming
- Basic.Consume setup
- Message delivery
- Basic.Ack/Nack/Reject
- **LLM Calls**: 2-3

## Expected Runtime

**Total**: 30-60 seconds (with Ollama LLM)
- Server startup: 5-10s
- Connection tests: 5-10s
- Queue operations: 5-10s
- Message flow: 10-20s
- Cleanup: 5-10s

## Known Issues

1. **Simplified Protocol**: Only core AMQP methods tested
2. **No Transactions**: tx methods not tested
3. **No Publisher Confirms**: Confirm mode not tested
4. **Limited Auth**: Only PLAIN mechanism tested
5. **Frame Parsing**: May fail on complex content

## Test Fixtures

- **Port**: Dynamic (find available port)
- **Virtual Host**: `/` (default)
- **Auth**: guest/guest (standard RabbitMQ default)

## Example Test

```rust
#[tokio::test]
#[cfg(all(test, feature = "amqp"))]
async fn test_amqp_queue_declare() {
    // Start server with LLM
    let server = spawn_amqp_server(/* ... */).await;

    // Connect with lapin client
    let conn = Connection::connect(&server.addr, ConnectionProperties::default()).await?;
    let channel = conn.create_channel().await?;

    // Declare queue (LLM handles)
    let queue = channel.queue_declare(
        "test_queue",
        QueueDeclareOptions::default(),
        FieldTable::default()
    ).await?;

    assert_eq!(queue.name(), "test_queue");
    // LLM calls: 1-2 (connection + declare)
}
```

## Validation

Tests validate:
- Protocol compliance (frame format, method encoding)
- LLM action execution (queue created, message routed)
- Error handling (invalid methods, connection errors)
- State management (channels, consumers)
