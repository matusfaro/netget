# Syslog Protocol Implementation

## Overview

Syslog server implementing RFC 3164 (BSD syslog) and RFC 5424 (modern syslog) using the syslog_loose library. Provides
log aggregation and analysis where the LLM controls message filtering, storage, forwarding, and alerting. Uses UDP
transport for message reception.

**Status**: Experimental (Network Monitoring Protocol)
**RFC**: RFC 3164 (BSD syslog), RFC 5424 (Syslog Protocol)
**Port**: 514 (standard syslog), 601 (alternative)

## Library Choices

### Core Syslog Implementation

- **syslog_loose v0.22** - Lenient syslog message parser
    - Parses both RFC 3164 (BSD syslog) and RFC 5424 (modern syslog)
    - Handles malformed messages gracefully
    - Extracts: facility, severity, timestamp, hostname, appname, procid, message
    - No dependencies on specific datetime libraries

**Rationale**: syslog_loose is the most lenient parser, accepting both standard and non-standard syslog messages.
Real-world syslog implementations vary widely, so a strict parser would reject many valid messages. The library
automatically detects format (RFC 3164 vs 5424) and extracts fields accordingly.

## Architecture Decisions

### 1. UDP One-Way Protocol

Syslog uses UDP (stateless, no response):

- Each message is independent
- No acknowledgment or response to sender
- Client may send multiple messages rapidly
- Server only receives and processes (no reply)

**Connection Tracking**:

- "Connection" = recent peer address that sent message
- Tracked in `ProtocolConnectionInfo::Syslog` with timestamp
- Used for UI display only (not protocol requirement)

### 2. LLM Control Point

Single integration point:

**Syslog Messages**:

- LLM receives `syslog_message` event with facility, severity, timestamp, hostname, appname, message
- Returns actions: `store_syslog_message`, `forward_syslog`, `ignore_syslog_message`
- Server performs actions (store in memory, forward to another server, drop, etc.)

**No Response to Client**:

- Syslog is one-way (client → server only)
- No acknowledgment sent to client
- LLM cannot send data back to sender

### 3. Message Format Parsing

LLM receives structured message data, not raw format:

**Facility** (source of message):

- kernel, user, mail, daemon, auth, syslog, lpr, news, uucp, cron, authpriv, ftp
- local0-local7 (custom applications)

**Severity** (importance level):

- emergency (0) - system unusable
- alert (1) - immediate action required
- critical (2) - critical condition
- error (3) - error condition
- warning (4) - warning condition
- notice (5) - normal but significant
- info (6) - informational
- debug (7) - debug messages

**Priority Encoding**: `Priority = Facility * 8 + Severity`

- Example: `<34>` = user.critical (1 * 8 + 2 = 10... wait, 34 / 8 = 4 remainder 2, so auth.critical)

**Message Components**:

- **Timestamp**: When message was generated (RFC 3339 for 5424, MMM DD HH:MM:SS for 3164)
- **Hostname**: Source device hostname or IP
- **Appname**: Application or process name
- **Procid**: Process ID (optional)
- **Message**: Actual log message text

### 4. RFC 3164 vs RFC 5424

Parser supports both formats automatically:

**RFC 3164 (BSD syslog)** - Legacy format:

```
<34>Oct 11 22:14:15 mymachine su: 'su root' failed for user on /dev/pts/8
```

- Priority in `<>`, timestamp, hostname, tag (appname), message
- Less structured, more free-form
- Widely used, especially by older systems

**RFC 5424 (Modern syslog)** - Structured format:

```
<165>1 2003-10-11T22:14:15.003Z mymachine.example.com evntslog - ID47 [exampleSDID@32473 iut="3" eventSource="Application"] An application event log entry...
```

- Version number (1), ISO 8601 timestamp, hostname, appname, procid, msgid
- Structured data in `[key=value]` format
- More precise, better for machine parsing

**Parser Behavior**:

- syslog_loose automatically detects format
- Extracts fields into unified structure
- LLM sees same data regardless of format

### 5. Dual Logging

All operations use **dual logging**:

- **DEBUG**: Message summary (facility, severity, hostname, peer address)
- **TRACE**: Full message text
- **INFO**: LLM messages and high-level events
- **ERROR**: Parse failures, LLM errors
- All logs go to both `netget.log` (via tracing) and TUI Status panel (via status_tx)

## LLM Integration

### Action-Based Message Handling

The LLM responds to syslog events with actions:

**Events**:

- `syslog_message` - Syslog message received
    - Parameters: `facility`, `severity`, `timestamp`, `hostname`, `appname`, `message`

**Available Actions**:

- `store_syslog_message` - Store message for later analysis (sync)
- `forward_syslog` - Forward message to another syslog server (async)
- `ignore_syslog_message` - Drop message (sync)
- Common actions: `show_message`, `update_instruction`, etc.

### Example LLM Responses

**Store Critical Messages**:

```json
{
  "actions": [
    {
      "type": "store_syslog_message",
      "message": "<34>Oct 11 22:14:15 mymachine su: 'su root' failed for user on /dev/pts/8"
    },
    {
      "type": "show_message",
      "message": "⚠️ Critical: Failed su attempt detected from mymachine"
    }
  ]
}
```

**Forward to Central Server**:

```json
{
  "actions": [
    {
      "type": "forward_syslog",
      "target": "192.168.1.100:514",
      "message": "<165>1 2003-10-11T22:14:15.003Z mymachine evntslog - ID47 [exampleSDID@32473 iut=\"3\"] Event log entry"
    }
  ]
}
```

**Ignore Debug Messages**:

```json
{
  "actions": [
    {
      "type": "ignore_syslog_message"
    }
  ]
}
```

**Alert on Emergency**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "🚨 EMERGENCY: Kernel panic on server-01! Priority: 0 (emergency)"
    },
    {
      "type": "forward_syslog",
      "target": "192.168.1.200:514",
      "message": "<0>Oct 11 22:14:15 server-01 kernel: Kernel panic - not syncing"
    }
  ]
}
```

## Connection Management

### Stateless Protocol

Syslog has no connection concept:

- Each UDP packet is independent message
- No handshake, no session, no connection state
- No authentication (plaintext, anyone can send)

### Message Processing Flow

1. Receive UDP packet on port 514
2. Parse as syslog message (RFC 3164 or 5424)
3. Extract facility, severity, timestamp, hostname, appname, message
4. Create `syslog_message` event with message data
5. Call LLM with event
6. LLM returns actions (store, forward, ignore, alert)
7. Execute actions
8. No response sent to client (one-way protocol)

### Concurrent Messages

- Each message spawned in separate tokio task
- No queueing (unlike TCP protocols)
- Messages from different clients processed in parallel
- Ollama lock serializes LLM calls but not UDP I/O

## Known Limitations

### 1. No TCP Transport

- Only UDP supported (port 514)
- TCP syslog (RFC 6587) not implemented
- No TLS/SSL encryption (RFC 5425) not implemented

**Rationale**: UDP is the standard syslog transport. TCP adds complexity (connection management, framing) and is less
common.

### 2. No Message Storage

- `store_syslog_message` action defined but doesn't persist to disk
- Messages only processed by LLM, not saved
- Would require database or file storage implementation

**Workaround**: LLM can log messages to netget.log via `show_message` action. For persistent storage, forward to real
syslog server (rsyslog, syslog-ng).

### 3. No Message Forwarding (Yet)

- `forward_syslog` action defined but not fully implemented
- Would require LLM-initiated UDP send to target server
- Would need configuration of forwarding targets

**Future Enhancement**: Allow LLM to forward messages to central syslog server for aggregation.

### 4. No Structured Data Parsing

- RFC 5424 structured data `[key=value]` not extracted
- Treated as part of message text
- LLM must parse from message string if needed

**Workaround**: syslog_loose provides raw structured data, could be enhanced to extract and pass to LLM.

### 5. No Rate Limiting

- No protection against syslog floods
- Attacker can send thousands of messages per second
- Each message triggers LLM call (expensive)

**Workaround**: Use `--ollama-lock` to serialize LLM calls. LLM can implement filtering logic to ignore noisy sources.

### 6. No Authentication

- Syslog (RFC 3164/5424 over UDP) has no authentication
- Anyone can send messages claiming any hostname
- No way to verify message source

**Rationale**: Standard syslog has no auth. Use this as honeypot to log forged messages.

## Example Prompts

### Basic Syslog Server

```
listen on port 514 via syslog
Store all messages with severity <= warning (0-4).
Ignore debug and info messages (6-7).
Show alerts for critical and above (0-2).
```

### Security Event Collector

```
listen on port 514 via syslog
Monitor auth and authpriv facilities.
Alert on failed login attempts (message contains "failed").
Store all authentication events.
Track hostnames sending auth messages.
```

### Log Aggregator with Forwarding

```
listen on port 514 via syslog
Accept logs from all sources.
Forward critical and above (0-2) to 192.168.1.100:514.
Store error and above (0-3) locally.
Show message count every 100 messages.
```

### Honeypot Syslog Server

```
listen on port 514 via syslog
Log all syslog messages (any source, any facility).
Track unique sender IPs and hostnames.
Alert on suspicious patterns:
- Messages claiming to be from kernel but not from trusted IPs
- Unusually high message rates from single source
- Messages with emergency/alert severity from unknown hosts
```

### Application Log Monitor

```
listen on port 514 via syslog
Accept only local0-local7 facilities (custom apps).
Parse message text for keywords:
- "error", "exception", "crash" → alert
- "startup", "shutdown" → log lifecycle
Track which applications (appname) are most active.
```

## Performance Characteristics

### Latency

- Parse message: <1ms (syslog_loose parsing)
- LLM processing: 2-5s (typical)
- Total: ~2-5s per message (LLM dominates)

**Impact**: High message rates will cause backlog if LLM can't keep up.

### Throughput

- Limited by LLM response time
- Concurrent messages handled in parallel (each in own tokio task)
- UDP has no connection overhead (faster than TCP)

**Recommendation**: For production, use filtering to reduce LLM calls (e.g., only process severity ≤ warning).

### Concurrency

- Unlimited concurrent messages (bounded by system resources)
- Each message processed independently
- Ollama lock serializes LLM calls but not UDP I/O or parsing

### Memory

- Each message allocates ~65KB buffer (max UDP packet size)
- Parsing allocates small strings (<1KB typical)
- No persistent state per client (stateless protocol)

## Security Considerations

### Plaintext Protocol

- Syslog (UDP/514) has no encryption
- All messages sent in cleartext
- Anyone on network can sniff messages

### No Authentication

- No verification of sender identity
- Hostname in message can be forged
- IP address can be spoofed (UDP)

### Honeypot Usage

Syslog commonly targeted by attackers:

- Port 514 scanned frequently
- Fake messages sent to test monitoring
- LLM can log forged messages and detect patterns
- Useful for tracking reconnaissance activity

### Denial of Service

- Attacker can flood with messages
- Each triggers LLM call (expensive)
- No built-in rate limiting

**Mitigation**: LLM can implement logic to ignore high-rate sources. Use `--ollama-lock` to prevent Ollama overload.

## References

- [RFC 3164: BSD syslog Protocol](https://datatracker.ietf.org/doc/html/rfc3164)
- [RFC 5424: The Syslog Protocol](https://datatracker.ietf.org/doc/html/rfc5424)
- [RFC 5425: TLS Transport Mapping for Syslog](https://datatracker.ietf.org/doc/html/rfc5425)
- [RFC 6587: Transmission of Syslog Messages over TCP](https://datatracker.ietf.org/doc/html/rfc6587)
- [syslog_loose Documentation](https://docs.rs/syslog_loose/latest/syslog_loose/)
- [Syslog Severity Levels](https://en.wikipedia.org/wiki/Syslog#Severity_level)
- [Syslog Facility Codes](https://en.wikipedia.org/wiki/Syslog#Facility)
