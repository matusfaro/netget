# BitTorrent Tracker Protocol - Implementation

## Overview

The BitTorrent Tracker is an HTTP-based coordination server that helps BitTorrent clients find peers for specific
torrents. This implementation provides a fully LLM-controlled tracker that can respond to announce and scrape requests.

## Protocol Specification

- **Base Protocol**: HTTP/1.1 over TCP
- **Encoding**: Bencode (BitTorrent's serialization format)
- **Request Types**: GET requests with query parameters
- **Response Types**: Bencode dictionaries or plaintext errors
- **Port**: Typically 6969 or 8080 (user-configurable)
- **RFC/BEP**: BEP 3 (The BitTorrent Protocol Specification)

## Architecture

### Server Implementation (`mod.rs`)

**Library Choice**: Pure Tokio implementation

- No external tracker library (HTTP parsing + bencode is sufficient)
- `tokio::net::TcpListener` for TCP connections
- `serde_bencode` (0.2) for bencode encoding/decoding
- `urlencoding` for URL-encoded query parameters

**Key Components**:

```rust
pub struct TorrentTrackerServer;

impl TorrentTrackerServer {
    pub async fn spawn_with_llm_actions(...) -> Result<SocketAddr>
    async fn handle_connection(...) -> Result<()>
    fn parse_http_request(request: &str) -> Result<(String, HashMap<String, serde_json::Value>)>
}
```

**Connection Flow**:

1. Accept TCP connection
2. Read HTTP GET request (up to 8192 bytes)
3. Parse request line and query parameters
4. Identify request type (announce or scrape)
5. Convert to JSON for LLM
6. LLM returns action (send_announce_response, send_scrape_response, or send_error_response)
7. Encode response in bencode format
8. Wrap in HTTP response
9. Send to client
10. Close connection (HTTP/1.0 style)

### LLM Actions (`actions.rs`)

**Protocol Trait Implementation**: `Server` trait from `crate::llm::actions::protocol_trait`

**Sync Actions** (network-triggered):

1. **send_announce_response** - Return peer list for a torrent
    - Parameters: `interval` (seconds), `peers` (array of {ip, port, peer_id})
    - Output: HTTP response with bencode peer dictionary
    - Example:
   ```json
   {
     "type": "send_announce_response",
     "interval": 1800,
     "peers": [
       {"ip": "192.168.1.100", "port": 51413, "peer_id": "2d5452323934302d..."}
     ]
   }
   ```

2. **send_scrape_response** - Return statistics for torrents
    - Parameters: `files` (array of {info_hash, complete, incomplete, downloaded})
    - Output: HTTP response with bencode scrape data
    - Example:
   ```json
   {
     "type": "send_scrape_response",
     "files": [
       {
         "info_hash": "0123456789abcdef0123456789abcdef01234567",
         "complete": 10,
         "incomplete": 5,
         "downloaded": 100
       }
     ]
   }
   ```

3. **send_error_response** - Return error message
    - Parameters: `failure_reason` (string)
    - Output: Bencode dictionary with failure reason
    - Example:
   ```json
   {
     "type": "send_error_response",
     "failure_reason": "Torrent not registered"
   }
   ```

**Event Types** (incoming requests):

1. **tracker_announce_request** - Client announces presence and requests peers
    - Payload: `{info_hash, peer_id, port, uploaded, downloaded, left, event, numwant, compact}`
    - Common events: "started", "completed", "stopped", or empty

2. **tracker_scrape_request** - Client requests statistics
    - Payload: `{info_hashes: [...]}`

### Request Parsing

**URL Format**:

```
GET /announce?info_hash=%XX%XX...&peer_id=%XX%XX...&port=6881&uploaded=0&downloaded=0&left=1000000&event=started HTTP/1.1
```

**Parameter Handling**:

- **info_hash** / **peer_id**: URL-encoded binary → hex string (40 chars)
- **port** / **uploaded** / **downloaded** / **left** / **numwant** / **compact**: Parsed as u64
- **event**: String ("started", "completed", "stopped", or empty)
- **ip**: String (client IP, optional)

**Special Cases**:

- Info hash and peer ID are 20 bytes each, URL-encoded in request
- Compact format (compact=1) returns 6-byte peer format (4 bytes IP + 2 bytes port)
- Non-compact format returns bencode list of peer dictionaries

### Response Encoding

**Announce Response (Success)**:

```python
{
  "interval": 1800,           # Seconds until next announce
  "complete": 10,             # Number of seeders (optional)
  "incomplete": 5,            # Number of leechers (optional)
  "peers": [                  # Non-compact format
    {
      "peer id": "...",       # 20 bytes
      "ip": "192.168.1.100",
      "port": 6881
    }
  ]
  # OR
  "peers": "..."              # Compact format: 6 bytes per peer (4 IP + 2 port)
}
```

**Scrape Response**:

```python
{
  "files": {
    "<20-byte info_hash>": {
      "complete": 10,
      "incomplete": 5,
      "downloaded": 100
    }
  }
}
```

**Error Response**:

```python
{
  "failure reason": "Error message"
}
```

## LLM Integration

### Instruction Guidelines

**Example Instruction**:

```
You are a BitTorrent tracker server. Track active peers for torrents and return peer lists on announce requests. Use a 30-minute announce interval. For scrape requests, return current statistics.
```

**Behavior Control**:

- **Public Tracker**: Return all known peers for any info_hash
- **Private Tracker**: Check authorization, return errors for unknown torrents
- **Peer Limits**: Control how many peers to return (numwant parameter)
- **Statistics**: Track seeders (left=0) vs leechers (left>0)

### Typical LLM Response Flow

**Announce Request**:

1. LLM receives JSON:
   `{info_hash: "abc...", peer_id: "xyz...", port: 6881, uploaded: 0, downloaded: 0, left: 1000000, event: "started"}`
2. LLM tracks this peer internally (can use schedule_task for cleanup)
3. LLM returns: `{type: "send_announce_response", interval: 1800, peers: [...]}`

**Scrape Request**:

1. LLM receives JSON: `{info_hashes: ["abc...", "def..."]}`
2. LLM looks up statistics for each torrent
3. LLM returns:
   `{type: "send_scrape_response", files: [{info_hash: "abc...", complete: 10, incomplete: 5, downloaded: 100}]}`

## Logging Strategy

**DEBUG Level**:

- Connection accepted/closed
- Request type identified
- LLM call initiated
- Response size sent

**TRACE Level**:

- Full HTTP request text
- Full HTTP response text
- Parsed query parameters

**INFO Level**:

- LLM-generated messages (via execution_result.messages)

**ERROR Level**:

- Accept errors
- Parse errors
- LLM call failures

## Connection State Tracking

**ProtocolConnectionInfo Variant**:

```rust
TorrentTracker {
    recent_requests: Vec<(String, Instant)>,  // Request type + timestamp
}
```

Tracks announce vs scrape request frequency per connection (though most clients make single requests per connection).

## Limitations

1. **Stateless by design**: Each connection is independent. LLM must track peers across connections using conversation
   history or scheduled tasks.

2. **No persistent storage**: Peer lists exist only in LLM context. Server restart = empty tracker (unless LLM uses
   filesystem or external storage).

3. **No UDP support**: Only HTTP tracker protocol. UDP tracker (BEP 15) not implemented.

4. **No IPv6 compact format**: Compact peer format only supports IPv4 (6 bytes per peer). IPv6 requires 18 bytes per
   peer.

5. **Single-threaded LLM calls**: One LLM call per request. High-traffic trackers may experience latency.

6. **No authentication**: No built-in support for private tracker authentication (can be added via LLM logic).

## Security Considerations

- **Input validation**: Info hash must be exactly 20 bytes (40 hex chars)
- **Port range**: Clients can announce any port (no validation)
- **IP spoofing**: No verification of client IP (uses peer-provided IP or connection IP)
- **DoS protection**: No rate limiting (LLM could implement via scheduled tasks)

## Testing

See `tests/server/torrent_tracker/CLAUDE.md` for comprehensive testing documentation.

## References

- [BEP 3: The BitTorrent Protocol Specification](http://www.bittorrent.org/beps/bep_0003.html)
- [BitTorrent Tracker Protocol](https://wiki.theory.org/BitTorrentSpecification#Tracker_HTTP.2FHTTPS_Protocol)
- [Bencode Encoding](https://wiki.theory.org/BitTorrentSpecification#Bencoding)
