# DynamoDB Client Implementation

## Overview

The DynamoDB client implementation provides LLM-controlled access to AWS DynamoDB or local DynamoDB instances (DynamoDB
Local, LocalStack). The LLM can execute DynamoDB operations and interpret responses.

## Implementation Details

### Library Choice

- **aws-sdk-dynamodb** - Official AWS SDK for Rust
- Supports all standard DynamoDB operations
- AWS Signature v4 authentication
- HTTP-based (uses TLS for AWS, optional for local)

### Architecture

```
┌──────────────────────────────────────────┐
│  DynamoDbClient::connect_with_llm_actions│
│  - Initialize AWS SDK config             │
│  - Store credentials/region/endpoint     │
│  - Mark as Connected                     │
└──────────────────────────────────────────┘
         │
         ├─► Operation Methods (PutItem, GetItem, etc.)
         │   - Build AWS SDK request
         │   - Execute via aws-sdk-dynamodb
         │   - Call LLM with response
         │   - Update memory
         │
         └─► Background Monitor Task
             - Checks if client still exists
             - Exits if client removed
```

### Connection Model

Unlike TCP (persistent connection), DynamoDB client is **request/response** based:

- "Connection" = initialization of AWS SDK client with credentials
- Each operation is independent
- LLM triggers operations via actions
- Responses trigger LLM calls for interpretation

### LLM Control

**Async Actions** (user-triggered):

- `put_item` - Put an item into a table
    - Parameters: table_name, item (with DynamoDB types)
- `get_item` - Get an item by primary key
    - Parameters: table_name, key (with DynamoDB types)
- `query` - Query items using key conditions
    - Parameters: table_name, key_condition_expression, expression_attribute_values
- `scan` - Scan all items in a table
    - Parameters: table_name, filter_expression (optional), expression_attribute_values (optional)
- `update_item` - Update an item
    - Parameters: table_name, key, update_expression, expression_attribute_values
- `delete_item` - Delete an item
    - Parameters: table_name, key
- `disconnect` - Stop DynamoDB client

**Sync Actions** (in response to DynamoDB responses):

- `put_item` - Put another item based on response data
- `query` - Query based on response data

**Events:**

- `dynamodb_connected` - Fired when client initialized
- `dynamodb_response_received` - Fired when response received
    - Data includes: operation, success (boolean), data (optional), error (optional)

### Structured Actions (CRITICAL)

DynamoDB client uses **structured data with DynamoDB types**, NOT raw bytes:

```json
// PutItem action
{
  "type": "put_item",
  "table_name": "Users",
  "item": {
    "id": {"S": "user123"},
    "name": {"S": "Alice"},
    "age": {"N": "30"},
    "active": {"BOOL": true}
  }
}

// GetItem action
{
  "type": "get_item",
  "table_name": "Users",
  "key": {
    "id": {"S": "user123"}
  }
}

// Query action
{
  "type": "query",
  "table_name": "Users",
  "key_condition_expression": "id = :id",
  "expression_attribute_values": {
    ":id": {"S": "user123"}
  }
}

// Response event
{
  "event_type": "dynamodb_response_received",
  "data": {
    "operation": "get_item",
    "success": true,
    "data": {
      "table_name": "Users",
      "item": {
        "id": {"S": "user123"},
        "name": {"S": "Alice"},
        "age": {"N": "30"}
      }
    }
  }
}
```

### DynamoDB Type System

DynamoDB uses typed attributes:

- **S** - String
- **N** - Number (stored as string)
- **B** - Binary (base64-encoded)
- **BOOL** - Boolean
- **NULL** - Null value
- **SS** - String Set
- **NS** - Number Set
- **BS** - Binary Set
- **M** - Map (nested object)
- **L** - List (array)

The LLM constructs typed attribute maps, and NetGet converts them to AWS SDK types.

### Startup Parameters

- `region` (optional) - AWS region (default: "us-east-1")
    - Example: "us-west-2", "eu-west-1"
- `endpoint_url` (optional) - Custom endpoint for local testing
    - Example: "http://localhost:8000" (DynamoDB Local)
    - Example: "http://localhost:4566" (LocalStack)
- `access_key_id` (optional) - AWS access key ID
    - Defaults to environment variable AWS_ACCESS_KEY_ID
- `secret_access_key` (optional) - AWS secret access key
    - Defaults to environment variable AWS_SECRET_ACCESS_KEY

### Dual Logging

```rust
info!("DynamoDB client {} PutItem to table {}", client_id, table_name);  // → netget.log
status_tx.send("[CLIENT] DynamoDB PutItem succeeded");                    // → TUI
```

### Error Handling

- **Connection Failed**: Initialization error, client not created
- **Operation Failed**: Log error, return Err, call LLM with error event
- **Authentication Failed**: AWS SDK handles authentication errors
- **LLM Error**: Log, continue accepting actions

## Features

### Supported Operations

- ✅ PutItem
- ✅ GetItem
- ✅ Query
- ✅ Scan
- ✅ UpdateItem
- ✅ DeleteItem
- ⏸ BatchGetItem (future)
- ⏸ BatchWriteItem (future)
- ⏸ TransactWriteItems (future)

### Authentication

- ✅ AWS credentials from environment variables
- ✅ Explicit credentials via startup parameters
- ✅ Custom endpoint for local testing
- ✅ AWS Signature v4 (handled by SDK)

## Limitations

- **No Streaming** - Responses buffered in memory
- **No Pagination** - Large scans/queries return first page only
- **No Complex Types** - Maps (M) and Lists (L) not yet supported
- **No Batch Operations** - BatchGetItem/BatchWriteItem not implemented
- **No Transactions** - TransactWriteItems not implemented
- **No Streams** - DynamoDB Streams not supported

## Usage Examples

### Connect to DynamoDB Local

**User**: "Connect to DynamoDB Local at localhost:8000"

**Startup Parameters**:

```json
{
  "region": "us-east-1",
  "endpoint_url": "http://localhost:8000",
  "access_key_id": "fakeAccessKeyId",
  "secret_access_key": "fakeSecretAccessKey"
}
```

### PutItem

**User**: "Put a user with id=user123, name=Alice, age=30"

**LLM Action**:

```json
{
  "type": "put_item",
  "table_name": "Users",
  "item": {
    "id": {"S": "user123"},
    "name": {"S": "Alice"},
    "age": {"N": "30"}
  }
}
```

### GetItem

**User**: "Get the user with id=user123"

**LLM Action**:

```json
{
  "type": "get_item",
  "table_name": "Users",
  "key": {
    "id": {"S": "user123"}
  }
}
```

### Query

**User**: "Query all users with id=user123"

**LLM Action**:

```json
{
  "type": "query",
  "table_name": "Users",
  "key_condition_expression": "id = :id",
  "expression_attribute_values": {
    ":id": {"S": "user123"}
  }
}
```

### Scan with Filter

**User**: "Scan all users older than 21"

**LLM Action**:

```json
{
  "type": "scan",
  "table_name": "Users",
  "filter_expression": "age > :min_age",
  "expression_attribute_values": {
    ":min_age": {"N": "21"}
  }
}
```

### UpdateItem

**User**: "Update user123's age to 31"

**LLM Action**:

```json
{
  "type": "update_item",
  "table_name": "Users",
  "key": {
    "id": {"S": "user123"}
  },
  "update_expression": "SET age = :age",
  "expression_attribute_values": {
    ":age": {"N": "31"}
  }
}
```

### DeleteItem

**User**: "Delete user with id=user123"

**LLM Action**:

```json
{
  "type": "delete_item",
  "table_name": "Users",
  "key": {
    "id": {"S": "user123"}
  }
}
```

## Testing Strategy

See `tests/client/dynamodb/CLAUDE.md` for E2E testing approach.

Recommended test setup:

- **DynamoDB Local** - Standalone Java application
    - Download: https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/DynamoDBLocal.html
    - Run: `java -Djava.library.path=./DynamoDBLocal_lib -jar DynamoDBLocal.jar -sharedDb`
    - Endpoint: http://localhost:8000
- **LocalStack** - AWS service emulator
    - Docker: `docker run -p 4566:4566 localstack/localstack`
    - Endpoint: http://localhost:4566

## Future Enhancements

- **Pagination** - Handle large result sets with pagination tokens
- **Batch Operations** - BatchGetItem, BatchWriteItem
- **Transactions** - TransactWriteItems, TransactGetItems
- **Complex Types** - Maps (M) and Lists (L) attribute types
- **TTL Support** - Time to Live attribute handling
- **Global Secondary Indexes** - Query GSIs
- **Conditional Operations** - ConditionExpression support
- **DynamoDB Streams** - Real-time change data capture

## Command channel (dashboard `[ send ]`)

`AppState::send_to_client` can inject an action into a running DynamoDB client.
`connect_with_llm_actions` registers the channel (`command_support::register_command_channel`)
and spawns a registered `command_loop` task in place of the old 5s "has this client been removed
yet" poll — dropping the client drops the handle and `recv()` returns `None` immediately.

**This closed a larger hole than the channel itself.** The client had no dispatcher at all:
`execute_action` advertised six verbs, `put_item` and `get_item` existed as standalone
functions nothing called, and `query`, `scan`, `update_item` and `delete_item` had no
implementation. `DynamoDbClient::execute_operation` is now the single dispatch point and all six
verbs are implemented against the SDK, so an advertised verb can no longer be silently swallowed.

Outcome semantics — the AWS SDK owns the socket and reports no wire byte count, so **`Sent` is
never returned**:

| Situation | `ClientSendOutcome` |
|---|---|
| `execute_action` refused it | `Rejected { error }` |
| Operation ran and the service answered | `Executed { detail: "put_item completed: {…}" }` |
| Operation ran and the service or the SDK failed | `Executed { detail: "put_item failed: …" }` |
| `disconnect` | `Disconnected` (loop ends, handle removed) |

The DynamoDB call itself is **awaited** in the loop, so the detail is a real result. The
`dynamodb_response_received` event is raised from its own registered task, so an event handler
that parks for a human answer cannot wedge the command loop.

**Known gap (not closed here):** this client still raises no `dynamodb_connected` event —
`connect_with_llm_actions` never calls `call_llm_for_client` — so the injected/command path is
currently the *only* way an action reaches the wire. The LLM cannot drive this client until a
connected-event call is added.

Test: `tests/client/dynamodb/command_channel_test.rs` (no LLM, no AWS — `endpoint_url` is a
loopback listener). Note that `tests/client/mod.rs` gates `pub mod dynamodb` on the **`dynamo`**
feature alias, not `dynamodb`, so the directory only compiles when `dynamo` is enabled.
