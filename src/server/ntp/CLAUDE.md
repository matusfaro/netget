# NTP Protocol Implementation

## Overview

NTP (Network Time Protocol) server implementing RFC 5905 for time synchronization. The LLM controls time responses with
stratum levels, reference timestamps, and clock precision using structured actions.

**Status**: Beta (Core Protocol)
**RFC**: RFC 5905 (NTPv4), RFC 1305 (NTPv3)
**Port**: 123 (UDP)

## Library Choices

- **Manual NTP packet construction** - No external library
    - NTP protocol is simple enough to implement directly
    - 48-byte fixed-size packet structure
    - Minimal parsing required (just extract client's transmit timestamp)
    - Custom `build_ntp_packet()` function constructs responses

**Rationale**: Unlike DNS/DHCP, NTP packet structure is very simple (fixed 48 bytes, mostly timestamps). Using a library
would add unnecessary dependency. Manual implementation provides full control and is easier to understand.

**Note**: ntpd-rs exists but is a full NTP daemon implementation, not a simple protocol library.

## Architecture Decisions

### 1. Action-Based LLM Control

The LLM responds with a single action:

- `send_ntp_time_response` - Send time synchronization response
- `send_ntp_response` - Send raw hex packet (advanced)
- `ignore_request` - No response

The `send_ntp_time_response` action has many optional parameters (stratum, precision, timestamps), all with sensible
defaults. LLM typically only needs to specify stratum level.

### 2. Automatic Origin Timestamp Injection

NTP protocol requires server to echo client's transmit timestamp as origin timestamp:

1. Client sends request with transmit_timestamp = T1
2. Server must echo T1 as origin_timestamp in response
3. This allows client to calculate round-trip delay

**Implementation**: Server automatically extracts client's transmit timestamp from request (bytes 40-47) and injects it
into the response if LLM doesn't provide it. This reduces LLM burden - it doesn't need to understand timestamp echoing.

### 3. Flexible Timestamp Format

Timestamps can be provided as:

- `"current_time"` - Use system's current time (most common)
- Unix timestamp (seconds since 1970) - Converted to NTP format
- Raw NTP timestamp (64-bit: seconds + fraction) - Used as-is
- `null` or omitted - Defaults to current_time

**NTP Epoch**: NTP uses January 1, 1900 as epoch (2,208,988,800 seconds before Unix epoch)

### 4. Sensible Defaults

LLM doesn't need to understand NTP packet structure. Default values:

- `leap_indicator`: 0 (no warning)
- `stratum`: 2 (secondary time source)
- `poll`: 6 (64-second polling interval)
- `precision`: -20 (~1 microsecond)
- `root_delay`: 0.0 (no delay)
- `root_dispersion`: 0.0 (no error)
- `reference_id`: "" (empty)
- All timestamps: current_time

LLM can override any field if instructed (e.g., "act as stratum 1 server").

### 5. Dual Logging

- **DEBUG**: Request summary ("NTP received 48 bytes from 127.0.0.1")
- **TRACE**: Full hex dump of NTP packets (both request and response)
- Both go to netget.log and TUI Status panel

### 6. Connection Tracking

Each NTP request creates a "connection" entry:

- Connection ID: Unique per request
- Protocol info: `ProtocolConnectionInfo::Ntp` with recent_clients list
- Tracks client addresses and timestamps
- Status: Active during processing

## LLM Integration

### Event Type

**`ntp_request`** - Triggered when NTP client sends time request

Event parameters:

- `current_time` (number) - Current server time as Unix timestamp
- `client_transmit_timestamp` (number, optional) - Client's transmit time
- `bytes_received` (number) - Size of received packet

### Available Actions

#### `send_ntp_time_response`

Send NTP time synchronization response. Most fields have sensible defaults.

Parameters (all optional except where noted):

- `leap_indicator` - Leap second warning: 0=none, 1=+1s, 2=-1s, 3=unsync (default: 0)
- `stratum` - Stratum level: 0=unspec, 1=primary, 2-15=secondary (default: 2)
- `poll` - Poll interval as log2(seconds): 4=16s, 6=64s, 10=1024s (default: 6)
- `precision` - Clock precision as log2(seconds), negative values (default: -20)
- `root_delay` - Round-trip delay to primary reference in seconds (default: 0.0)
- `root_dispersion` - Max error relative to primary reference in seconds (default: 0.0)
- `reference_id` - 4-char identifier: "LOCL", "GPS.", "PPS.", "ATOM", or IP (default: "")
- `reference_timestamp` - When clock was last set (default: current_time)
- `origin_timestamp` - Client's transmit time (default: auto-extracted from request)
- `receive_timestamp` - When server received request (default: current_time)
- `transmit_timestamp` - When server sends response (default: current_time)

**Note**: LLM should leave origin_timestamp null - server auto-injects correct value.

#### `send_ntp_response` (Advanced)

Send custom NTP response packet as hex string (96 hex chars = 48 bytes).

#### `ignore_request`

Don't send any response.

### Example LLM Response

```json
{
  "actions": [
    {
      "type": "send_ntp_time_response",
      "stratum": 2
    },
    {
      "type": "show_message",
      "message": "Sent NTP time response as stratum 2 server"
    }
  ]
}
```

### Example: Stratum 1 Server

```json
{
  "actions": [
    {
      "type": "send_ntp_time_response",
      "stratum": 1,
      "reference_id": "GPS.",
      "precision": -20,
      "root_delay": 0.001,
      "root_dispersion": 0.001
    }
  ]
}
```

## Connection Management

### Connection Lifecycle

1. **Request Received**: UDP datagram on port 123
2. **Parse**: Extract client's transmit timestamp (bytes 40-47)
3. **Register**: Create ConnectionId and add to ServerInstance
4. **Process**: Call LLM with `ntp_request` event
5. **Auto-Inject**: Add origin_timestamp if not provided by LLM
6. **Build**: Construct 48-byte NTP response packet
7. **Respond**: Send UDP response
8. **Update**: Track bytes/packets sent/received
9. **Persist**: Connection remains in UI

### NTP Packet Structure

Fixed 48-byte packet:

- Byte 0: LI (2 bits) + Version (3 bits) + Mode (3 bits)
- Byte 1: Stratum
- Byte 2: Poll interval
- Byte 3: Precision
- Bytes 4-7: Root delay (32-bit fixed point)
- Bytes 8-11: Root dispersion (32-bit fixed point)
- Bytes 12-15: Reference ID (4 ASCII chars)
- Bytes 16-23: Reference timestamp (64-bit NTP timestamp)
- Bytes 24-31: Origin timestamp (64-bit NTP timestamp)
- Bytes 32-39: Receive timestamp (64-bit NTP timestamp)
- Bytes 40-47: Transmit timestamp (64-bit NTP timestamp)

**NTP Timestamp Format**: 64 bits = 32-bit seconds + 32-bit fraction (1/2^32 seconds precision)

## Known Limitations

### 1. NTPv4 Only

- Server responds with version 4 packets
- Accepts any version in requests (NTPv1-4)
- No NTPv5 support (still in draft)

### 2. No NTP Extensions

- No extension fields beyond 48-byte basic packet
- No authentication (NTP extensions)
- No autokey or symmetric key authentication

### 3. No Stratum 0

- Server can't act as primary reference clock (stratum 0 is reserved)
- Can act as stratum 1 (primary) or 2-15 (secondary)
- No integration with actual hardware clocks (GPS, atomic, etc.)

### 4. Simplified Time Handling

- Uses system time (SystemTime::now())
- No clock discipline algorithms
- No tracking of time offset or drift
- Just returns current system time with NTP formatting

### 5. No NTP Control Protocol

- Only implements client/server mode (mode 3 request → mode 4 response)
- No broadcast mode (mode 5)
- No NTP control messages (ntpq/ntpdc protocol)

### 6. No Kiss-of-Death

- Doesn't send Kiss-of-Death (KoD) packets to rate-limit clients
- No rate limiting or denial-of-service protection

## Example Prompts

### Basic NTP Server

```
listen on port 123 via ntp
Respond to NTP requests with the current system time
Use stratum 2
```

### Stratum 1 Server

```
listen on port 123 via ntp
Act as a stratum 1 NTP server
Use reference ID "GPS." to indicate GPS time source
Set precision to -20 (1 microsecond)
```

### Custom Stratum Server

```
listen on port 123 via ntp
Act as a stratum 3 NTP server
Reference identifier: "LOCL" (local clock)
Poll interval: 6 (64 seconds)
```

### High-Precision Server

```
listen on port 123 via ntp
Respond with:
  - Stratum 1
  - Reference ID: "ATOM" (atomic clock)
  - Precision: -30 (1 nanosecond)
  - Root delay: 0.0001 seconds
  - Root dispersion: 0.00001 seconds
```

## Performance Characteristics

### Latency

- **With Scripting**: Sub-millisecond (script handles requests)
- **Without Scripting**: 2-5 seconds (one LLM call per request)
- Packet construction: ~5-10 microseconds (very fast)
- Timestamp extraction: ~1-2 microseconds

### Throughput

- **With Scripting**: Tens of thousands of requests per second
- **Without Scripting**: Limited by LLM (~0.2-0.5 requests/sec)
- NTP traffic is very low volume (clients poll every 64-1024 seconds)

### Scripting Compatibility

NTP is excellent candidate for scripting:

- Extremely simple logic (just return current time)
- No state machine
- Deterministic responses
- Very high query rate potential

When scripting enabled:

- Server startup generates script (1 LLM call)
- All requests handled by script (0 LLM calls)
- Script can get system time and format NTP response instantly

### Time Accuracy

- Accuracy limited by system clock (typically ±1-100ms)
- No hardware clock integration
- No clock discipline or offset correction
- Good enough for testing, not for production time service

## References

- [RFC 5905: Network Time Protocol Version 4](https://datatracker.ietf.org/doc/html/rfc5905)
- [RFC 1305: Network Time Protocol Version 3](https://datatracker.ietf.org/doc/html/rfc1305)
- [NTP Packet Format](https://www.rfc-editor.org/rfc/rfc5905.html#section-7.3)
- [NTP Timestamp Format](https://www.rfc-editor.org/rfc/rfc5905.html#section-6)
- [NTP Stratum Levels](https://www.ntp.org/reflib/book/ch11/)
- [ntpd-rs (Rust NTP daemon)](https://github.com/pendulum-project/ntpd-rs)
