# AWS SQS Client Protocol Implementation

## Overview

AWS SQS (Simple Queue Service) client implementation using the official AWS SDK. The client provides full LLM control
over queue operations including sending messages, receiving messages, deleting messages, and managing queue attributes.

**Protocol**: HTTP/1.1 with AWS JSON protocol
**API Version**: AmazonSQS
**Stack Representation**: `ETH>IP>TCP>HTTP>SQS`

## Library Choices

**aws-sdk-sqs** (v1.86):

- Official AWS SDK for Rust
- Full SQS API support (SendMessage, ReceiveMessage, DeleteMessage, etc.)
- Built-in AWS authentication (credentials, IAM roles, environment)
- Automatic request signing (AWS Signature v4)
- Async/await with tokio
- Comprehensive error handling

**aws-config** (v1.5):

- AWS SDK configuration and credential loading
- Supports multiple credential sources:
    - Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
    - AWS credentials file (~/.aws/credentials)
    - IAM instance profiles (EC2)
    - IAM roles (ECS, Lambda)
- Region configuration
- Custom endpoint support (for LocalStack)

**Rationale**:

- Official SDK provides reliability and compliance
- No need to implement AWS authentication/signing manually
- Supports both production AWS and local testing (LocalStack)
- Well-maintained with regular security updates
- Type-safe API with comprehensive error handling

## Architecture Decisions

### HTTP-Based Design

- SQS uses HTTP POST requests to AWS API endpoints
- Operation specified in `x-amz-target` header (e.g., "AmazonSQS.SendMessage")
- Request/response bodies use JSON (AWS JSON protocol)
- SDK handles all HTTP details transparently

### No Real Socket Connection

- Unlike TCP/Redis clients, SQS has no persistent connection
- Each operation is a stateless HTTP request
- Client returns a dummy socket address for API compatibility
- Uses `127.0.0.1:{10000+client_id}` as placeholder

### Startup Parameters

The SQS client requires specific startup parameters:

- **queue_url** (required): Full SQS queue URL
    - Format: `https://sqs.{region}.amazonaws.com/{account_id}/{queue_name}`
    - Example: `https://sqs.us-east-1.amazonaws.com/123456789012/MyQueue`
- **region** (optional): AWS region (e.g., "us-east-1")
    - Defaults to AWS config or environment
- **endpoint_url** (optional): Custom endpoint for local testing
    - Example: `http://localhost:9324` (LocalStack)

### LLM Integration Flow

1. **Connection**: Initialize AWS SDK client with queue URL and credentials
2. **Connected Event**: Call LLM with `sqs_connected` event
3. **LLM Actions**: Execute actions from LLM response
4. **Operations**: Each operation triggers follow-up LLM calls with results
5. **Events**: Three event types:
    - `sqs_connected`: Initial connection
    - `sqs_message_sent`: After sending a message
    - `sqs_message_received`: After receiving messages

### Action Execution Pattern

Unlike streaming protocols (TCP, Redis), SQS uses a request-response pattern:

1. LLM generates action (e.g., "receive_messages")
2. Client executes AWS SDK call
3. Result triggers new LLM call with event
4. LLM processes result and generates next action
5. Repeat until disconnect

## LLM Control Points

### Async Actions (User-Triggered)

- **send_message**: Send message to queue
    - Parameters: message_body, message_attributes, delay_seconds
    - Returns: message_id via `sqs_message_sent` event
- **receive_messages**: Poll queue for messages (long polling supported)
    - Parameters: max_messages (1-10), wait_time_seconds (0-20), visibility_timeout
    - Returns: array of messages via `sqs_message_received` event
- **delete_message**: Delete message using receipt handle
    - Parameters: receipt_handle (from received message)
    - Confirms deletion success
- **purge_queue**: Delete all messages in queue
    - No parameters
    - Use with caution (irreversible)
- **get_queue_attributes**: Get queue metadata
    - Parameters: attribute_names (optional list)
    - Returns: queue attributes (message count, ARN, etc.)
- **disconnect**: Close client

### Sync Actions (Response to Events)

- **send_message**: Send message after receiving messages
- **delete_message**: Delete message after processing

### Event Flow Examples

**Example 1: Send Message**

```
LLM Action: {"type": "send_message", "message_body": "Hello"}
  → AWS SDK: SendMessage API call
  → Event: {"event": "sqs_message_sent", "message_id": "abc123"}
  → LLM: Process confirmation
```

**Example 2: Receive and Process**

```
LLM Action: {"type": "receive_messages", "max_messages": 5}
  → AWS SDK: ReceiveMessage API call (long polling)
  → Event: {"event": "sqs_message_received", "messages": [...]}
  → LLM: Process messages, generate delete actions
  → LLM Action: {"type": "delete_message", "receipt_handle": "xyz"}
  → AWS SDK: DeleteMessage API call
```

## Message Format

### Received Messages

Messages returned by ReceiveMessage include:

- **message_id**: Unique message identifier
- **receipt_handle**: Handle for deletion (required for delete_message)
- **body**: Message content (string)
- **attributes**: System attributes (timestamp, sender, etc.)
- **message_attributes**: User-defined attributes (key-value pairs)

### Message Attributes

LLM can set custom attributes when sending:

```json
{
  "type": "send_message",
  "message_body": "Order placed",
  "message_attributes": {
    "order_id": "12345",
    "priority": "high"
  }
}
```

Attributes are transmitted as typed values (String, Number, Binary).

## Authentication

The AWS SDK supports multiple credential sources (in order of precedence):

1. Environment variables: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`
2. AWS credentials file: `~/.aws/credentials`
3. IAM instance profile (EC2)
4. IAM role (ECS, Lambda)

For local testing with LocalStack:

```bash
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=us-east-1
```

## Local Testing with LocalStack

LocalStack provides a local SQS implementation for testing:

```bash
# Start LocalStack with SQS
docker run -d --rm -p 9324:4566 -e SERVICES=sqs localstack/localstack

# Create queue
aws --endpoint-url=http://localhost:9324 sqs create-queue --queue-name MyQueue

# Get queue URL
aws --endpoint-url=http://localhost:9324 sqs list-queues
```

NetGet startup with LocalStack:

```json
{
  "queue_url": "http://localhost:9324/000000000000/MyQueue",
  "endpoint_url": "http://localhost:9324"
}
```

## Limitations

1. **No Persistent Connection**: Each operation is a separate HTTP request
2. **No Real-Time Updates**: Must poll for messages (no push notifications)
3. **Visibility Timeout**: Messages become visible again if not deleted within timeout
4. **Message Ordering**: Standard queues don't guarantee FIFO order
5. **At-Least-Once Delivery**: Messages may be delivered multiple times
6. **No Built-in Retry**: LLM must implement retry logic for failed operations

## Error Handling

AWS SDK errors are propagated to LLM via status messages:

- **Authentication errors**: Invalid credentials, missing permissions
- **Queue not found**: Invalid queue URL
- **Throttling**: Too many requests (AWS rate limits)
- **Network errors**: Connection failures, timeouts

The LLM can respond to errors by:

- Retrying with backoff
- Switching to a different queue
- Logging error and continuing
- Disconnecting

## Long Polling

SQS supports long polling to reduce API calls:

```json
{
  "type": "receive_messages",
  "wait_time_seconds": 20
}
```

Benefits:

- Reduces empty responses
- Lower API costs
- More efficient than short polling
- LLM can use this to wait for messages instead of busy-looping

## Use Cases

1. **Message Producer**: Send messages to queue for processing
2. **Message Consumer**: Poll queue, process messages, delete after success
3. **Queue Monitoring**: Check queue attributes (message count, age)
4. **Dead Letter Queue Processing**: Receive failed messages from DLQ
5. **Event-Driven Workflows**: React to messages and trigger actions
6. **Testing**: Local SQS testing with LocalStack

## Example LLM Prompts

1. **Send Message**: "Connect to SQS queue MyQueue and send a test message"
2. **Receive Messages**: "Poll the queue for up to 10 messages with 20 second wait time"
3. **Process and Delete**: "Receive messages, log their content, and delete them"
4. **Monitor Queue**: "Get the approximate number of messages in the queue"
5. **Purge Queue**: "Delete all messages from the queue"

## Comparison with SQS Server

| Aspect              | Client                           | Server                          |
|---------------------|----------------------------------|---------------------------------|
| **Purpose**         | Connect to AWS/LocalStack        | Accept connections from clients |
| **Implementation**  | AWS SDK (aws-sdk-sqs)            | Manual HTTP (hyper)             |
| **Authentication**  | SDK handles automatically        | LLM controls auth decisions     |
| **Message Storage** | AWS manages                      | LLM memory/state                |
| **Queue URL**       | Startup parameter                | Generated by server             |
| **Operations**      | All SQS API calls                | LLM-controlled responses        |
| **Use Case**        | Test SQS server, AWS integration | Honeypot, testing, simulation   |
