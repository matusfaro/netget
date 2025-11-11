# USB Keyboard E2E Testing

## Test Strategy

The USB keyboard E2E tests verify the virtual HID keyboard device by using real Linux `usbip` client tools to attach the
device and read keyboard events.

## Test Environment Requirements

### System Requirements

- **OS**: Linux (kernel 3.17+ with vhci-hcd support)
- **Tools**: `usbip`, `evtest` (or similar event reader)
- **Privileges**: Root access for `usbip attach` and `/dev/input` access
- **Network**: Localhost (127.0.0.1) - no external endpoints

### Installation

```bash
# Ubuntu/Debian
sudo apt-get install usbip evtest

# Fedora/RHEL
sudo dnf install usbip-utils evtest

# Load kernel module
sudo modprobe vhci-hcd
```

## Test Approach

### Unit Tests

**Current**: None (protocol implementation not complete)
**Future**: Test descriptor builders, HID report generation, character mapping

### E2E Tests

**Current**: Placeholder (waiting for USB/IP protocol integration)
**Future**: Real client tests using usbip tools

## Planned E2E Test Cases

### Test 1: Device Enumeration

**Objective**: Verify device appears in usbip list

```bash
# Start server
netget server usb-keyboard 127.0.0.1:3240

# List devices
usbip list -r 127.0.0.1 -p 3240

# Expected output:
#   Exportable USB devices
#   ======================
#    - 127.0.0.1:3240
#        1-1: NetGet Virtual Keyboard
#             : HID / Boot Interface Subclass / Keyboard (03/01/01)
```

**LLM Calls**: 0 (no LLM needed for enumeration)

### Test 2: Device Attachment

**Objective**: Verify device can be attached and appears as /dev/input/eventX

```bash
# Start server
netget server usb-keyboard 127.0.0.1:3240

# Attach device
sudo usbip attach -r 127.0.0.1 -p 3240 -b 1-1

# Verify device attached
lsusb | grep "NetGet"
ls /dev/input/by-id/ | grep keyboard
```

**LLM Calls**: 1 (device attached event)

### Test 3: Simple Typing

**Objective**: Verify LLM can type text

**Setup**:

```bash
# Start server with LLM instruction
netget server usb-keyboard 127.0.0.1:3240 --instruction "Type 'test123' when keyboard is attached"

# Attach and monitor events
sudo usbip attach -r 127.0.0.1 -p 3240 -b 1-1
sudo evtest /dev/input/eventX
```

**Expected Events**:

- Key press events for: t, e, s, t, 1, 2, 3
- Each key: press event, release event
- Correct HID usage codes

**LLM Calls**: 1 (on attach, executes type_text action)

### Test 4: Key Combinations

**Objective**: Verify modifier keys (Ctrl, Shift, Alt)

**Setup**:

```bash
# Test Ctrl+C
netget server usb-keyboard 127.0.0.1:3240 --instruction "Press Ctrl+C when attached"
```

**Expected Events**:

- Key press: Left Control (modifier)
- Key press: C
- Key release: C
- Key release: Left Control

**LLM Calls**: 1

### Test 5: LED Status Feedback

**Objective**: Verify server receives LED status from host

**Setup**:

```bash
# Start server with LED monitoring
netget server usb-keyboard 127.0.0.1:3240 --instruction "Report LED status changes"

# Attach device
sudo usbip attach -r 127.0.0.1 -p 3240 -b 1-1

# Toggle Caps Lock on host
xdotool key Caps_Lock
```

**Expected**:

- Server logs LED status change (Caps Lock ON)
- LLM receives usb_keyboard_led_status event

**LLM Calls**: 2 (attach + LED status)

## LLM Call Budget

**Target**: < 10 LLM calls per full test suite

### Breakdown:

- Device enumeration: 0 calls (no LLM)
- Device attachment: 1 call (on connect)
- Simple typing: 1 call (execute type_text)
- Key combinations: 1 call (execute press_key_combo)
- LED status: 2 calls (attach + LED event)
- **Total**: ~5 calls

### Optimization Strategies:

1. **Scripting Mode**: Use deterministic behavior, no LLM needed for predictable actions
2. **Single Server Instance**: Reuse server across multiple test cases
3. **Mock Mode**: Test protocol without LLM for unit tests
4. **Batch Actions**: Test multiple keypresses in single LLM call

## Expected Runtime

**Per Test**:

- Device enumeration: < 1 second
- Device attachment: < 2 seconds
- Typing test: < 5 seconds (depends on typing speed)
- Key combo test: < 2 seconds
- LED test: < 3 seconds

**Full Suite**: < 15 seconds (excluding LLM warm-up)

## Known Issues / Flaky Tests

### Current Status: No Tests Yet

Once tests are implemented, document any flaky behavior here.

### Potential Issues:

1. **vhci-hcd Not Loaded**: Tests fail if kernel module missing
2. **Port Conflicts**: Need unique port for each test or sequential execution
3. **Root Privileges**: Tests require sudo (may need CI configuration)
4. **evtest Timing**: Need to wait for device ready before reading events
5. **Device Cleanup**: Must detach devices after tests to avoid conflicts

## Running Tests

### Full Suite

```bash
# Build with USB keyboard feature
./cargo-isolated.sh build --no-default-features --features usb-keyboard

# Run E2E tests (requires root for usbip)
sudo ./cargo-isolated.sh test --no-default-features --features usb-keyboard \
  --test usb_keyboard_e2e
```

### Single Test

```bash
sudo ./cargo-isolated.sh test --no-default-features --features usb-keyboard \
  --test usb_keyboard_e2e -- test_keyboard_typing
```

### Debug Mode

```bash
# Enable trace logging
RUST_LOG=netget=trace sudo ./cargo-isolated.sh test \
  --no-default-features --features usb-keyboard \
  --test usb_keyboard_e2e
```

## CI Considerations

### Docker/Container

- Need privileged container for vhci-hcd module
- Or use VM with full kernel access
- Alternative: Mock tests without real usbip

### Permissions

- Tests require root/sudo
- May need special CI runner configuration
- Consider marking tests as manual/optional

## Future Improvements

1. **Mock USB/IP Client**: Avoid requiring real usbip tools
2. **Automated Event Verification**: Parse evtest output programmatically
3. **Performance Tests**: Measure typing latency, throughput
4. **Stress Tests**: Multiple simultaneous connections, rapid attach/detach
5. **Compatibility Tests**: Test with different Linux distros/kernels

## References

- **usbip man page**: `man usbip`
- **evtest**: https://gitlab.freedesktop.org/libevdev/evtest
- **Linux Input Subsystem**: https://www.kernel.org/doc/html/latest/input/input.html
- **vhci-hcd**: https://docs.kernel.org/usb/usbip_protocol.html#vhci
