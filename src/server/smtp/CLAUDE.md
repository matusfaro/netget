# SMTP Protocol Implementation

## Overview
SMTP (Simple Mail Transfer Protocol) server implementing basic RFC 5321 functionality for sending and receiving email messages.

## Library Choices
- **Manual Implementation** - No external SMTP library used
- Raw TCP handling with tokio for async I/O
- Line-based protocol parsing using `AsyncBufReadExt`
- **TLS Support** - rustls and tokio-rustls for optional SMTPS (implicit TLS)
- **Certificate Generation** - rcgen for self-signed certificates
- Chosen for maximum flexibility and LLM control over protocol behavior

## Architecture Decisions

### Connection Handling
- **Single Event Type**: `SMTP_COMMAND_EVENT` handles all SMTP commands
- Commands are parsed line-by-line from the TCP stream
- Each command triggers an LLM call for action-based response
- Connection ID tracked for multi-connection support

### LLM Integration
- **Action-based responses** - LLM returns JSON actions for all protocol interactions
- **Greeting on connect** - Special `CONNECTION_ESTABLISHED` command triggers initial 220 greeting
- **No state machine** - SMTP state (HELO, MAIL FROM, RCPT TO, DATA) managed implicitly by LLM
- **Protocol-aware actions** - Dedicated actions for SMTP responses (greeting, OK, EHLO, error, etc.)

### Session Management
- No persistent session state beyond connection tracking
- SMTP transaction state (MAIL FROM → RCPT TO → DATA) determined by LLM logic
- Each command is stateless from NetGet's perspective

### Response Actions
The LLM controls SMTP responses through these actions:
- `send_smtp_greeting` - 220 greeting banner
- `send_smtp_ok` - 250 OK responses
- `send_smtp_ehlo` - 250-hostname with extensions
- `send_smtp_start_data` - 354 start data input
- `send_smtp_error` - 4xx/5xx error responses
- `send_smtp_quit` - 221 closing connection
- `send_smtp_message` - Custom SMTP response
- `wait_for_more` - Accumulate multi-line DATA
- `close_connection` - Terminate session

## Connection Management
- Connections tracked in `AppState` (bytes sent/received, packet counts)
- Each connection spawns independent async task
- Write operations use `AsyncWriteExt` directly on split write half
- Read operations use `BufReader` for line-based parsing

## State Management
- **No protocol-specific state** - SMTP doesn't use `ProtocolConnectionInfo::Smtp`
- Connection lifecycle managed by tokio tasks
- Session state implicit in LLM conversation context

## TLS Support (SMTPS)
- **Implicit TLS** - SMTPS on port 465 (connection starts with TLS handshake)
- **Configurable** - Enable via `enable_tls: true` in open_server action options
- **Self-signed certificates** - Auto-generated using rcgen
- **Customizable certificates** - LLM can specify CN, SAN, validity, organization
- **Backward compatible** - TLS is optional, defaults to plain SMTP

### Enabling SMTPS
Use the `open_server` action with TLS options:
```json
{
  "type": "open_server",
  "protocol": "smtp",
  "port": 465,
  "options": {
    "enable_tls": true,
    "tls_common_name": "mail.example.com",
    "tls_san_dns_names": ["mail.example.com", "localhost"],
    "tls_validity_days": 365
  }
}
```

## Limitations
- **No STARTTLS support** - Only implicit TLS (SMTPS) is supported, not STARTTLS upgrade
- **No SMTP AUTH** - Authentication not implemented
- **No message persistence** - Messages logged but not stored
- **No PIPELINING** - Commands processed sequentially
- **No size validation** - MESSAGE_SIZE limits not enforced
- **No relay control** - Accepts all MAIL FROM/RCPT TO

## Examples

### Example LLM Prompt (Plain SMTP)
```
listen on port 25 via smtp. Send greeting '220 mail.example.com ESMTP'.
Respond to EHLO with '250 8BITMIME'.
Accept all MAIL FROM and RCPT TO commands with '250 OK'.
For DATA, respond with '354 Start mail input' then '250 Message accepted'.
```

### Example LLM Prompt (SMTPS with TLS)
```
listen on port 465 via smtp with TLS enabled. Send greeting '220 secure.mail.example.com ESMTPS'.
Respond to EHLO with '250 8BITMIME'.
Accept all MAIL FROM and RCPT TO commands with '250 OK'.
For DATA, respond with '354 Start mail input' then '250 Message accepted'.
```

### Example LLM Response (Greeting)
```json
{
  "actions": [
    {
      "type": "send_smtp_greeting",
      "hostname": "mail.example.com",
      "message": "ESMTP Service Ready"
    }
  ]
}
```

### Example LLM Response (EHLO)
```json
{
  "actions": [
    {
      "type": "send_smtp_ehlo",
      "hostname": "mail.example.com",
      "extensions": ["8BITMIME", "SIZE 10240000"]
    }
  ]
}
```

### Example LLM Response (Error)
```json
{
  "actions": [
    {
      "type": "send_smtp_error",
      "code": 550,
      "message": "Mailbox unavailable"
    }
  ]
}
```

## References
- RFC 5321 - Simple Mail Transfer Protocol
- RFC 5322 - Internet Message Format
- tokio documentation: https://docs.rs/tokio
