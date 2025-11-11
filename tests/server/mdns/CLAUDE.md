# mDNS Protocol E2E Tests

## Test Overview

Tests mDNS service advertisement with real mDNS-SD client library (`mdns-sd`) validating service discovery and TXT
record properties.

## Test Strategy

- **Consolidated per feature** - Each test focuses on a specific mDNS capability
- **Multiple server instances** - 4 separate servers (one per test)
- **Real mDNS client** - Uses `mdns-sd` library for protocol correctness
- **Discovery-based validation** - Tests verify services are advertised and discoverable
- **No scripting** - Action-based service registration only

## LLM Call Budget

- `test_mdns_service_advertisement()`: 1 startup call (single service registration)
- `test_mdns_multiple_services()`: 1 startup call (multiple service registrations)
- `test_mdns_service_with_properties()`: 1 startup call (service with TXT properties)
- `test_mdns_custom_service_type()`: 1 startup call (custom service type)
- **Total: 4 LLM calls** (4 startups, 0 subsequent calls)

**Well under 10 LLM call limit** - mDNS is startup-only, no per-request processing.

## Scripting Usage

**Scripting Disabled** - mDNS uses action-based service registration

- Services registered once at startup via `register_mdns_service` action
- No ongoing network events to script
- LLM interprets user prompt and returns service definitions

## Client Library

**mdns-sd** v0.11+ - Multicast DNS service discovery

- `ServiceDaemon::new()` - Create mDNS daemon
- `browse(service_type)` - Browse for services of a type
- `recv_async()` - Receive service events asynchronously
- Events: `ServiceFound`, `ServiceResolved`, `ServiceRemoved`
- Real mDNS library ensures protocol correctness

## Expected Runtime

- Model: qwen3-coder:30b
- Runtime: ~40-60 seconds for full test suite
- Fast due to only 4 LLM calls
- Most time spent waiting for mDNS service resolution (10-second timeouts per test)

## Failure Rate

- **Medium** (10-15%) - mDNS discovery can be flaky
- Common issues:
    - Service not discovered within timeout (network/timing)
    - LLM returns incorrect service_type format
    - LLM omits required fields (port, instance_name)
    - Firewall blocking multicast traffic (224.0.0.251)

## Test Cases

1. **test_mdns_service_advertisement** - Tests basic service registration and discovery
2. **test_mdns_multiple_services** - Tests registering multiple services simultaneously
3. **test_mdns_service_with_properties** - Tests TXT record properties
4. **test_mdns_custom_service_type** - Tests custom service types (non-standard)

## Known Issues

- **Discovery timing** - Tests wait up to 10 seconds for service resolution
- **Flaky on some networks** - Multicast may be blocked or delayed
- Tests use polling with timeout (not ideal but necessary for mDNS)
- Service resolution may succeed on `ServiceFound` instead of `ServiceResolved`
- Tests may pass even if properties are missing (resolution might not include TXT)

## Example Test Pattern

```rust
// Start server with service registration prompt
let server = start_netget_server(ServerConfig::new(prompt)).await?;

// Create mDNS browser
let mdns = mdns_sd::ServiceDaemon::new()?;
let service_type = "_http._tcp.local.";
let receiver = mdns.browse(service_type)?;

// Poll for service discovery (with timeout)
let mut found_service = false;
let timeout_duration = Duration::from_secs(10);
let start = Instant::now();

while start.elapsed() < timeout_duration {
    match tokio::time::timeout(Duration::from_secs(2), async {
        receiver.recv_async().await
    }).await {
        Ok(Ok(event)) => {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    // Service fully resolved with IP/port
                    assert_eq!(info.get_port(), expected_port);
                    found_service = true;
                    break;
                }
                ServiceEvent::ServiceFound(ty, fullname) => {
                    // Service found but not yet resolved
                    // May be sufficient for some tests
                }
                _ => {}
            }
        }
        _ => continue, // Timeout or error, keep polling
    }
}

assert!(found_service, "Service should be discovered");

// Cleanup
mdns.shutdown();
server.stop().await?;
```

## mDNS Discovery Flow

1. **Advertiser** (NetGet) registers service
2. **Browser** (test client) sends multicast query for service type
3. **Advertiser** responds with PTR, SRV, TXT, A records
4. **Browser** receives `ServiceFound` event (quick)
5. **Browser** resolves SRV/A records → `ServiceResolved` event (may take seconds)

## Performance Considerations

- mDNS uses multicast (224.0.0.251:5353)
- Service resolution can take 1-5 seconds depending on network
- Tests use generous 10-second timeouts to avoid flakes
- Multiple services discovered independently (no batching)

## Network Requirements

- **Multicast support** - Network must allow 224.0.0.251
- **Local network only** - mDNS is link-local (not routed)
- **Firewall rules** - UDP port 5353 must be allowed
- **Docker/VM** - May have issues with multicast forwarding

## Service Type Format

Service types must follow DNS-SD naming:

- Format: `_<service>._<proto>.local.`
- Examples:
    - `_http._tcp.local.` - HTTP service
    - `_ftp._tcp.local.` - FTP service
    - `_myapp._tcp.local.` - Custom application
- **Trailing period required** - `local.` not `local`
- **Lowercase** - Service types are case-insensitive but lowercase preferred

## TXT Record Properties

Properties are key-value pairs:

- Max 255 bytes per property
- Keys typically lowercase
- Values as strings
- Example: `{"version": "1.0", "path": "/api"}`
