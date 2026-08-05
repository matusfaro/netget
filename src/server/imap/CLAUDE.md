# IMAP Protocol Implementation

## Overview

IMAP4rev1 (Internet Message Access Protocol) server covering enough of RFC 3501 for real
clients (`imaplib`, Thunderbird-style flows) to log in, select a mailbox and fetch messages.
**Plain TCP on port 143 only** - there is no IMAPS and no STARTTLS.

## Library Choices

- **Manual Implementation** - No external IMAP library used
- **Manual parsing** - Line-based command parsing without imap-codec. Commands are split into
  at most three fields (`splitn(3, ' ')`); quoting, literals (`{n}`) and continuation requests
  are not modelled, and whatever the split does not understand is handed to the model as text.
- Raw TCP handling with tokio for async I/O
- Chosen for maximum LLM control over mailbox and message storage

An `ImapServer::spawn_with_tls` advertising IMAPS on 993 used to live in `mod.rs`. Nothing
called it and it could not have worked - it passed a concatenated PEM string to
`native_tls::Identity::from_pkcs12`, which only accepts DER PKCS#12 - so it was deleted rather
than left looking implemented.

## Architecture Decisions

### Session State Machine

IMAP maintains explicit session state transitions:

- **NotAuthenticated** - Initial state after connection
- **Authenticated** - After successful LOGIN
- **Selected** - After SELECT/EXAMINE mailbox
- **Logout** - Terminal state

State stored in `ProtocolConnectionInfo::Imap`:

```rust
ImapSessionState: NotAuthenticated | Authenticated | Selected | Logout
authenticated_user: Option<String>
selected_mailbox: Option<String>
mailbox_read_only: bool
```

### LLM Integration

- **Three event types**, each advertising its own action list. That list - not
  `get_sync_actions()` - is what `call_llm` offers the model, so an action missing from it is
  unreachable over the LLM path even though `execute_action` handles it:
    - `IMAP_CONNECTION_EVENT` - Initial greeting. Offers `send_imap_greeting`,
      `send_imap_untagged` and `send_imap_response`, because an IMAP banner is an untagged line
      and all three can produce one.
    - `IMAP_AUTH_EVENT` - LOGIN command (special handling). Deliberately narrow:
      `send_imap_response` and `close_connection`.
    - `IMAP_COMMAND_EVENT` - All other commands (CAPABILITY, SELECT, FETCH, etc.)
- **Action-based responses** - LLM returns JSON actions for all protocol interactions
- **Tagged responses** - IMAP uses command tags (A001, A002) for request/response correlation
- **Untagged responses** - Server data (EXISTS, RECENT, FLAGS) sent before tagged completion

### Connection Management

- Connections tracked in `AppState` with full statistics
- Each connection spawns independent async task
- Write operations use `Arc<Mutex<WriteHalf>>` for safe concurrent access
- Read operations use `BufReader` for line-based parsing

### Response Actions

The LLM controls IMAP responses through these actions:

- `send_imap_greeting` - Initial `* OK` greeting with capabilities
- `send_imap_response` - Tagged completion (`tag` + `status`), or one verbatim line via
  `response` (used for untagged banners)
- `send_imap_untagged` - Untagged informational responses
- `send_imap_capability` - CAPABILITY response
- `send_imap_list` - LIST response with mailbox list
- `send_imap_status` - STATUS response with mailbox info
- `send_imap_select` - the full untagged block for SELECT/EXAMINE (EXISTS, RECENT, UIDVALIDITY,
  UIDNEXT, FLAGS, PERMANENTFLAGS) in one action
- `send_imap_fetch` - FETCH response with message data
- `send_imap_search` - SEARCH response with message IDs
- `send_imap_exists` - EXISTS count
- `send_imap_recent` - RECENT count
- `send_imap_flags` - FLAGS list
- `send_imap_expunge` - EXPUNGE notification
- `wait_for_more` - Accumulate multi-line commands (APPEND)
- `close_connection` - Terminate session

### Command Parsing

Simple 3-field parser splits IMAP commands:

```rust
(tag, command, args) = parse_imap_command(line)
// Example: "A001 LOGIN alice secret"
// → ("A001", "LOGIN", "alice secret")
```

LOGIN command has special handling for authentication event.

## State Management

IMAP session state tracked in `AppState`:

- `ImapSessionState` - Current session state
- `authenticated_user` - Username after successful LOGIN
- `selected_mailbox` - Currently selected mailbox (INBOX, Sent, etc.)
- `mailbox_read_only` - Whether mailbox is read-only (EXAMINE vs SELECT)

State transitions:

- `LOGIN OK` → `Authenticated`
- `SELECT/EXAMINE OK` → `Selected`
- `CLOSE` → `Authenticated`
- `LOGOUT` → `Logout`

## Limitations

- **No message persistence** - LLM manages mailbox data in memory/context
- **No TLS** - plain TCP only; neither IMAPS nor STARTTLS
- **No SASL AUTH** - Only LOGIN authentication supported
- **No IMAP extensions** - IDLE, CONDSTORE, QRESYNC not implemented
- **No mailbox subscriptions** - SUBSCRIBE/UNSUBSCRIBE not tracked
- **No server-side search** - SEARCH criteria interpreted by LLM
- **No message flags persistence** - Flags not persisted across sessions
- **State transitions are unconditional** - `update_session_state` moves the session to
  `Selected` on any `SELECT`/`EXAMINE` line, whatever the model answered, so a SELECT the model
  refused with `NO` still leaves the session reported as Selected.

## Examples

### Example LLM Prompt

```
listen on port 143 via imap. Support IMAP4rev1, IDLE, NAMESPACE capabilities.
Allow LOGIN for 'alice' with password 'secret'.
INBOX has 5 messages, 2 recent.
For FETCH 1, return message with From: test@example.com, Subject: Test.
```

### Example LLM Response (Greeting)

```json
{
  "actions": [
    {
      "type": "send_imap_greeting",
      "hostname": "mail.example.com",
      "capabilities": ["IMAP4rev1", "IDLE", "NAMESPACE"]
    }
  ]
}
```

### Example LLM Response (LOGIN Success)

```json
{
  "actions": [
    {
      "type": "send_imap_response",
      "tag": "A001",
      "status": "OK",
      "message": "LOGIN completed"
    }
  ]
}
```

### Example LLM Response (SELECT)

```json
{
  "actions": [
    {
      "type": "send_imap_exists",
      "count": 5
    },
    {
      "type": "send_imap_recent",
      "count": 2
    },
    {
      "type": "send_imap_flags",
      "flags": ["\\Seen", "\\Answered", "\\Flagged", "\\Deleted", "\\Draft"]
    },
    {
      "type": "send_imap_response",
      "tag": "A002",
      "status": "OK",
      "code": "READ-WRITE",
      "message": "SELECT completed"
    }
  ]
}
```

### Example LLM Response (FETCH)

```json
{
  "actions": [
    {
      "type": "send_imap_fetch",
      "sequence": 1,
      "data": {
        "FLAGS": ["\\Seen"],
        "UID": 1001,
        "RFC822.SIZE": 2048,
        "BODY[]": "From: test@example.com\r\nSubject: Test\r\n\r\nHello World"
      }
    },
    {
      "type": "send_imap_response",
      "tag": "A004",
      "status": "OK",
      "message": "FETCH completed"
    }
  ]
}
```

## References

- RFC 3501 - IMAP4rev1 Protocol Specification
- RFC 4551 - IMAP Extension for Conditional STORE (CONDSTORE)
