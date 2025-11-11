# IPP Protocol Implementation

## Overview

IPP (Internet Printing Protocol) server implementing RFC 2910 (Internet Printing Protocol/1.1) over HTTP. The protocol
uses HTTP POST requests to send IPP operations encoded in a binary format.

**Protocol**: IPP/1.1 and IPP/2.0
**Transport**: HTTP/1.1 over TCP
**Port**: 631 (standard), configurable
**Status**: Alpha

## Library Choices

- **hyper** (v1.x) - HTTP server foundation, handles connection management
- **Manual IPP parsing** - Custom parsing of IPP binary format
    - IPP uses custom binary encoding for operations and attributes
    - Version + Operation ID + Request ID + Attribute groups
    - No existing Rust IPP library suitable for LLM control

**Why manual parsing?**

- IPP protocol is relatively simple: 8-byte header + attribute groups
- Full control over response generation for LLM
- Avoids dependency on incomplete IPP libraries

## Architecture Decisions

### HTTP-Based Protocol

IPP runs over HTTP, so we use hyper's HTTP/1 server infrastructure:

- Each connection handled by hyper's service_fn
- IPP requests arrive as HTTP POST with Content-Type: application/ipp
- IPP responses returned as HTTP 200 OK with application/ipp body

### LLM Control Points

The LLM controls:

1. **Printer attributes** - printer-name, printer-state, capabilities
2. **Job handling** - Accept/reject print jobs, assign job IDs
3. **IPP responses** - Status codes, attribute groups

### Operation Parsing

Simple operation ID extraction from first 8 bytes:

- Bytes 0-1: IPP version (0x02 0x00 for v2.0)
- Bytes 2-3: Operation ID (big-endian u16)
- Bytes 4-7: Request ID

Common operation IDs:

- 0x0002: Print-Job
- 0x000B: Get-Printer-Attributes
- 0x0009: Get-Job-Attributes
- 0x000A: Get-Jobs

### Response Format

LLM generates IPP responses via `ipp_response` action:

```json
{
  "type": "ipp_response",
  "status": 200,
  "body": "hex-encoded IPP response data"
}
```

The hex-encoded body contains:

- IPP status code (successful-ok, client-error, etc.)
- Response attributes (printer-uri, job-id, etc.)

## Connection Management

### HTTP Request-Response Cycle

Each IPP operation follows HTTP request-response:

1. Client sends HTTP POST with IPP request body
2. Server parses HTTP headers and body
3. LLM processes operation and generates response
4. Server sends HTTP 200 with IPP response body

### Connection Tracking

Connections tracked in ServerInstance state:

- Connection ID assigned per HTTP connection
- Protocol-specific info: `ProtocolConnectionInfo::Ipp { recent_jobs }`
- Stats: bytes_sent, bytes_received, packets_sent, packets_received
- Status updated on connection close

### Concurrency

Multiple concurrent connections supported:

- Each connection handled in separate tokio task
- hyper manages HTTP/1 multiplexing
- No shared state between connections (stateless HTTP)

## State Management

### Per-Connection State

Minimal state required since IPP is stateless over HTTP:

- Connection ID for tracking
- Recent jobs list (for UI display)

### No Session State

IPP doesn't maintain sessions:

- Each operation is independent
- Job IDs returned to client for future reference
- Printer state managed by LLM (in memory or instructions)

## Limitations

### Not Implemented

- **CUPS extensions** - CUPS-specific IPP extensions not supported
- **IPP/2.x features** - Only basic IPP/1.1 and 2.0 operations
- **Authentication** - No IPP authentication (could be added)
- **Encryption** - No IPP-over-HTTPS support (plain HTTP only)
- **Job persistence** - Jobs not stored, only logged

### Simplified Implementation

- **Attribute parsing** - Minimal attribute parsing, relies on operation ID
- **Response encoding** - LLM must provide properly formatted hex data
- **Status codes** - Limited IPP status code mapping

### Testing Limitations

- Real IPP clients (CUPS) may expect specific attributes
- Some clients require full IPP/1.1 compliance
- Testing mostly uses raw HTTP POST requests

## LLM Integration

### Action-Based Responses

LLM returns actions via structured JSON:

**ipp_response** - Send IPP response:

```json
{
  "type": "ipp_response",
  "status": 200,
  "body": "020000000000000103..."
}
```

**show_message** - Display message in UI:

```json
{
  "type": "show_message",
  "message": "Print job accepted"
}
```

### Event-Based Processing

IPP operations trigger `IPP_REQUEST_EVENT`:

```json
{
  "method": "POST",
  "uri": "/printers/netget",
  "operation": "Print-Job"
}
```

LLM receives:

- HTTP method and URI
- Parsed operation name
- Request context

### Scripting Support

IPP operations can be scripted for repetitive responses:

- Scripting mode generates Python/JS handlers
- Fast responses without LLM calls
- Good for printer attribute queries

## Example Prompts and Responses

### Example 1: Get-Printer-Attributes

**Prompt:**

```
Listen on port 631 via IPP. When clients send Get-Printer-Attributes,
respond with printer-name="NetGet Printer", printer-state="idle".
```

**LLM Response:**

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Received Get-Printer-Attributes request"
    },
    {
      "type": "ipp_response",
      "status": 200,
      "body": "02000000000000010347001200..."
    }
  ]
}
```

### Example 2: Print-Job

**Prompt:**

```
Listen on port 631 via IPP. Accept all print jobs with job-id=1,
job-state="processing".
```

**LLM Response:**

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Accepted print job: test.pdf"
    },
    {
      "type": "ipp_response",
      "status": 200,
      "body": "02000000000000010421000600..."
    }
  ]
}
```

### Example 3: Rejecting Jobs

**Prompt:**

```
Listen on port 631 via IPP. Reject all print jobs from clients,
return client-error-not-authorized.
```

**LLM Response:**

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Rejected print job - not authorized"
    },
    {
      "type": "ipp_response",
      "status": 401,
      "body": "02000502000000010103..."
    }
  ]
}
```

## References

- [RFC 2910: Internet Printing Protocol/1.1](https://tools.ietf.org/html/rfc2910)
- [RFC 8011: Internet Printing Protocol/1.1 (updated)](https://tools.ietf.org/html/rfc8011)
- [IPP Registrations](https://www.pwg.org/ipp/ipp-registrations.xml)
- [CUPS Implementation Guide](https://www.cups.org/doc/spec-ipp.html)
- [hyper HTTP library](https://docs.rs/hyper)

## Logging

### Structured Logging Levels

**TRACE** - Full IPP packet details:

- Complete hex dump of request/response
- Pretty-printed IPP structures
- Attribute group parsing

**DEBUG** - IPP operation summaries:

- Operation name and request ID
- HTTP status and response size
- "IPP GET-PRINTER-ATTRIBUTES (123 bytes)"

**INFO** - High-level events:

- Connection open/close
- "IPP connection from 192.168.1.100"
- "IPP connection closed"

**WARN** - Non-fatal issues:

- Malformed IPP requests
- Unknown operation IDs

**ERROR** - Critical failures:

- HTTP parsing errors
- LLM communication failures

All logs use dual logging pattern (tracing macros + status_tx).
