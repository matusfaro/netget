# XML-RPC Client Implementation

## Overview

XML-RPC client implementation for calling remote procedure calls over HTTP using XML encoding.

## Library Choices

### xmlrpc crate (v0.15.1)

**Why xmlrpc:**
- Mature, actively maintained library
- Simple API for method calls: `Request::new("method").arg(param1).arg(param2)`
- Built on reqwest for HTTP transport
- Full XML-RPC type support (int, bool, string, double, datetime, base64, array, struct, nil)
- Synchronous API (wrapped in tokio::task::spawn_blocking for async compatibility)

**Alternatives considered:**
- `dxr_client`: More modern (Dec 2024), but repository archived and moved to Codeberg
- `xml-rpc`: Less mature alternative

## Architecture

### Connection Model

XML-RPC is **connectionless** - each method call is an independent HTTP POST request:

1. **Initialization**: Store server URL in client protocol_data
2. **Method Calls**: On-demand HTTP requests with XML-encoded parameters
3. **No persistent connection**: Unlike TCP/SSH, no socket kept open
4. **Stateless protocol**: Each call is independent (though LLM has memory)

### LLM Integration Flow

```
User Instruction
    ↓
LLM generates action: call_xmlrpc_method
    ↓
Execute action → Build XML-RPC Request
    ↓
HTTP POST to server (blocking in spawn_blocking)
    ↓
Receive XML response → Parse to xmlrpc::Value
    ↓
Convert to JSON → Event: xmlrpc_response_received
    ↓
Call LLM with result
    ↓
LLM decides next action (another call, disconnect, etc.)
```

### Data Conversion

**JSON → XML-RPC Value:**
- `null` → `String("")` (XML-RPC has no null in spec, though extensions exist)
- `bool` → `Bool`
- `number` (integer) → `Int` (i32) or `Int64` (i64)
- `number` (float) → `Double`
- `string` → `String`
- `array` → `Array`
- `object` → `Struct`

**XML-RPC Value → JSON:**
- `Int`, `Int64` → `number`
- `Bool` → `bool`
- `String` → `string`
- `Double` → `number`
- `DateTime` → `string` (ISO 8601)
- `Base64` → `string` (base64-encoded)
- `Array` → `array`
- `Struct` → `object`
- `Nil` → `null`

## LLM Control Points

### Async Actions (User-triggered)

**call_xmlrpc_method**
- Method name: Any string
- Parameters: Array of mixed types (LLM constructs parameter list)
- Example: `{"type": "call_xmlrpc_method", "method_name": "system.listMethods", "params": []}`

**disconnect**
- No parameters
- Stops client monitoring task

### Sync Actions (Network event responses)

**call_xmlrpc_method**
- Same as async version
- Triggered after receiving a response
- Allows chained method calls

### Events

**xmlrpc_connected**
- Triggered on initialization
- Provides server URL
- LLM can make initial method call

**xmlrpc_response_received**
- Triggered after each method call
- Provides method_name and result (or fault)
- LLM analyzes response and decides next action

## Implementation Details

### Blocking API Wrapper

xmlrpc crate is synchronous, so we use:
```rust
tokio::task::spawn_blocking(move || {
    request.call_url(&server_url)
}).await
```

This runs the blocking HTTP call on a dedicated thread pool without blocking the async runtime.

### Error Handling

XML-RPC supports structured faults:
```xml
<methodResponse>
  <fault>
    <value>
      <struct>
        <member>
          <name>faultCode</name>
          <value><int>4</int></value>
        </member>
        <member>
          <name>faultString</name>
          <value><string>Too many parameters.</string></value>
        </member>
      </struct>
    </value>
  </fault>
</methodResponse>
```

LLM receives fault as:
```json
{
  "method_name": "foo",
  "fault": {
    "code": 4,
    "message": "Too many parameters."
  }
}
```

## Limitations

1. **Synchronous HTTP**: Each method call blocks a thread from the tokio blocking pool
2. **No streaming**: Cannot handle long-running methods with progress updates
3. **Type limitations**:
   - Null handling varies (converted to empty string for compatibility)
   - Binary data must use Base64 encoding
   - No native support for complex types beyond struct/array
4. **No authentication**: xmlrpc crate doesn't provide built-in HTTP auth (would need custom transport)
5. **No TLS configuration**: Uses reqwest defaults (could add custom transport for cert validation control)

## Common Use Cases

### System Introspection
```json
{
  "type": "call_xmlrpc_method",
  "method_name": "system.listMethods",
  "params": []
}
```

### Simple Calculator
```json
{
  "type": "call_xmlrpc_method",
  "method_name": "examples.getStateName",
  "params": [41]
}
```

### Complex Parameters
```json
{
  "type": "call_xmlrpc_method",
  "method_name": "blogger.newPost",
  "params": [
    "appkey123",
    "blogid456",
    "username",
    "password",
    {
      "title": "My Post",
      "description": "Post content here"
    },
    true
  ]
}
```

## Testing Servers

**Public XML-RPC test servers:**
- http://betty.userland.com/RPC2 (historical test server)
- http://phpxmlrpc.sourceforge.net/server.php (validator)

**LLM can test with:**
- `system.listMethods` - List available methods
- `system.methodSignature` - Get method signature
- `system.methodHelp` - Get method documentation

## Security Considerations

- No authentication mechanism in current implementation
- Sends data in plaintext (use HTTPS URLs for encryption)
- LLM can call any method on the server (instruction should specify allowed methods)
- No rate limiting (LLM makes calls as fast as it decides)

## Future Enhancements

1. **Custom Transport**: Add HTTP Basic Auth support
2. **TLS Configuration**: Certificate validation control
3. **Timeout Control**: Per-call timeout configuration
4. **Connection Pooling**: Reuse HTTP connections (reqwest::Client stored in protocol_data)
5. **Multicall Extension**: Batch multiple calls in one request (system.multicall)
