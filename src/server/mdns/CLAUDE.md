# mDNS Protocol Implementation

## Overview
mDNS/DNS-SD (Multicast DNS / DNS Service Discovery) server for zero-configuration network service advertisement. Implements RFC 6762 (mDNS) and RFC 6763 (DNS-SD) using the mdns-sd library.

## Library Choices
- **mdns-sd** v0.11+ - Full mDNS/DNS-SD implementation
  - Handles multicast group management (224.0.0.251:5353)
  - Service advertisement and discovery
  - TXT record management
  - Automatic response caching and conflict resolution
- Chosen because mDNS/DNS-SD is complex (multicast, timing, caching)
- Library handles protocol details, LLM focuses on service registration

## Architecture Decisions

### Service Advertisement Model
mDNS is **advertisement-only** in NetGet:
- Server starts, LLM registers services via `register_mdns_service` action
- No incoming request handling (mDNS is announcement-based)
- Services continuously advertised until server shutdown
- No sync actions - only async `register_mdns_service`

### LLM Integration
- **Single event type**: `MDNS_SERVER_STARTUP_EVENT`
- Triggered once when mDNS server initializes
- LLM returns one or more `register_mdns_service` actions
- **Manual action processing** - Actions processed in `spawn_with_llm_actions()` directly
- Actions not executed via standard `ProtocolActions::execute_action()` flow

### Service Registration Flow
1. Server startup triggers `MDNS_SERVER_STARTUP_EVENT`
2. LLM returns actions: `[{type: "register_mdns_service", ...}, ...]`
3. Code extracts `raw_actions` from execution result
4. For each action, create `ServiceInfo` with:
   - Service type (e.g., `_http._tcp.local.`)
   - Instance name (e.g., `My Web Server`)
   - Host name (generated from instance name)
   - Port number
   - TXT properties (key-value pairs)
5. Register service with `mdns.register(service_info)`
6. Service continuously advertised on multicast address

### Local IP Detection
Uses heuristic to find local IP:
```rust
fn get_local_ip() -> Option<String> {
    // Bind UDP socket and "connect" to 8.8.8.8:80 (no packets sent)
    // Socket reports local IP that would route to destination
    // Fallback to 127.0.0.1 if detection fails
}
```

## Connection Management
- **No connections** - mDNS is multicast-based
- No TCP/UDP listener
- `ServiceDaemon` runs in background tokio task
- Daemon kept alive with infinite sleep loop
- Dummy address returned: `224.0.0.251:5353` (multicast group)

## State Management
- **No state** - Service registrations managed by mdns-sd library
- No tracking in `AppState`
- Services automatically re-announced periodically per RFC 6762
- Daemon cleanup handled by Drop trait

## Service Types
Common service types:
- `_http._tcp.local.` - HTTP web server
- `_https._tcp.local.` - HTTPS web server
- `_ftp._tcp.local.` - FTP server
- `_ssh._tcp.local.` - SSH server
- `_printer._tcp.local.` - Printer service
- `_smb._tcp.local.` - SMB/CIFS file sharing
- Custom types: `_myapp._tcp.local.`

## TXT Properties
Optional key-value pairs advertised with service:
- `txtvers=1` - TXT record version
- `path=/api` - Service path
- `version=2.0` - Application version
- `secure=true` - Security flag
- Any custom properties

## Limitations
- **Advertisement only** - No mDNS query handling
- **No dynamic updates** - Services registered at startup only
- **No service unregistration** - Services advertised until shutdown
- **No conflict resolution visibility** - Handled by library
- **No IPv6 support** - IPv4 only (could be added)
- **No custom TTL** - Uses library defaults
- **No service browsing** - NetGet doesn't browse for other services

## Examples

### Example LLM Prompt
```
listen on port 8080 via mdns. Advertise HTTP service:
- type: _http._tcp.local.
- name: NetGet Web Server
- port: 8080
- properties: version=1.0, path=/
```

### Example LLM Response (Single Service)
```json
{
  "actions": [
    {
      "type": "register_mdns_service",
      "service_type": "_http._tcp.local.",
      "instance_name": "NetGet Web Server",
      "port": 8080,
      "properties": {
        "version": "1.0",
        "path": "/"
      }
    }
  ]
}
```

### Example LLM Response (Multiple Services)
```json
{
  "actions": [
    {
      "type": "register_mdns_service",
      "service_type": "_http._tcp.local.",
      "instance_name": "Web Server",
      "port": 8080,
      "properties": {
        "path": "/"
      }
    },
    {
      "type": "register_mdns_service",
      "service_type": "_ftp._tcp.local.",
      "instance_name": "FTP Server",
      "port": 21,
      "properties": {
        "version": "1.0"
      }
    }
  ]
}
```

## Technical Details

### Multicast DNS
- **Multicast group**: 224.0.0.251 (IPv4)
- **Port**: 5353
- **TTL**: Typically 255 (link-local)
- **Announcement frequency**: Initial burst (3x), then periodic

### DNS-SD Records
For service `My Web Server._http._tcp.local.`:
- **PTR** record: `_http._tcp.local.` → `My Web Server._http._tcp.local.`
- **SRV** record: hostname + port
- **TXT** record: key=value properties
- **A** record: IPv4 address

### Service Discovery
Other devices discover services by:
1. Querying `_http._tcp.local.` PTR records
2. Receiving instance names
3. Querying instance SRV/TXT/A records
4. Connecting to advertised IP:port

## References
- RFC 6762 - Multicast DNS
- RFC 6763 - DNS-Based Service Discovery
- mdns-sd documentation: https://docs.rs/mdns-sd
- Apple Bonjour: https://developer.apple.com/bonjour/
