# BLE Beacon E2E Tests

## Test Strategy

BLE beacons are **advertisement-only** devices that broadcast data without accepting connections. Testing requires a BLE
scanner to detect and parse advertising packets.

### Test Approach

**Black-box testing**: Tests validate beacon behavior by scanning for advertising packets and verifying:

- Beacon type (iBeacon, Eddystone-UID, Eddystone-URL, Eddystone-TLM)
- Advertising data format (Apple/Google standard compliance)
- TX power calibration values
- UUID/namespace/instance identifiers

### Client Library

Using **btleplug** as BLE central/scanner to detect beacons:

- Cross-platform (same as bluetooth-ble-client)
- Supports passive scanning (no connection required)
- Parses manufacturer-specific data and service data

## Test Cases

### 1. iBeacon Advertising Test

**LLM Budget**: 1 call (start server with beacon protocol)

**Note**: Uses `base_stack: "BLUETOOTH_BLE_BEACON"` (not "BluetoothBLE") to match the protocol's `stack_name()`.

Validates:

- Server starts without errors
- Beacon protocol is properly registered and accessible

### 2. Eddystone-UID Advertising Test

**LLM Budget**: 1 call (start server with beacon protocol)

**Note**: Uses `base_stack: "BLUETOOTH_BLE_BEACON"` to match the protocol's `stack_name()`.

Validates:

- Server starts without errors
- Beacon protocol handles Eddystone-UID configuration

### 3. Eddystone-URL Advertising Test

**LLM Budget**: 1 call (start server with beacon protocol)

**Note**: Uses `base_stack: "BLUETOOTH_BLE_BEACON"` to match the protocol's `stack_name()`.

Validates:

- Server starts without errors
- Beacon protocol handles Eddystone-URL configuration

## LLM Call Budget

**Total**: 3 LLM calls (one per test)

- iBeacon test: 1 call (server startup only)
- Eddystone-UID test: 1 call (server startup only)
- Eddystone-URL test: 1 call (server startup only)

**Optimization**: Tests use mocks and only verify that the beacon server starts successfully. Actual beacon advertising functionality is tested via the underlying bluetooth-ble protocol.

## Expected Runtime

- **Per test**: 5-10 seconds (LLM response + BLE scan duration)
- **Total suite**: 30-50 seconds

**Scan Duration**: BLE scanning requires 2-5 seconds to reliably detect advertising packets due to advertising
intervals (typically 100ms-1s).

## Test Environment Requirements

### Hardware

- **BLE adapter** required (USB dongle, built-in Bluetooth)
- **Permissions**: No special permissions (scanning only, no pairing)

### Platform Support

- **Linux**: BlueZ with D-Bus permissions
- **macOS**: Bluetooth enabled (system dialogs may appear)
- **Windows**: Windows 10+ with Bluetooth

### CI/CD Considerations

- Tests **cannot run** in headless CI without BLE hardware
- Mark as `#[ignore]` for CI, run manually on dev machines
- Consider mocking BLE advertising for unit tests

## Known Issues

### Scan Timing

- BLE advertising intervals vary (100ms-10s)
- Tests may need longer scan durations for reliability
- Retry logic recommended for flaky scanning

### Platform Differences

- **macOS**: System may cache advertising data, causing stale results
- **Windows**: BLE stack may not support all advertising data types
- **Linux**: Requires BlueZ daemon and D-Bus access

### Beacon Detection

- Multiple beacons advertising simultaneously may interfere
- Tests should use unique UUIDs/namespaces to avoid conflicts
- Stop previous beacon before starting next test

## Test Fixtures

### Beacon Scanner Helper

```rust
async fn scan_for_beacon(
    timeout: Duration,
    filter: impl Fn(&Advertisement) -> bool,
) -> Option<Advertisement>
```

Scans for BLE beacons matching a filter predicate.

### UUID/Namespace Generators

```rust
fn random_uuid() -> String
fn random_namespace() -> String
```

Generate unique identifiers to avoid beacon conflicts.

## Limitations

- **No connection testing**: Beacons don't accept connections
- **No RSSI accuracy**: Cannot validate TX power calibration without physical measurement
- **No Eddystone-EID**: Encrypted ephemeral IDs require key exchange (complex)
- **No interleaved advertising**: Cannot test multiple beacon types simultaneously

## References

- iBeacon Spec: https://developer.apple.com/ibeacon/
- Eddystone Spec: https://github.com/google/eddystone
- btleplug Library: https://github.com/deviceplug/btleplug
