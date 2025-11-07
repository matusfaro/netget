# SMTP Client Implementation

## Overview

The SMTP client allows LLM-controlled email sending via SMTP servers. It uses the `lettre` library for robust SMTP protocol support including STARTTLS and authentication.

## Library Choice

**Primary: `lettre` v0.11**
- Mature, well-maintained SMTP client library
- Full STARTTLS support for secure email transmission
- Multiple authentication methods (PLAIN, LOGIN)
- Tokio-based async support
- Excellent error handling and RFC compliance

## Architecture

### Connection Model

SMTP is a **request-based protocol** (like HTTP), not a persistent stream (like Redis/TCP):
- Connection is established when client is opened
- No persistent read loop (emails are sent on-demand)
- Each `send_email` action creates a new SMTP transaction
- Connection info (server address, credentials) stored in client state

### State Management

Client state tracked in `ClientInstance.protocol_data`:
- `smtp_server`: Server hostname
- `remote_addr`: Full server address with port

### LLM Integration

#### Events

1. **`smtp_connected`** - Triggered when client connects to SMTP server
   - Parameters: `smtp_server` (hostname)
   - LLM decides: What email to send, authentication strategy

2. **`smtp_email_sent`** - Triggered after successful email transmission
   - Parameters: `to` (recipients), `subject`, `success` (boolean)
   - LLM decides: Send follow-up emails, update memory

#### Actions

**Async Actions** (user-triggered):
- `send_email` - Send an email via SMTP
  - Parameters: `from`, `to` (array), `subject`, `body`, `username` (optional), `password` (optional), `use_tls` (optional)
  - Returns: `ClientActionResult::Custom` with email data

- `disconnect` - Close SMTP client
  - Returns: `ClientActionResult::Disconnect`

**Sync Actions** (LLM response to events):
- `send_email` - Same as async action, triggered by LLM in response to events

### Email Sending Flow

```
1. User opens SMTP client → smtp_connected event
2. LLM receives event, decides to send email
3. LLM returns send_email action with email details
4. Action executor calls SmtpClient::send_email()
5. Email sent via lettre library
6. smtp_email_sent event triggered
7. LLM processes result, may send more emails
```

## SMTP Features

### Authentication

Supports optional SMTP authentication:
- **Username/Password**: Pass via `username` and `password` parameters
- **Credentials**: Converted to lettre `Credentials` object
- **No auth**: Leave credentials empty for open relays (testing)

### TLS/STARTTLS

- **Enabled by default**: `use_tls: true`
- **STARTTLS**: Upgrades connection to TLS after initial handshake
- **TLS Parameters**: Configurable via lettre's `TlsParameters`
- **Certificate validation**: Enabled by default (can be disabled for testing)

### Email Composition

- **Simple emails**: Plain text body only (no HTML in initial implementation)
- **Multiple recipients**: `to` field accepts array of addresses
- **Required fields**: `from`, `to`, `subject`, `body`
- **Future**: Attachments, HTML bodies, CC/BCC (not yet implemented)

## Implementation Details

### Startup

```rust
SmtpClient::connect_with_llm_actions(
    remote_addr,      // e.g., "smtp.example.com:587"
    llm_client,
    app_state,
    status_tx,
    client_id,
)
```

- Parses server address
- Stores connection info in client state
- Calls LLM with `smtp_connected` event
- Spawns monitoring task for client lifecycle

### Sending Email

```rust
SmtpClient::send_email(
    client_id,
    from,
    to,             // Vec<String>
    subject,
    body,
    username,       // Option<String>
    password,       // Option<String>
    use_tls,        // bool
    app_state,
    llm_client,
    status_tx,
)
```

- Retrieves SMTP server from client state
- Builds email message using lettre's `Message` builder
- Configures SMTP transport with TLS and auth
- Sends email in blocking task (lettre is sync)
- Calls LLM with `smtp_email_sent` event

### Action Dispatch Integration

**Status**: Async action dispatching for clients is in progress framework-wide

The SMTP client returns `ClientActionResult::Custom` with name `"smtp_send_email"` and email data. This needs to be handled by a central client action dispatcher (similar to server actions) to call `SmtpClient::send_email()`.

**Current State**:
- Action structure defined ✓
- Event system integrated ✓
- Action dispatcher integration: TODO (framework-wide)

## Limitations

1. **Plain text only**: No HTML email support yet
2. **No attachments**: File attachments not implemented
3. **No CC/BCC**: Only direct `to` recipients
4. **Blocking send**: Email sending uses `spawn_blocking` (lettre is sync)
5. **No SMTP pipelining**: One email at a time
6. **Action dispatch**: Async user actions need framework integration

## Example Prompts

```
"Connect to smtp.gmail.com:587 and send a test email to user@example.com"

"Send an email from sender@example.com to recipient@example.com with subject 'Test' and body 'Hello from NetGet SMTP client'"

"Connect to localhost:25 and send an email without authentication"
```

## Testing Strategy

See `tests/client/smtp/CLAUDE.md` for:
- E2E test approach
- Local SMTP server setup (mailhog, fakesmtp)
- LLM call budget
- Expected runtime

## Future Enhancements

1. **HTML emails**: Add HTML body support
2. **Attachments**: File attachment support via lettre
3. **CC/BCC**: Additional recipient fields
4. **Custom headers**: X-* headers for tracking
5. **Template support**: Email template rendering
6. **Async lettre**: When lettre adds full async support
7. **DKIM signatures**: Email authentication
8. **Bounce handling**: Parse SMTP error responses
