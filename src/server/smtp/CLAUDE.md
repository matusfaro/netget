# SMTP Protocol Implementation

## Overview

SMTP (Simple Mail Transfer Protocol) server implementing basic RFC 5321 functionality for sending and receiving email
messages.

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
- **DATA is not accumulated** - after `send_smtp_start_data` every body line arrives as its own
  `smtp_command` event, terminated by a line containing only `.`. That is one model call per
  line of the message. Use a script or static event handler for anything that receives real
  mail; the LLM path is only practical for short messages.
- **Protocol-aware actions** - Dedicated actions for SMTP responses (greeting, OK, EHLO, error, etc.)

### Error Handling

A failed handler call is answered with a 4xx, never with silence and never with a 2xx. SMTP
already has the vocabulary for "the backend is unavailable, come back later" (RFC 5321 §4.2.1),
and a sending MTA that gets one requeues the message instead of bouncing it:

| Failure | Reply | Then |
|---|---|---|
| greeting (`CONNECTION_ESTABLISHED`) | `421 4.3.0` (`4.3.2` on overload) | session closes, per RFC 5321 §3.1 |
| any later command | `451 4.3.0` (`4.3.2` on overload) | session stays open |

The enhanced code splits the two cases apart: 4.3.2 ("system not accepting network messages")
is used when `crate::llm::is_overload_error` says the failure was capacity exhaustion, 4.3.0
otherwise. Both are refusals — a failure must never be able to look like acceptance, or an
outage would silently report mail as delivered.

Both are still logged at ERROR on the tracing and status channels. Covered by
`tests/server/smtp/llm_failure_test.rs`.

### Session Management

- No persistent session state beyond connection tracking
- SMTP transaction state (MAIL FROM → RCPT TO → DATA) determined by LLM logic
- Each command is stateless from NetGet's perspective

### Response Actions

The LLM controls SMTP responses through these actions:

- `send_smtp_greeting` - 220 greeting banner
- `send_smtp_ok` - 250 OK responses
- `send_smtp_ehlo` - 250-hostname with extensions. An empty `extensions` array emits the
  single-line form `250 hostname`; emitting `250-hostname` with nothing after it leaves the
  reply unterminated and the client blocks until its own timeout.
- `send_smtp_start_data` - 354 start data input
- `send_smtp_error` - 4xx/5xx error responses
- `send_smtp_quit` - 221 closing connection
- `send_smtp_message` - Custom SMTP response
- `wait_for_more` - Send nothing and read the next line (used during DATA, where SMTP expects
  no per-line reply)
- `close_connection` - Terminate session

## Connection Management

- Connections tracked in `AppState` (bytes sent/received, packet counts): `handle_session`
  calls `add_connection_to_server` and `update_connection_stats` on every read and write, so
  the dashboard's `↓ ↑` counters and `last_activity` are live
- Each connection spawns independent async task
- Write operations go through a shared `Arc<Mutex<WriteHalf>>` (reader task and peer command
  task share it); the guard is dropped before any LLM call
- Read operations use `BufReader` for line-based parsing

### Dashboard injection (`[ message this peer ]` / `[ disconnect this peer ]`)

Every connection registers a peer handle (`server::peer_support`) right after it is tracked and
*before* the greeting event, so a manual `*` rule parking the greeting still leaves the operator
able to reach the connection. `AppState::send_to_peer` runs the action through the same executor
as the LLM path; every wire verb here returns `ActionResult::Output` and `close_connection`
half-closes, so there is no `Custom` gap. The handle is removed on every exit path (EOF, read
error, `close_connection`, refused 421 greeting) through the single cleanup in `handle_session`,
which wraps `run_session`. Test: `tests/server/smtp/peer_inject_test.rs` (zero LLM calls).

## State Management

- **No protocol-specific state** - SMTP doesn't use `ProtocolConnectionInfo::Smtp`
- Connection lifecycle managed by tokio tasks
- Session state implicit in LLM conversation context

## TLS Support (SMTPS)

- **Implicit TLS** - SMTPS on port 465 (connection starts with TLS handshake)
- **Configurable** - Enable via `enable_tls: true` in open_server action options
- **Fails closed** - if the certificate cannot be generated, `spawn` returns an error. It used
  to log and fall back to plain text, handing a caller who asked for SMTPS a cleartext mail
  port that reported itself as Running.
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
- **No message persistence** - Messages logged but not stored; NetGet is not an MTA and never
  delivers or relays anything
- **Privileged default port** - `metadata()` declares `PrivilegedPort(25)`, so `server_startup`
  preflights the bind against `SystemCapabilities` instead of failing with a bare EPERM
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
