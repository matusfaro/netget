# XMPP Client Implementation

## Overview

XMPP (Extensible Messaging and Presence Protocol), formerly known as Jabber, is an open-standard instant messaging protocol. This client implementation allows NetGet to connect to XMPP servers, send/receive messages, manage presence, and interact with other XMPP clients.

**Complexity:** Hard (🟠)
**Status:** Incomplete - Requires tokio-xmpp 5.0 API updates

## ⚠️ Current Implementation Status

**This implementation does NOT currently compile.** It was scaffolded based on tokio-xmpp 3.x API but tokio-xmpp 5.0 has significant breaking changes.

###What's Complete:
- ✅ Protocol structure and trait implementations
- ✅ Event type definitions
- ✅ Action definitions
- ✅ Test infrastructure
- ✅ Documentation
- ✅ Feature flags and dependencies

### What Needs Work:
- ❌ Update to tokio-xmpp 5.0 API (see "Required API Updates" below)
- ❌ Fix Jid type mismatches
- ❌ Replace removed methods
- ❌ Update stanza handling
- ❌ Use re-exported xmpp_parsers types

## Library Choice

**Primary:** `tokio-xmpp` v5.0 + `xmpp-parsers`

### Why tokio-xmpp?
- **Async/Await Support:** Full tokio integration for non-blocking I/O
- **Modern Design:** Built for async Rust with proper error handling
- **Active Development:** Well-maintained with recent updates
- **XMPP Compliance:** Supports core XMPP RFCs (RFC 6120, 6121, 6122)
- **Parser Integration:** Works with `xmpp-parsers` for stanza construction

### Alternatives Considered
- `xmpp-rs`: Less mature, fewer features
- Raw XML parsing: Too complex, reinventing the wheel

## Architecture

### Connection Flow

```
1. Parse JID and password from remote_addr or startup params
   Format: "user@domain@password" or via startup params

2. Create tokio-xmpp Client with JID and password

3. Authenticate via SASL (handled by library)

4. Call LLM with "xmpp_connected" event

5. Spawn event loop to process incoming stanzas

6. State machine: Idle → Processing → Accumulating
   (prevents concurrent LLM calls)
```

### State Management

**Connection State:**
- **Idle:** Ready to process events
- **Processing:** LLM is currently processing an event
- **Accumulating:** Queuing events while LLM is busy

**Per-Client Data:**
- `state`: Current connection state
- `queued_events`: Events queued during Processing state
- `memory`: LLM conversation memory

**Stored in AppState:**
- `jid`: Connected Jabber ID
- XMPP client writer (for sending stanzas)

### XMPP Features Implemented

**✅ Implemented:**
1. **Authentication:** SASL authentication via tokio-xmpp
2. **Messages:** Send and receive chat messages
3. **Presence:** Send presence updates (away, chat, dnd, xa), receive presence from contacts
4. **Auto-reconnect:** Handled by tokio-xmpp

**⚠️ Partially Implemented:**
5. **IQ Stanzas:** Received but not yet processed (TODO)

**❌ Not Implemented:**
6. **Roster Management:** Add/remove contacts (future enhancement)
7. **Multi-User Chat (MUC):** Join/leave chatrooms (future enhancement)
8. **File Transfer:** XEP-0096/XEP-0234 (complex, low priority)
9. **Service Discovery:** XEP-0030 (future enhancement)
10. **Message Receipts:** XEP-0184 (future enhancement)

## LLM Integration

### Events Sent to LLM

1. **xmpp_connected**
   - Triggered: After successful authentication
   - Parameters: `jid` (connected Jabber ID)
   - LLM Action: Send initial presence, greet contacts, etc.

2. **xmpp_message_received**
   - Triggered: When receiving a message from another user
   - Parameters: `from`, `to`, `body`, `message_type`
   - LLM Action: Respond to message, log, ignore, etc.

3. **xmpp_presence_received**
   - Triggered: When receiving presence updates from contacts
   - Parameters: `from`, `presence_type`, `show`, `status`
   - LLM Action: Acknowledge, send message, update roster, etc.

### Actions Available to LLM

**Async Actions (user-triggered):**
- `send_message(to, body)` - Send message to a JID
- `send_presence(show?, status?)` - Update presence
- `disconnect()` - Disconnect from server

**Sync Actions (event-triggered):**
- `send_message(to, body)` - Reply to received message
- `wait_for_more()` - Accumulate more events before responding

### Action Execution

Actions return `ClientActionResult::Custom` with structured data:

```rust
ClientActionResult::Custom {
    name: "send_message",
    data: json!({
        "to": "friend@example.com",
        "body": "Hello!"
    })
}
```

The client handler parses the custom action and executes the corresponding XMPP stanza send.

## Message Flow Examples

### Example 1: Auto-Reply Bot

**Instruction:** "Auto-reply to all messages with 'I'm busy'"

**Flow:**
1. User sends message → `xmpp_message_received` event
2. LLM receives: `{"from": "alice@example.com", "body": "Hi there!"}`
3. LLM action: `send_message(to: "alice@example.com", body: "I'm busy")`
4. Client sends XMPP message stanza

### Example 2: Presence Monitor

**Instruction:** "Log when contacts come online or go offline"

**Flow:**
1. Contact changes presence → `xmpp_presence_received` event
2. LLM receives: `{"from": "bob@example.com", "presence_type": "Available", "show": "chat"}`
3. LLM action: Log to memory (no stanza sent)

## Known Limitations

### 1. No Roster Management
**Issue:** Cannot add/remove contacts programmatically
**Workaround:** Manually add contacts using XMPP client before connecting NetGet
**Future:** Implement roster management actions

### 2. IQ Stanzas Not Handled
**Issue:** IQ (Info/Query) stanzas are received but ignored
**Impact:** Cannot respond to service discovery, version queries, etc.
**Future:** Parse and respond to common IQ queries

### 3. No MUC (Multi-User Chat)
**Issue:** Cannot join group chats
**Workaround:** Use direct messages only
**Future:** Implement XEP-0045 for MUC support

### 4. No TLS Configuration
**Issue:** TLS settings are library defaults, no customization
**Impact:** Cannot connect to servers with self-signed certs
**Future:** Expose TLS configuration in startup params

### 5. Password in URL
**Issue:** Password must be in connection string or startup params
**Security:** Not ideal for production use
**Workaround:** Use startup params instead of URL

## Connection String Format

**Option 1: URL Format**
```
user@domain@password
```

Example:
```
alice@example.com@secretpass
```

**Option 2: Startup Parameters (Recommended)**
```bash
open_client xmpp example.com --param jid=alice@example.com --param password=secretpass "Reply to all messages"
```

## Security Considerations

1. **TLS Encryption:** tokio-xmpp uses TLS by default (STARTTLS or direct TLS)
2. **Password Storage:** Passwords stored in AppState protocol_data (in-memory only)
3. **SASL Authentication:** Uses library's SASL implementation (PLAIN, SCRAM)

**⚠️ Warning:** Do not hardcode passwords in prompts or instructions. Use startup params.

## Testing Approach

### Local XMPP Server

**Option 1: Prosody (Recommended)**
```bash
# Install prosody
sudo apt install prosody

# Configure /etc/prosody/prosody.cfg.lua
# Add test user
sudo prosodyctl adduser alice@localhost

# Start server
sudo systemctl start prosody
```

**Option 2: ejabberd**
```bash
# Install ejabberd
sudo apt install ejabberd

# Add test user
sudo ejabberdctl register alice localhost password
```

### Public Test Server

XMPP has public test servers (e.g., `jabber.org`, `404.city`), but use with caution for testing.

### E2E Test Strategy

1. **Setup:** Start local prosody server with two test users
2. **Test 1:** Connect and send presence
3. **Test 2:** Send message to another JID
4. **Test 3:** Receive message and auto-reply
5. **Cleanup:** Disconnect

**LLM Call Budget:** < 10 calls
- 1 call for connect
- 3 calls for message send/receive
- 2 calls for presence updates

## Dependencies

```toml
tokio-xmpp = "5.0"
xmpp-parsers = "0.20"
```

**Transitive Dependencies:**
- `tokio-tls` (TLS support)
- `minidom` (XML DOM)
- `trust-dns-resolver` (SRV record lookups)

## Required API Updates for tokio-xmpp 5.0

The following changes are needed to make this implementation compile with tokio-xmpp 5.0:

### 1. JID Type Mismatch
**Issue:** `xmpp_parsers::Jid` != `tokio_xmpp::Jid`
**Fix:** Use `tokio_xmpp::Jid` throughout, as tokio-xmpp 5.0 re-exports xmpp_parsers types

```rust
// Instead of:
use xmpp_parsers::Jid;

// Use:
use tokio_xmpp::Jid;
```

### 2. Client Clone Not Supported
**Issue:** `XmppClient` no longer implements `Clone`
**Fix:** Restructure to use channels or split reading/writing differently

### 3. Missing Methods
**Issues:**
- `set_reconnect()` no longer exists
- `wait_for_event()` replaced with different API
- Stanza doesn't implement `Clone`

**Fix:** Review tokio-xmpp 5.0 API and use:
- Event stream API (likely `futures::Stream`)
- Remove clone calls on stanzas

### 4. Stanza Conversion
**Issue:** `message.into()` fails for type conversion
**Fix:** Use tokio-xmpp's re-exported types:

```rust
// Instead of:
use xmpp_parsers::message::Message;
let stanza: tokio_xmpp::Stanza = message.into();  // FAILS

// Use:
use tokio_xmpp::xmpp_parsers::message::Message;
let stanza = tokio_xmpp::Stanza::from(message);  // OK
```

### 5. Event Loop Pattern
The event loop needs to be rewritten to match tokio-xmpp 5.0's async stream pattern instead of `wait_for_event()`.

## Future Enhancements

1. **Roster Management:** Add/remove/list contacts
2. **MUC Support:** Join/leave chatrooms
3. **File Transfer:** Send/receive files (XEP-0096, XEP-0234)
4. **Message Receipts:** XEP-0184 for delivery confirmation
5. **Service Discovery:** XEP-0030 for capability negotiation
6. **PubSub:** XEP-0060 for publish-subscribe
7. **Message Archiving:** XEP-0136 for history retrieval
8. **OMEMO Encryption:** End-to-end encryption support

## References

- [RFC 6120](https://www.rfc-editor.org/rfc/rfc6120.html) - XMPP Core
- [RFC 6121](https://www.rfc-editor.org/rfc/rfc6121.html) - XMPP IM
- [RFC 6122](https://www.rfc-editor.org/rfc/rfc6122.html) - XMPP Address Format
- [tokio-xmpp Documentation](https://docs.rs/tokio-xmpp/)
- [xmpp-parsers Documentation](https://docs.rs/xmpp-parsers/)
- [XMPP Extensions (XEPs)](https://xmpp.org/extensions/)
