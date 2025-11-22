# MongoDB Server Implementation

## Overview

This MongoDB server implementation provides a MongoDB-compatible server that allows an LLM (via Ollama) to control database responses without maintaining persistent storage. All data is returned by the LLM via actions in response to queries.

## Architecture

### Wire Protocol

- **Protocol**: MongoDB OP_MSG (opCode 2013) - MongoDB 3.6+ wire protocol
- **Encoding**: BSON (Binary JSON) via `bson` crate v3.0
- **Port**: Default 27017 (configurable)
- **Authentication**: Not implemented (Phase 1)

### Libraries Used

- **`bson` v3.0**: Official BSON encoding/decoding library
  - Provides `Document`, `doc!` macro, and BSON value types
  - Conversion between BSON and relaxed extended JSON
  - Serialization/deserialization support
- **No opensrv-mongodb**: Unlike MySQL/PostgreSQL, there's no high-level MongoDB server library
  - We implement OP_MSG parsing manually
  - Direct TCP stream handling with `tokio::io::split()`

### Message Flow

```
Client Connection
    ↓
Read OP_MSG Header (16 bytes)
    ↓
Read OP_MSG Body (BSON document)
    ↓
Parse Command Document
    ├─ Extract: command name, database, collection
    ├─ Extract: filter, document, options
    └─ Convert BSON → JSON for LLM
    ↓
Call LLM with mongodb_command Event
    ↓
LLM Returns Actions (find_response, insert_response, etc.)
    ↓
Execute Actions
    ├─ Convert JSON → BSON Document
    ├─ Encode OP_MSG Response
    └─ Send to Client
    ↓
Loop or Disconnect
```

### OP_MSG Format

**Request Header** (16 bytes):
```
messageLength (4) | requestID (4) | responseTo (4) | opCode (4)
```

**Request Body**:
```
flagBits (4) | sectionKind (1) | BSON Document
```

**Response** (same format):
```
Header: messageLength | responseID=0 | responseTo=requestID | opCode=2013
Body: flagBits=0 | sectionKind=0 | BSON Response Document
```

### Supported Commands

Currently parsed commands (LLM decides how to respond):
- **find**: Query documents (`find_response` action)
- **insert**: Insert documents (`insert_response` action)
- **update**: Update documents (`update_response` action)
- **delete**: Delete documents (`delete_response` action)
- **Custom**: Any other MongoDB command (generic response)

## LLM Integration

### Events

**1. mongodb_command** - Fired when a command is received
```json
{
  "command": "find",
  "database": "testdb",
  "collection": "users",
  "filter": {"age": {"$gte": 18}},
  "document": null
}
```

**2. mongodb_disconnected** - Fired when client disconnects
```json
{
  "reason": "client_disconnect"
}
```

### Actions

**1. find_response** - Return query results
```json
{
  "type": "find_response",
  "documents": [
    {"_id": {"$oid": "507f1f77bcf86cd799439011"}, "name": "Alice", "age": 30},
    {"_id": {"$oid": "507f191e810c19729de860ea"}, "name": "Bob", "age": 25}
  ]
}
```

**2. insert_response** - Acknowledge insert
```json
{
  "type": "insert_response",
  "inserted_count": 1
}
```

**3. update_response** - Acknowledge update
```json
{
  "type": "update_response",
  "matched_count": 2,
  "modified_count": 2
}
```

**4. delete_response** - Acknowledge delete
```json
{
  "type": "delete_response",
  "deleted_count": 3
}
```

**5. error_response** - Return error
```json
{
  "type": "error_response",
  "code": 26,
  "message": "Namespace not found"
}
```

**6. close_this_connection** - Close connection
```json
{
  "type": "close_this_connection"
}
```

## Data Handling

### No Persistent Storage

**CRITICAL**: This MongoDB server does NOT maintain any persistent storage.

- **No HashMap**: No in-memory database of documents
- **No File System**: No disk-based storage
- **LLM Memory**: All "data" exists in LLM's conversation memory
- **Stateless**: Each connection is independent

The LLM is responsible for:
1. Remembering what documents "exist"
2. Filtering documents based on queries
3. Generating realistic `_id` values
4. Maintaining consistency within a conversation

### BSON ↔ JSON Conversion

**Structured Data Only** (no binary):
```rust
// BSON to JSON (for LLM)
fn bson_to_json(&self, bson: Option<&Bson>) -> serde_json::Value {
    match bson {
        Some(b) => b.clone().into_relaxed_extjson(),
        None => serde_json::Value::Null,
    }
}

// JSON to BSON Document (for response)
fn json_to_bson_doc(&self, json: &serde_json::Value) -> Result<Document> {
    match action_type {
        "find_response" => {
            let docs: Vec<Bson> = documents
                .iter()
                .filter_map(|d| Bson::try_from(d.clone()).ok())
                .collect();
            Ok(doc! {
                "ok": 1,
                "cursor": {
                    "id": 0i64,
                    "ns": "test.collection",
                    "firstBatch": docs
                }
            })
        }
        // ... other response types
    }
}
```

## Limitations

1. **No Authentication**: Phase 1 implementation accepts all connections
2. **No Replication**: Single-instance only
3. **No Sharding**: No distributed queries
4. **No Transactions**: No multi-document ACID guarantees
5. **No Aggregation Pipeline**: Would require complex LLM instructions
6. **Simplified OP_MSG**: Only handles section kind 0 (body document)
7. **No Compression**: OP_MSG compression not supported
8. **No Checksums**: Message integrity not validated

## Error Handling

Errors are returned as MongoDB error responses:
```rust
Ok(doc! {
    "ok": 0,
    "code": 26,  // Namespace not found
    "errmsg": "Collection does not exist"
})
```

Common error codes:
- `0`: Generic error
- `11000`: Duplicate key error
- `26`: Namespace not found
- `50`: Maximum time exceeded

## Example Prompts

### Basic Server
```
Listen on port 27017 via MongoDB. Store user documents with name and age fields.
When queried, return Alice (age 30) and Bob (age 25).
```

### Advanced Server
```
Start a MongoDB server on port 27017 for database "ecommerce".

Collections:
- users: {_id, name, email, created_at}
- products: {_id, name, price, stock}
- orders: {_id, user_id, items[], total, status}

Populate with sample data for 3 users, 10 products, and 5 orders.
Support find, insert, update, delete operations.
```

## Performance Characteristics

- **Connection overhead**: Low (simple TCP accept)
- **Parsing overhead**: Low (BSON is efficient)
- **LLM latency**: High (1-3 seconds per query)
- **Throughput**: Limited by LLM API rate limits
- **Concurrent connections**: Unlimited (each spawns tokio task)

## Testing Approach

See `tests/server/mongodb/CLAUDE.md` for E2E testing strategy.

## Future Enhancements

**Phase 2**:
- SCRAM-SHA-256 authentication
- Index definitions (LLM manages)
- Basic aggregation ($match, $project, $group)

**Phase 3**:
- GridFS file storage (in LLM memory)
- Change streams (event-based updates)
- Collation and text search

## Implementation Notes

### Why Manual OP_MSG Parsing?

Unlike MySQL (`opensrv-mysql`) and PostgreSQL (`pgwire`), there's no Rust library that provides server-side MongoDB wire protocol handling. Options considered:

1. **`mongodb` crate**: Client-only library
2. **`bson` crate**: Only handles BSON encoding, not wire protocol
3. **FerretDB**: Written in Go, translates MongoDB → PostgreSQL
4. **Custom implementation**: ✅ Chosen for full control

### Why BSON Crate?

- **Official**: Maintained by MongoDB team
- **Complete**: Full BSON spec support
- **Interop**: Works seamlessly with `mongodb` client crate
- **Serde**: Automatic serialization support

### Connection Lifecycle

```rust
// 1. Accept connection
let (stream, addr) = listener.accept().await?;

// 2. Track in state
app_state.add_connection_to_server(server_id, conn_state).await;

// 3. Spawn handler
tokio::spawn(async move {
    handler.handle_connection(stream).await
});

// 4. Read OP_MSG loop
loop {
    let header = read_exact(16).await?;
    let body = read_exact(message_length - 16).await?;
    let command = parse_op_msg(&body)?;

    // Call LLM
    let actions = call_llm(...).await?;

    // Execute and respond
    for action in actions {
        let response = encode_op_msg_response(...)?;
        writer.write_all(&response).await?;
    }
}
```

## Debugging

### Enable Trace Logging
```bash
RUST_LOG=netget::server::mongodb=trace cargo run
```

### Monitor BSON Documents
```rust
trace!("MongoDB command document: {:?}", command_doc);
trace!("MongoDB response document: {:?}", response_doc);
```

### View Wire Protocol
Use `tcpdump` or Wireshark with MongoDB dissector:
```bash
tcpdump -i lo -w mongodb.pcap port 27017
```

## References

- [MongoDB Wire Protocol Spec](https://www.mongodb.com/docs/manual/reference/mongodb-wire-protocol/)
- [BSON Spec](http://bsonspec.org/)
- [MongoDB OP_MSG](https://github.com/mongodb/mongo/blob/master/src/mongo/rpc/op_msg.h)
- [BSON Rust Crate](https://docs.rs/bson/latest/bson/)
