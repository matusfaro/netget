# USB FIDO2 E2E Tests

## Test Strategy

**Approach:** Black-box testing with real libfido2 tools and browsers

**Test Types:**
- Unit tests: None (protocol logic tested via E2E)
- E2E tests: USB/IP attachment + libfido2 commands
- Browser tests: WebAuthn with Chrome/Firefox (optional)

## LLM Call Budget

**Target:** < 5 LLM calls total
**Current:** 0 (tests use real client tools, not LLM)

**Rationale:**
- FIDO2 protocol doesn't require LLM for operation
- Tests verify USB/IP and cryptographic correctness
- No prompt-driven behavior to test

## Test Environment

### System Requirements

```bash
# Ubuntu/Debian
sudo apt-get install libusb-1.0-0-dev pkg-config usbip libfido2-dev fido2-tools

# Fedora/RHEL
sudo dnf install libusb1-devel pkgconfig usbip libfido2-devel fido2-tools

# macOS (USB/IP not available)
# Tests marked as ignored on macOS
```

### Kernel Module

```bash
# Load USB/IP kernel module
sudo modprobe vhci-hcd

# Verify module loaded
lsmod | grep vhci
```

## Running Tests

### All E2E Tests (Requires Root)

```bash
# Build with FIDO2 feature
./cargo-isolated.sh build --no-default-features --features usb-fido2

# Run tests (requires sudo for usbip)
sudo -E ./cargo-isolated.sh test --no-default-features --features usb-fido2 --test usb_fido2::e2e_test
```

### Individual Tests

```bash
# Test GetInfo only
sudo -E cargo test --no-default-features --features usb-fido2 test_fido2_get_info -- --ignored

# Test registration
sudo -E cargo test --no-default-features --features usb-fido2 test_fido2_make_credential -- --ignored
```

## Expected Runtime

| Test | Duration | LLM Calls |
|------|----------|-----------|
| Server startup | 2s | 0 |
| GetInfo | 5s | 0 |
| U2F registration | 10s | 0 |
| FIDO2 MakeCredential | 10s | 0 |
| FIDO2 GetAssertion | 15s | 0 |
| Multiple credentials | 30s | 0 |
| Reset | 10s | 0 |
| Chrome WebAuthn | 60s | 0 |

**Total:** ~2-3 minutes

## Known Issues

### Issue 1: Requires Root Access

**Problem:** usbip command requires root/sudo
**Workaround:** Run tests with `sudo -E`
**Fix:** Add udev rules for non-root usbip (future)

### Issue 2: libusb-dev Build Dependency

**Problem:** Compilation fails without libusb-1.0-dev
**Workaround:** Install system package before building
**Fix:** Document in README, add to CI

### Issue 3: Linux-Only

**Problem:** USB/IP requires Linux kernel module
**Workaround:** Tests automatically ignored on non-Linux
**Fix:** None (inherent limitation)

### Issue 4: USB/IP Port Conflicts

**Problem:** Multiple test runs may conflict on ports
**Workaround:** Use cargo-isolated.sh for separate target dirs
**Fix:** Random port allocation in tests

## Test Coverage

**Current Coverage:** 0% (tests not yet implemented)

**Target Coverage:**
- ✅ CTAPHID transport layer
- ✅ U2F commands (REGISTER, AUTHENTICATE, VERSION)
- ✅ CTAP2 commands (GetInfo, MakeCredential, GetAssertion, Reset)
- ✅ Credential management (create, use, delete)
- ❌ PIN/UV support (not implemented)
- ❌ Resident keys (not implemented)

## Manual Testing

### Quick Manual Test

```bash
# Terminal 1: Start server
cargo run --no-default-features --features usb-fido2 -- --protocol usb-fido2 --listen 0.0.0.0:3240

# Terminal 2: Attach device
sudo modprobe vhci-hcd
sudo usbip list -r localhost
sudo usbip attach -r localhost:3240 -b 1-1

# Terminal 3: Test with libfido2
fido2-token -L
fido2-token -I /dev/hidraw0
fido2-cred -M -h example.com /dev/hidraw0

# Cleanup
sudo usbip detach -p 0
```

### Browser Testing

1. Start FIDO2 server on localhost:3240
2. Attach via `sudo usbip attach`
3. Open Chrome to https://webauthn.io
4. Click "Register new credential"
5. Touch virtual security key (auto-approved)
6. Verify credential created
7. Click "Authenticate"
8. Verify authentication succeeds

## References

- libfido2 docs: https://developers.yubico.com/libfido2/
- fido2-tools: https://packages.ubuntu.com/search?keywords=fido2-tools
- USB/IP protocol: https://www.kernel.org/doc/html/latest/usb/usbip_protocol.html
- WebAuthn spec: https://www.w3.org/TR/webauthn-2/
