# XML-RPC Server Implementation

## Overview

XML-RPC server over HTTP POST where the LLM controls all RPC method execution. Supports standard XML-RPC types (int, boolean, string, double, dateTime, base64, array, struct), introspection methods, and fault responses.

## Protocol Version

- **XML-RPC**: Specification from http://xmlrpc.com/spec.md
- **Transport**: HTTP/1.1 POST with XML request/response bodies
- **Content-Type**: `text/xml`

## Library Choices

### Core Dependencies
- **quick-xml** v0.36 - Fast XML parsing and writing
  - Chosen for: Performance, low memory usage, streaming support
  - Used for: Parsing `<methodCall>` and generating `<methodResponse>`
- **hyper** v1 - HTTP/1.1 server
- **base64** - Base64 encoding for `<base64>` type

### Why Manual XML Parsing?
- XML-RPC has simple, well-defined structure
- quick-xml provides full control over parsing
- No comprehensive XML-RPC server library for Rust exists

## Architecture Decisions

### LLM Control Points
**Complete Method Implementation** - LLM handles all method logic:
1. **Parse**: Extract `methodName` and `params` from XML
2. **LLM Call**: Send method details to LLM as JSON event
3. **Generate**: Convert LLM response to XML `<methodResponse>`

**Action-Based Responses**:
```json
{
  "actions": [
    {
      "type": "xmlrpc_success",
      "value": {"type": "int", "value": 8}
    }
  ]
}
```

Or for faults:
```json
{
  "actions": [
    {
      "type": "xmlrpc_fault",
      "faultCode": -32601,
      "faultString": "Method not found"
    }
  ]
}
```

### Type System
**XML-RPC Types Supported**:
- `<int>` / `<i4>` - 32-bit integer
- `<i8>` - 64-bit integer (extension)
- `<boolean>` - 0 or 1
- `<string>` - UTF-8 text
- `<double>` - Floating point
- `<dateTime.iso8601>` - ISO 8601 timestamp
- `<base64>` - Binary data (base64 encoded)
- `<array>` - Ordered list
- `<struct>` - Key-value map
- `<nil>` - Null value (extension)

**Conversion to JSON for LLM**:
- XML types converted to JSON for LLM interpretation
- LLM returns JSON, converted back to XML

### Introspection Support
**Standard Methods** (LLM can implement):
- `system.listMethods` - List available methods
- `system.methodHelp` - Get method documentation
- `system.methodSignature` - Get method signature
- `system.multicall` - Batch method calls (extension)

### Connection Management
- Each HTTP connection spawned as tokio task
- Connections tracked in `ProtocolConnectionInfo::XmlRpc` with `recent_methods` Vec
- HTTP/1.1 handled by hyper

## Limitations

### Not Implemented
- **Transport negotiation** - Only HTTP POST supported
- **Authentication** - No auth layer
- **Compression** - No gzip support
- **Custom extensions** - Only standard + nil/i8 extensions

### Type Limitations
- **Date parsing** - Accepts ISO 8601 strings, no validation
- **Number precision** - double may lose precision
- **Struct key ordering** - Not preserved

### Performance
- **XML parsing overhead** - Slower than JSON
- **Manual type conversion** - Extra overhead vs. binary protocols

## Example Prompts and Responses

### Startup
```
listen on port 8080 via xmlrpc stack.

Implement these methods:
- add(a int, b int) -> int: Return sum of a and b
- greet(name string) -> string: Return "Hello, {name}!"
- system.listMethods() -> array: Return ["add", "greet", "system.listMethods"]

For unknown methods, return fault code -32601 with message "Method not found".
```

### Network Event (Method Call)
**Received XML**:
```xml
<?xml version="1.0"?>
<methodCall>
  <methodName>add</methodName>
  <params>
    <param><value><int>5</int></value></param>
    <param><value><int>3</int></value></param>
  </params>
</methodCall>
```

**Converted to JSON for LLM**:
```json
{
  "event_type": "xmlrpc_method_call",
  "method_name": "add",
  "params": [5, 3]
}
```

**LLM Response**:
```json
{
  "actions": [
    {
      "type": "xmlrpc_success",
      "value": 8
    }
  ]
}
```

**Sent to Client**:
```xml
<?xml version="1.0"?>
<methodResponse>
  <params>
    <param><value><int>8</int></value></param>
  </params>
</methodResponse>
```

### Fault Response
**LLM Response**:
```json
{
  "actions": [
    {
      "type": "xmlrpc_fault",
      "faultCode": -32601,
      "faultString": "Method not found"
    }
  ]
}
```

**Sent to Client**:
```xml
<?xml version="1.0"?>
<methodResponse>
  <fault>
    <value>
      <struct>
        <member>
          <name>faultCode</name>
          <value><int>-32601</int></value>
        </member>
        <member>
          <name>faultString</name>
          <value><string>Method not found</string></value>
        </member>
      </struct>
    </value>
  </fault>
</methodResponse>
```

## References

- [XML-RPC Specification](http://xmlrpc.com/spec.md)
- [quick-xml Documentation](https://docs.rs/quick-xml/)

## Key Design Principles

1. **Spec Compliance** - Follows XML-RPC specification exactly
2. **Type Safety** - Validates and converts all XML-RPC types
3. **Introspection** - Supports standard introspection methods
4. **Fault Handling** - Uses XML-RPC fault structure for errors
5. **LLM Control** - All method logic implemented by LLM
