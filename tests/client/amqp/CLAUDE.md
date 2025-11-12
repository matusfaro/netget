# AMQP Client Testing

## Test Strategy

Black-box testing with real AMQP broker (RabbitMQ or NetGet AMQP server) and LLM-controlled client.

## LLM Call Budget

**Target**: < 10 LLM calls per test suite
- **Connection test**: 1-2 calls (connected event, open channel)
- **Declare operations**: 2-3 calls (queue, exchange, bind)
- **Publish/consume**: 3-4 calls (publish, consume setup, message processing)
- **Total**: ~6-9 calls

## Budget Optimization Strategies

1. **Reuse Connection**: Single client across tests
2. **Batch Declarations**: Group queue/exchange declarations
3. **Static Actions**: Pre-configure common operations
4. **Mock Broker**: Use NetGet AMQP server (no external RabbitMQ)

## Test Categories

### 1. Connection Tests
- Connect to broker
- Channel creation
- Connection events
- **LLM Calls**: 1-2

### 2. Declaration Tests
- Declare queues (durable, exclusive, auto_delete)
- Declare exchanges (direct, fanout, topic)
- Bind queues to exchanges
- **LLM Calls**: 2-3

### 3. Publishing Tests
- Publish messages to exchanges
- Message properties
- Routing keys
- **LLM Calls**: 1-2

### 4. Consuming Tests
- Start consumer on queue
- Receive messages
- Acknowledge messages
- **LLM Calls**: 2-3

## Expected Runtime

**Total**: 30-60 seconds (with Ollama LLM)
- Broker startup (if using NetGet): 5-10s
- Client connection: 5-10s
- Declaration tests: 5-10s
- Publish/consume: 10-20s
- Cleanup: 5-10s

## Known Issues

1. **No Local Address**: lapin doesn't expose TCP local_addr (uses placeholder)
2. **No Publisher Confirms**: Confirm mode not tested
3. **Limited TLS**: TLS configuration not tested
4. **Single Channel**: Only one channel per test
5. **Broker Dependency**: Requires running AMQP broker (or NetGet server)

## Test Fixtures

- **Broker**: NetGet AMQP server (in-process) or external RabbitMQ
- **Port**: Dynamic (find available port)
- **Virtual Host**: `/` (default)
- **Auth**: guest/guest or none

## Example Test

```rust
#[tokio::test]
#[cfg(all(test, feature = "amqp"))]
async fn test_amqp_client_publish() {
    // Start NetGet AMQP server
    let server = spawn_amqp_server(/* ... */).await;

    // Start LLM-controlled client
    let client_id = spawn_amqp_client(server.addr, /* ... */).await;

    // LLM receives connected event, opens channel, declares queue, publishes
    // Wait for operations to complete
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Validate message was published (via server logs or consumer)
    // LLM calls: 2-3 (connected, channel opened, publish)
}
```

## Validation

Tests validate:
- Connection establishment (lapin connects successfully)
- LLM action execution (queues declared, messages published)
- Message reception (consumer receives messages)
- Acknowledgment (messages ack'd correctly)
- Error handling (connection failures, invalid operations)
