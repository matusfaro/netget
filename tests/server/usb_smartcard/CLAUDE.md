# USB Smart Card E2E Tests

## Test Strategy

**Approach:** Black-box testing with OpenSC tools and PC/SC middleware

**Test Types:**

- Unit tests: None (APDU logic tested via E2E)
- E2E tests: USB/IP attachment + OpenSC/PKCS#11 tools
- PIV tests: PIV applet operations (when implemented)

## LLM Call Budget

**Target:** < 5 LLM calls total
**Current:** 0 (tests use real PC/SC tools, not LLM)

**Rationale:**

- Smart Card protocol doesn't require LLM for operation
- Tests verify USB CCID and cryptographic correctness
- No prompt-driven behavior to test

## Test Environment

### System Requirements

```bash
# Ubuntu/Debian
sudo apt-get install libusb-1.0-0-dev pkg-config usbip pcscd pcsc-tools \
    opensc opensc-pkcs11 libpcsclite-dev

# Fedora/RHEL
sudo dnf install libusb1-devel pkgconfig usbip pcsc-lite pcsc-tools \
    opensc opensc-pkcs11 pcsc-lite-devel

# macOS (USB/IP not available)
brew install opensc pcsc-lite
# Tests marked as ignored on macOS
```

### PC/SC Daemon

```bash
# Start pcscd daemon
sudo systemctl start pcscd

# Verify daemon running
sudo systemctl status pcscd

# Monitor card events (optional)
pcsc_scan
```

## Running Tests

### All E2E Tests (Requires Root)

```bash
# Build with Smart Card feature
./cargo-isolated.sh build --no-default-features --features usb-smartcard

# Run tests (requires sudo for usbip)
sudo -E ./cargo-isolated.sh test --no-default-features --features usb-smartcard --test usb_smartcard::e2e_test
```

### Individual Tests

```bash
# Test card detection
sudo -E cargo test --no-default-features --features usb-smartcard test_card_detection -- --ignored

# Test APDU exchange
sudo -E cargo test --no-default-features --features usb-smartcard test_apdu_exchange -- --ignored
```

## Expected Runtime

| Test             | Duration | LLM Calls |
|------------------|----------|-----------|
| Server startup   | 2s       | 0         |
| Card detection   | 5s       | 0         |
| APDU exchange    | 10s      | 0         |
| PIN verification | 10s      | 0         |
| RSA signing      | 15s      | 0         |
| Key generation   | 20s      | 0         |
| PKCS#11 access   | 15s      | 0         |
| PIV operations   | 30s      | 0         |

**Total:** ~2-3 minutes

## Known Issues

### Issue 1: Requires Root Access

**Problem:** usbip command requires root/sudo
**Workaround:** Run tests with `sudo -E`
**Fix:** Add udev rules for non-root usbip (future)

### Issue 2: pcscd Must Be Running

**Problem:** Tests fail if pcscd not running
**Workaround:** `sudo systemctl start pcscd` before testing
**Fix:** Tests should start pcscd automatically

### Issue 3: Linux-Only

**Problem:** USB/IP requires Linux kernel module
**Workaround:** Tests automatically ignored on non-Linux
**Fix:** None (inherent limitation)

### Issue 4: PIV Not Implemented

**Problem:** PIV tests will fail (PIV applet not yet implemented)
**Workaround:** Mark PIV tests as ignored until implemented
**Fix:** Implement PIV card application (Phase 1 roadmap)

## Test Coverage

**Current Coverage:** 0% (tests not yet implemented)

**Target Coverage:**

- ✅ USB CCID device detection
- ✅ ATR (Answer To Reset)
- ✅ Basic APDU exchange (SELECT, GET DATA)
- ✅ PIN verification
- ✅ RSA cryptography (INTERNAL_AUTHENTICATE)
- ✅ Key storage and retrieval
- ❌ PIV applet (not implemented)
- ❌ OpenPGP applet (not implemented)
- ❌ Full ISO 7816-4 file system (not implemented)

## Manual Testing

### Quick Manual Test

```bash
# Terminal 1: Start server
cargo run --no-default-features --features usb-smartcard -- --protocol usb-smartcard --listen 0.0.0.0:3240

# Terminal 2: Attach device
sudo modprobe vhci-hcd
sudo usbip list -r localhost
sudo usbip attach -r localhost:3240 -b 1-1

# Terminal 3: Test with OpenSC
pcsc_scan  # Should show virtual reader with card
opensc-tool --list-readers
opensc-tool --reader 0 --send-apdu 00:A4:00:0C:02:3F:00  # SELECT MF

# Test RSA signing
opensc-tool --reader 0 --send-apdu 00:88:00:9A:20:...  # INTERNAL_AUTHENTICATE

# Cleanup
sudo usbip detach -p 0
```

### PKCS#11 Testing

```bash
# List slots
pkcs11-tool --module /usr/lib/opensc-pkcs11.so --list-slots

# Login and list objects
pkcs11-tool --module /usr/lib/opensc-pkcs11.so --login --list-objects

# Test signing
pkcs11-tool --module /usr/lib/opensc-pkcs11.so --login --sign --id 9A --mechanism RSA-PKCS
```

## References

- OpenSC project: https://github.com/OpenSC/OpenSC
- CCID specification: https://www.usb.org/sites/default/files/DWG_Smart-Card_CCID_Rev110.pdf
- ISO 7816-4: https://www.iso.org/standard/77180.html
- PC/SC workgroup: https://pcscworkgroup.com/
- PKCS#11: https://docs.oasis-open.org/pkcs11/pkcs11-base/v2.40/pkcs11-base-v2.40.html
