# WireGuard Client E2E Tests

## Test Strategy

**Approach**: Unit tests + Manual E2E (ignored by default)

The WireGuard client tests are split into two categories:

1. **Unit Tests** (always run):
   - Parameter parsing validation
   - Action definition verification
   - Event type verification
   - Protocol metadata validation

2. **E2E Tests** (ignored by default, requires manual setup):
   - Actual VPN connection to running server
   - Handshake verification
   - Status queries
   - Graceful disconnection

## LLM Call Budget

**Target**: < 5 LLM calls for full E2E suite (when run)

**Rationale**:
- WireGuard is connection-oriented, not request/response
- LLM calls only on connect/disconnect events
- Status queries don't require LLM calls
- Minimal back-and-forth needed

## Expected Runtime

**Unit tests**: < 1 second
**E2E tests** (when enabled): ~20-30 seconds
- Connection establishment: ~5 seconds
- Handshake wait: ~10 seconds
- Status queries: ~5 seconds
- Disconnection: ~5 seconds

## Test Requirements

### Unit Tests
- No special requirements
- Run with: `./cargo-isolated.sh test --no-default-features --features wireguard`

### E2E Tests (Ignored)
- **Root privileges** (Linux/FreeBSD/Windows) or standard user (macOS)
- **Running WireGuard server** with known configuration
- **Valid server public key**
- **Network connectivity** to server endpoint

## Running E2E Tests Manually

### Setup

1. Start WireGuard server:
```bash
# Terminal 1 (as root)
sudo ./cargo-isolated.sh run --no-default-features --features wireguard

# In netget TUI
> Start a WireGuard VPN server on port 51820

# Note the server public key from output:
# [INFO] Server public key: xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
```

2. Run client tests:
```bash
# Terminal 2 (as root on Linux/FreeBSD/Windows)
sudo ./cargo-isolated.sh test --no-default-features --features wireguard client::wireguard::e2e_test::tests::test_wireguard_client_connectivity -- --ignored
```

### Manual Testing

For interactive testing:

```bash
# Start netget as root
sudo ./cargo-isolated.sh run --no-default-features --features wireguard

# In TUI
> Connect to WireGuard VPN at 127.0.0.1:51820 with server public key xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg= and assign me IP 10.20.30.2/32

# Verify connection
> Check VPN connection status

# Disconnect
> Disconnect from VPN
```

## Test Coverage

### What We Test

✅ **Protocol Metadata**:
- Protocol name, stack name, group name
- Keywords for parsing
- Startup parameter definitions

✅ **Action Definitions**:
- Async actions (get_connection_status, disconnect, get_client_info)
- Sync actions (none for WireGuard)
- Action parameter validation

✅ **Event Types**:
- wireguard_connected event
- wireguard_disconnected event

### What We Don't Test (E2E)

❌ **Actual VPN Traffic**: Don't test data transfer through tunnel
❌ **Multiple Clients**: Don't test concurrent VPN connections
❌ **Reconnection Logic**: Don't test automatic reconnect
❌ **Packet Routing**: Don't verify routing table changes
❌ **Keepalive**: Don't test persistent keepalive behavior

**Rationale**: These are infrastructure concerns handled by WireGuard library (defguard_wireguard_rs) and kernel. Our tests focus on LLM integration and control flow.

## Known Issues

### Platform-Specific

- **macOS**: Uses userspace wireguard-go (slower, different interface naming)
- **Linux**: Requires kernel WireGuard or module
- **Windows**: Requires WireGuard driver installed

### Test Limitations

- E2E tests disabled by default (requires root + server)
- No automated E2E in CI (privilege escalation required)
- Manual test setup required for full validation

### Flaky Tests

None expected. Unit tests are deterministic. E2E tests (when run) may timeout if:
- Server not running
- Network connectivity issues
- Firewall blocks UDP port 51820
- Insufficient privileges

## CI Integration

**Current Status**: E2E tests ignored in CI

**Reason**: Requires root privileges and running server infrastructure

**Future**: Could add privileged CI runner with WireGuard server setup

## Test Maintenance

- Unit tests should always pass
- E2E tests require manual verification after major changes
- Update test documentation when adding new actions or events
- Keep LLM call budget under 5 for E2E suite

## Example Test Output

### Unit Tests (Passing)
```
running 2 tests
test tests::test_wireguard_param_parsing ... ok
test tests::test_wireguard_actions ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### E2E Tests (When Run)
```
running 1 test
test tests::test_wireguard_client_connectivity ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
