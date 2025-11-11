# BLE Remote Control E2E Tests

## Test Strategy

BLE remote control is a simple HID Consumer Control device with button presses. Testing validates button press events
are sent correctly.

### Test Approach

**Black-box testing**: Tests validate remote behavior by:

- Connecting as BLE central
- Pairing with remote server
- Subscribing to HID report notifications
- Validating button press reports

### Client Library

Using **btleplug** as BLE central for testing.

## Test Cases

### 1. Play/Pause Button Test

**LLM Budget**: 2 calls (start server, press play/pause)

Validates:

- HID service (0x1812) is advertised
- Report descriptor matches Consumer Control format
- Play/pause button press sends correct report (0x01 0x00)

### 2. Volume Control Test

**LLM Budget**: 3 calls (start server, volume up, volume down)

Validates:

- Volume up button (bit 6)
- Volume down button (bit 7)
- Button release (0x00 0x00)

### 3. Multiple Button Sequence Test

**LLM Budget**: 4 calls (start server, multiple buttons)

Validates:

- Sequential button presses work correctly
- No button state interference
- Each button press followed by release

## LLM Call Budget

**Total**: < 10 LLM calls across all tests

- Server startup: 1 call (shared)
- Button press actions: 6-8 calls

## Expected Runtime

- **Per test**: 3-5 seconds
- **Total suite**: 15-25 seconds

## Test Environment Requirements

### Hardware

- **BLE adapter** required
- **Permissions**: No special permissions

### Platform Support

- **Linux**: BlueZ
- **macOS**: Bluetooth enabled
- **Windows**: Windows 10+ with Bluetooth

### CI/CD Considerations

- Tests marked `#[ignore]` (require BLE hardware)
- Server startup test runs without hardware

## Known Issues

### Platform Differences

- **Windows**: May require pairing dialog
- **macOS**: System Bluetooth preferences may interfere
- **Linux**: BlueZ HID plugin must be enabled

## Limitations

- **No connection tracking**: Cannot test per-client messaging
- **No button combinations**: Cannot press multiple buttons simultaneously
- **No long press**: Only momentary button presses supported
