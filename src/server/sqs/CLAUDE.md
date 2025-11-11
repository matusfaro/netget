# AWS SQS Protocol Implementation

## Overview

AWS SQS (Simple Queue Service) compatible server implementing the AWS SQS HTTP/JSON API. The server handles SQS queue
operations (SendMessage, ReceiveMessage, CreateQueue, etc.) with full LLM control over responses. This is a "virtual"
message queue system where the LLM maintains queues and messages through conversation context rather than persistent
storage.

**Port**: 9324 (standard SQS local port)
**Protocol**: HTTP/1.1 with JSON payloads (AWS JSON protocol)
**API Version**: AmazonSQS
**Stack Representation**: `ETH>IP>TCP>HTTP>SQS`

## Library Choices

**hyper** (v1.5):

- HTTP/1.1 server implementation
- Async connection handling with tokio
- Service-based request routing
- Handles HTTP framing, headers, body parsing

**http-body-util**:

- Body aggregation (`BodyExt::collect()`)
- Full body type (`Full<Bytes>`)
- Efficient byte handling

**serde_json**:

- JSON request/response parsing
- SQS uses JSON for all API calls (AWS JSON protocol)
- No binary protocol support (Query protocol not implemented)

**Manual API Implementation**:

- LLM controls all SQS operations through action system
- No AWS SDK dependencies
- Responses manually constructed as JSON
- Request ID generation (timestamp-based)

**Rationale**:

- No suitable Rust SQS server library exists
- Manual implementation provides full LLM control
- Similar to DynamoDB implementation pattern (proven approach)
- HTTP-based protocol is well-understood and tested
- Allows LLM-controlled authentication decisions

## Architecture Decisions

### HTTP-Based Design

- Each SQS request is a POST to the root endpoint
- Operation specified in `x-amz-target` header (e.g., "AmazonSQS.SendMessage")
- Request body is JSON with operation parameters
- Response body is JSON with operation results
- Standard HTTP status codes (200, 400, 500)

### JSON Protocol Only

- Uses AWS JSON protocol (`Content-Type: application/x-amz-json-1.0`)
- 23% faster than legacy Query protocol
- Simpler parsing and generation
- Modern default for AWS
- Legacy Query protocol (form-encoded) not implemented

### Stateful Operation

- Unlike DynamoDB (stateless), SQS requires queue state across requests
- LLM maintains "virtual" queues through conversation context
- Messages persist in LLM memory between requests
- Visibility timeouts tracked with timestamps
- Receipt handles enable message deletion

### Request Processing Flow

1. Accept TCP connection
2. Parse HTTP request (method, URI, headers, body)
3. Extract operation from `x-amz-target` header
4. Parse queue URL from JSON body (if present)
5. Create `SQS_REQUEST_EVENT` with operation, queue_url, request_body
6. Call LLM via `call_llm()` with event and protocol
7. Process action result:
    - `send_sqs_response`: Build HTTP response with status/body
8. If no action, return empty JSON `{}`
9. Close connection (HTTP/1.1 without keep-alive)

### Operation Detection

- Operations parsed from `x-amz-target` header
- Format: `AmazonSQS.<Operation>`
- Supported operations: SendMessage, ReceiveMessage, DeleteMessage, CreateQueue, DeleteQueue, GetQueueAttributes,
  PurgeQueue, ListQueues
- LLM decides how to respond to each operation

### Response Format

- Status: 200 (success), 400 (client error), 500 (server error)
- Headers:
    - `Content-Type: application/x-amz-json-1.0`
    - `x-amzn-RequestId: <hex-timestamp>`
- Body: JSON object with operation-specific fields
- Error format: `{"__type": "ErrorType", "message": "error message"}`

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):

- `send_sqs_response`: Return HTTP response with status and body

**Event Types**:

- `SQS_REQUEST_EVENT`: Fired for every SQS operation
    - Data: `{ "operation": "SendMessage", "queue_url": "http://...", "request_body": "{...}" }`

### Startup Parameters

The LLM can configure the SQS server with these parameters via the `open_server` action:

- **`default_visibility_timeout`** (number): Default visibility timeout in seconds (0-43200, default: 30)
- **`default_message_retention`** (number): Default message retention period in seconds (60-1209600, default: 345600 = 4
  days)
- **`max_receive_count`** (number): Maximum receives before message considered undeliverable (default: 10)

### Example LLM Prompts

**CreateQueue operation**:

```
For CreateQueue with QueueName "orders-queue", use send_sqs_response with:
status_code=200
body='{"QueueUrl":"http://localhost:9324/queue/orders-queue"}'
```

**SendMessage operation**:

```
For SendMessage to orders-queue with body "Order #123", use send_sqs_response with:
status_code=200
body='{"MessageId":"msg-1234567890","MD5OfMessageBody":"d41d8cd98f00b204e9800998ecf8427e"}'
```

**ReceiveMessage operation**:

```
For ReceiveMessage from orders-queue, use send_sqs_response with:
status_code=200
body='{"Messages":[{"MessageId":"msg-1234567890","ReceiptHandle":"receipt-xyz","Body":"Order #123","Attributes":{"SentTimestamp":"1234567890","ApproximateReceiveCount":"1"}}]}'
```

**DeleteMessage operation**:

```
For DeleteMessage with valid receipt handle, use send_sqs_response with:
status_code=200
body='{}'
```

**Error responses**:

```
For invalid queue URL, use send_sqs_response with:
status_code=400
body='{"__type":"QueueDoesNotExist","message":"The specified queue does not exist"}'
```

## Connection Management

### Connection Lifecycle

1. Server accepts TCP connection on port 9324
2. Create `ConnectionId` for tracking
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::Sqs`
4. Spawn HTTP service handler
5. `http1::Builder` serves single request
6. Connection closed after response sent

### State Tracking

- Connection state stored in `ServerInstance.connections` HashMap
- Protocol-specific: `recent_operations` Vec (operation, queue_url, time)
- Tracks: remote_addr, local_addr, bytes_sent/received
- Status: Active → Closed after each request
- HTTP/1.1 without keep-alive (new connection per request)

### Concurrency

- Multiple connections handled concurrently
- Each connection is independent
- LLM maintains queue state through conversation memory
- Message visibility timeouts prevent concurrent processing

## State Management

### Virtual Queues

- **Queue Creation**: LLM "creates" queue by remembering it in conversation
- **Queue URL Format**: `http://localhost:<port>/queue/<QueueName>`
- **Queue Attributes**: Visibility timeout, message retention, ARN
- **Queue Deletion**: LLM "forgets" queue and all its messages

### Virtual Messages

- **Message Storage**: LLM maintains message list for each queue
- **Message ID Format**: `msg-<timestamp>-<random>`
- **Message Attributes**: Body, attributes, sent timestamp, receive count
- **Receipt Handle Format**: `receipt-<timestamp>-<message_id>`

### Visibility Timeout

- **In-Flight Messages**: Messages become invisible after ReceiveMessage
- **Timeout Tracking**: LLM tracks timestamp of receive operation
- **Expiration**: After visibility timeout, message becomes available again
- **Validation**: LLM checks timestamp when processing DeleteMessage

### Message Lifecycle

1. **SendMessage**: Message added to queue with unique ID and MD5
2. **ReceiveMessage**: Message marked in-flight with receipt handle and visibility timeout
3. **DeleteMessage**: Message permanently removed using receipt handle
4. **Expiration**: Message deleted after retention period

## LLM-Controlled Authentication

The SQS server supports LLM-controlled authentication:

### Authentication Flow

1. Client sends request with AWS Signature V4 headers
2. Server parses signature headers (but does not validate)
3. Signature details included in event context
4. **LLM decides whether to accept or reject the request**
5. LLM can return error response for invalid/missing signatures

### Signature Headers

- `Authorization`: AWS4-HMAC-SHA256 signature
- `X-Amz-Date`: Request timestamp
- `X-Amz-Security-Token`: (optional) Session token

### LLM Decisions

- **Accept all**: Honeypot mode, log but accept everything
- **Reject invalid**: Validate signature format (not crypto)
- **Selective auth**: Accept specific access keys, reject others
- **Custom logic**: Time-based access, rate limiting, etc.

## Scripting Mode Support

The SQS protocol is designed with scripting mode in mind to minimize LLM calls during testing:

### Scriptable Operations

- **SendMessage**: Repetitive, predictable response (MessageId, MD5)
- **GetQueueAttributes**: Simple response with queue stats

### Non-Scriptable Operations

- **ReceiveMessage**: Requires queue state inspection
- **DeleteMessage**: Requires receipt handle validation
- **CreateQueue**: Initial queue setup

### Script Generation

- On server startup, LLM generates Python/JavaScript script
- Script handles SendMessage requests without calling LLM
- Script can access queue state from LLM-provided context
- Reduces E2E test LLM calls from ~10 to ~3-5

## Limitations

### Protocol Features

- **No persistent storage** - queues and messages only exist in LLM conversation context
- **No authentication crypto** - AWS Signature V4 parsed but not cryptographically validated
- **HTTP/1.1 only** - no HTTP/2 support
- **No keep-alive** - new connection per request
- **JSON protocol only** - legacy Query protocol not implemented
- **No streaming** - full request/response buffering
- **Standard queues only** - FIFO queues not implemented
- **No DLQ** - Dead Letter Queues not implemented
- **No long polling** - WaitTimeSeconds supported in design but requires async waiting
- **No message attributes** - Supported in design, LLM can include in responses
- **No batch operations** - SendMessageBatch, DeleteMessageBatch not yet implemented

### Performance

- Each request triggers LLM call (unless scripting enabled)
- No query optimization
- Full request/response in memory
- Connection overhead per request

### Data Management

- **Virtual data** - LLM maintains queues and messages through conversation
- **No persistence** - data lost when LLM context is cleared
- **Consistency** - depends on LLM memory
- **Scalability** - limited by LLM context window

## Known Issues

1. **Data consistency**: LLM may forget or hallucinate messages between requests
2. **Receipt handle validation**: Timestamp-based handles may be guessed (low probability)
3. **Visibility timeout accuracy**: Depends on LLM timestamp arithmetic
4. **Message ordering**: Standard queues don't guarantee order (by design)
5. **Request ID uniqueness**: Timestamp-based IDs may collide (very rare)
6. **Error codes**: Limited AWS error code vocabulary

## Example Responses

### CreateQueue Success

```json
{
  "actions": [
    {
      "type": "send_sqs_response",
      "status_code": 200,
      "body": "{\"QueueUrl\":\"http://localhost:9324/queue/orders-queue\"}"
    }
  ]
}
```

### SendMessage Success

```json
{
  "actions": [
    {
      "type": "send_sqs_response",
      "status_code": 200,
      "body": "{\"MessageId\":\"msg-123\",\"MD5OfMessageBody\":\"d41d8cd98f00b204e9800998ecf8427e\"}"
    }
  ]
}
```

### ReceiveMessage Response

```json
{
  "actions": [
    {
      "type": "send_sqs_response",
      "status_code": 200,
      "body": "{\"Messages\":[{\"MessageId\":\"msg-123\",\"ReceiptHandle\":\"receipt-xyz\",\"Body\":\"Hello\"}]}"
    }
  ]
}
```

### DeleteMessage Success

```json
{
  "actions": [
    {
      "type": "send_sqs_response",
      "status_code": 200,
      "body": "{}"
    }
  ]
}
```

### Error Response

```json
{
  "actions": [
    {
      "type": "send_sqs_response",
      "status_code": 400,
      "body": "{\"__type\":\"QueueDoesNotExist\",\"message\":\"The specified queue does not exist\"}"
    }
  ]
}
```

## Comparison to DynamoDB

### Similarities

- HTTP-based protocol with JSON payloads
- Operation specified in header (`x-amz-target`)
- LLM maintains "virtual" data in conversation context
- No authentication (simplified for testing/honeypot)
- Action-based response system
- Manual API implementation

### Differences

| Aspect                | DynamoDB                             | SQS                                     |
|-----------------------|--------------------------------------|-----------------------------------------|
| **State**             | Stateless (each request independent) | Stateful (queue persistence required)   |
| **Data Lifecycle**    | Items stored indefinitely            | Messages expire after retention period  |
| **Operations**        | CRUD (read/write operations)         | Queue operations (send/receive/delete)  |
| **Concurrency**       | Simple (no coordination)             | Complex (visibility timeout, in-flight) |
| **Temporal Behavior** | None                                 | Visibility timeouts, message expiration |
| **Header**            | `DynamoDB_20120810.Operation`        | `AmazonSQS.Operation`                   |
| **Default Port**      | 8000                                 | 9324                                    |
| **Stack Name**        | `ETH>IP>TCP>HTTP>DYNAMODB`           | `ETH>IP>TCP>HTTP>SQS`                   |

### Key Architectural Difference: State Management

**DynamoDB**: Each request is independent, no state between requests

- GetItem: LLM "retrieves" from conversation memory
- PutItem: LLM "stores" in conversation memory
- No coordination needed

**SQS**: Requires queue state across requests

- SendMessage: Message added to queue
- ReceiveMessage: Message marked in-flight with timeout
- DeleteMessage: Message removed permanently
- Requires temporal state (visibility timeouts, expiration)

**Implication**: SQS prompt must emphasize state persistence and temporal behavior more than DynamoDB.

## References

- [AWS SQS API Reference](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/APIReference/)
- [SQS Developer Guide](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/)
- [AWS JSON Protocol](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-json-faqs.html)
- [AWS SDK for Rust - SQS](https://github.com/awslabs/aws-sdk-rust) - for testing
- [ElasticMQ](https://github.com/softwaremill/elasticmq) - SQS-compatible server (Scala, inspiration)
