# Bluetooth Low Energy (BLE) GATT Server Implementation

## Library Choice

**ble-peripheral-rust v0.2** - Cross-platform BLE peripheral/server library

### Platform Support

✅ **Windows**: Native WinRT Bluetooth API
✅ **macOS/iOS**: Native CoreBluetooth framework via objc2
✅ **Linux**: BlueZ via D-Bus (using bluer backend)

### Why ble-peripheral-rust?

- ✅ **True cross-platform peripheral mode** - Only Rust library with production backends on all platforms
- ✅ **Clean async API** - Tokio-based with event-driven architecture
- ✅ **Reuses mature libraries** - bluer (Linux), native OS APIs (Windows/macOS)
- ✅ **GATT server support** - Full service/characteristic management
- ⚠️ **Early maturity** (v0.2, ~3K downloads) - Experimental status appropriate
- 📚 Documentation: https://crates.io/crates/ble-peripheral-rust

### Alternatives Considered

| Library | Peripheral Support | Cross-Platform | Verdict |
|---------|-------------------|----------------|---------|
| btleplug | ❌ Central only | ✅ Win/Mac/Linux | Wrong role |
| bluest | ❌ Central only | ✅ Win/Mac/Linux | Wrong role |
| bluer | ✅ Full GATT server | ❌ Linux only | Platform-limited |
| btle | ⚠️ WIP | ⚠️ Win/Linux | Not production-ready |
| bluster | ✅ Peripheral | ✅ Mac/Linux | Abandoned (2+ years) |
| SimpleBLE | ✅ C++ only | ✅ All platforms | Rust bindings = central only |

**Conclusion**: ble-peripheral-rust is the only viable cross-platform peripheral option in pure Rust.

### System Dependencies

**Linux (BlueZ)**:
```bash
# Ubuntu/Debian
sudo apt install bluez libdbus-1-dev pkg-config

# Fedora/RHEL
sudo dnf install bluez dbus-devel pkgconf-pkg-config

# Start bluetoothd daemon
sudo systemctl start bluetooth
```

**macOS**:
- macOS Big Sur (11)+ requires app bundle with `Info.plist` containing `NSBluetoothAlwaysUsageDescription` for production
- Development: May need manual permission grants or notarization
- No additional system dependencies required

**Windows**:
- Windows 10+ with Bluetooth support
- No additional system dependencies required
- Uses native WinRT Bluetooth LE APIs

## Architecture

### BLE GATT Hierarchy

```
Peripheral (Device)
├── Service (e.g., Heart Rate Service 0x180D)
│   ├── Characteristic (e.g., HR Measurement 0x2A37)
│   │   ├── Properties: [read, notify]
│   │   ├── Permissions: [readable]
│   │   ├── Value: [0x00, 0x48] (72 BPM)
│   │   └── Descriptors: [...]
│   └── Characteristic (e.g., Body Sensor Location 0x2A38)
│       ├── Properties: [read]
│       ├── Value: [0x01] (Chest)
└── Service (Battery Service 0x180F)
    └── Characteristic (Battery Level 0x2A19)
        ├── Properties: [read, notify]
        └── Value: [0x5F] (95%)
```

### LLM Integration Points

The LLM has full control over the BLE GATT server:

#### 1. Server Initialization (Event-triggered)
**Event**: `bluetooth_server_started`
- Triggered when server starts and adapter is powered on
- LLM decides initial services to create
- LLM configures advertising parameters

**LLM Actions**:
```json
{
  "type": "add_service",
  "uuid": "0000180d-0000-1000-8000-00805f9b34fb",
  "primary": true,
  "characteristics": [
    {
      "uuid": "00002a37-0000-1000-8000-00805f9b34fb",
      "properties": ["read", "notify"],
      "permissions": ["readable"],
      "initial_value": "0048"
    }
  ]
}
```

#### 2. Service Management (Async Actions)
- **add_service**: Create new GATT service with characteristics
- **start_advertising**: Begin advertising (make device discoverable)
- **stop_advertising**: Stop advertising

**Example**:
```json
{
  "type": "start_advertising",
  "device_name": "MyHeartRateMonitor"
}
```

#### 3. Read Requests (Sync Actions - Event Response)
**Event**: `bluetooth_read_request`
```json
{
  "event": "bluetooth_read_request",
  "characteristic_uuid": "00002a37-0000-1000-8000-00805f9b34fb",
  "offset": 0
}
```

**LLM Response**:
```json
{
  "type": "respond_to_read",
  "value": "0048"  // Hex-encoded: 72 in decimal
}
```

#### 4. Write Requests (Sync Actions - Event Response)
**Event**: `bluetooth_write_request`
```json
{
  "event": "bluetooth_write_request",
  "characteristic_uuid": "00002a39-0000-1000-8000-00805f9b34fb",
  "value": "01",
  "offset": 0,
  "with_response": true
}
```

**LLM Response**:
```json
{
  "type": "respond_to_write",
  "status": "success"
}
```

#### 5. Notifications (Async Actions)
**Action**: `send_notification`
- Push data to subscribed clients
- Used for periodic updates (heart rate, temperature, etc.)

```json
{
  "type": "send_notification",
  "characteristic_uuid": "00002a37-0000-1000-8000-00805f9b34fb",
  "value": "0049"  // Updated value
}
```

#### 6. Subscription Events
**Event**: `bluetooth_subscribe`
- Client subscribes to or unsubscribes from notifications
- LLM can start/stop periodic updates

```json
{
  "event": "bluetooth_subscribe",
  "characteristic_uuid": "00002a37-0000-1000-8000-00805f9b34fb",
  "subscribed": true
}
```

### State Management

**Server-Level State**:
- `ConnectionState`: Idle/Processing/Accumulating (prevents concurrent LLM calls)
- `memory`: LLM conversation memory
- `characteristics`: Tracked characteristic metadata and current values
- `queued_events`: Events queued during Processing state

**No Connection Tracking**:
- BLE peripheral mode is broadcast-based
- Multiple clients can connect, but ble-peripheral-rust abstracts this
- Server responds to all clients uniformly

### Event Flow

```
1. Server Startup:
   Peripheral::new() → Wait for powered on
   → bluetooth_server_started event
   → LLM adds services (add_service actions)
   → LLM starts advertising (start_advertising action)

2. Client Scans & Connects:
   BLE advertising → Client discovers device → Client connects
   (Connection is transparent to server)

3. Client Reads Characteristic:
   ReadRequest event from ble-peripheral-rust
   → State: Idle → Processing
   → bluetooth_read_request event to LLM
   → LLM responds with respond_to_read action
   → Send response via responder
   → State: Processing → Idle

4. Client Subscribes to Notifications:
   SubscribeNotifications event
   → bluetooth_subscribe event to LLM
   → LLM starts periodic send_notification actions

5. Periodic Notifications (Scheduled Tasks):
   schedule_task(interval=2s, send_notification)
   → LLM sends updated values every 2 seconds
   → Subscribed clients receive updates

6. Client Writes Characteristic:
   WriteRequest event with data
   → State: Idle → Processing
   → Store value in characteristic data
   → bluetooth_write_request event to LLM
   → LLM processes write
   → Send acknowledgment if with_response=true
   → State: Processing → Idle
```

### Dual Logging

All BLE events are logged to both `netget.log` (via `tracing`) and the TUI (via `status_tx`):

- **INFO**: Server started, advertising started/stopped, subscriptions
- **DEBUG**: Read/write requests with characteristic UUIDs, notification sends
- **TRACE**: Full hex-encoded data payloads
- **ERROR**: LLM failures, peripheral errors, invalid UUIDs

Example:
```
[INFO] Bluetooth adapter powered on
[INFO] Added BLE service 0000180d-0000-1000-8000-00805f9b34fb with 2 characteristics
[INFO] Started BLE advertising as 'NetGet-HeartRate'
[DEBUG] BLE read request on 00002a37-0000-1000-8000-00805f9b34fb (offset: 0)
[DEBUG] Sent BLE notification on 00002a37-0000-1000-8000-00805f9b34fb (2 bytes)
[TRACE] BLE write data (hex): 01
```

## Data Format

All BLE data exchanged with LLM is **hex-encoded strings**, not raw bytes:

### UUIDs
- **Standard 16-bit**: `"180D"` → expanded to `0000180d-0000-1000-8000-00805f9b34fb`
- **Full 128-bit**: `"0000180d-0000-1000-8000-00805f9b34fb"`

### Values
- **Hex-encoded**: `"0048"` = [0x00, 0x48] = 72 in decimal
- **Leading zeros**: Important for proper byte representation
- **Optional 0x prefix**: `"0x0048"` and `"0048"` both work

**Why hex-encoded?**
- LLMs understand hex better than base64 for small values
- Direct mapping to Bluetooth SIG specifications
- Easy to construct multi-byte values (e.g., heart rate: flags + BPM)

### Example: Heart Rate Measurement

Standard Bluetooth SIG format:
```
Byte 0: Flags (0x00 = uint8 format, no sensor contact)
Byte 1: BPM value (0x48 = 72)
```

LLM constructs:
```json
{
  "value": "0048"  // Flags: 0x00, BPM: 0x48
}
```

## Common GATT Services

LLMs are familiar with standard Bluetooth SIG GATT services:

### Heart Rate Service (0x180D)
- **Characteristic 0x2A37**: Heart Rate Measurement (notify)
  - Byte 0: Flags
  - Byte 1+: BPM value

### Battery Service (0x180F)
- **Characteristic 0x2A19**: Battery Level (read, notify)
  - Single byte: 0-100 percentage

### Device Information (0x180A)
- **Characteristic 0x2A29**: Manufacturer Name (read)
- **Characteristic 0x2A24**: Model Number (read)
- **Characteristic 0x2A26**: Firmware Revision (read)

### Environmental Sensing (0x181A)
- **Characteristic 0x2A6E**: Temperature (read, notify)
- **Characteristic 0x2A6F**: Humidity (read, notify)

Full specifications: https://www.bluetooth.com/specifications/assigned-numbers/

## Limitations

### BLE Only (No Bluetooth Classic)
- Cannot emulate Bluetooth Classic devices (speakers, mice, keyboards)
- Only Bluetooth Low Energy (BLE/Bluetooth 4.0+)
- Use case: IoT sensors, fitness trackers, smart home devices

### Platform-Specific Behavior

**macOS**:
- Production apps require .app bundle with Info.plist
- Development may need manual permission grants
- CoreBluetooth restrictions apply

**Linux**:
- Requires `bluetoothd` daemon running
- User may need to be in `bluetooth` group or use sudo
- BlueZ version 5.50+ recommended

**Windows**:
- Windows 10+ required
- Native Bluetooth LE adapter required (no USB dongles tested)
- Some adapters may have driver issues

### ble-peripheral-rust Maturity
- **Version 0.2.0** (early development)
- Only ~3,000 total downloads
- Limited documentation and examples
- Platform-specific bugs possible

**Mitigation**:
- Mark protocol as `Experimental`
- Comprehensive error logging
- Document known issues as discovered
- Can contribute fixes upstream (small codebase)

### No Connection-Level Control
- ble-peripheral-rust abstracts connection management
- Cannot accept/reject individual client connections
- Cannot distinguish between multiple connected clients
- All clients receive the same data

### Advertising Limitations
- Basic advertising only (device name, service UUIDs)
- No manufacturer data customization
- No beacon protocols (iBeacon, Eddystone) in v0.2

## Error Handling

### Adapter Not Powered On
- Server waits up to 10 seconds for adapter to power on
- Error if not powered after timeout
- User should check Bluetooth is enabled in system settings

### Invalid UUIDs
- LLM may provide malformed UUIDs
- Server validates and returns error to LLM
- LLM can retry with corrected UUID

### Read/Write Failures
- LLM call failure → return error response to client
- Client sees "Unlikely Error" status
- Logged with ERROR level

### Platform-Specific Errors
- BlueZ D-Bus errors (Linux)
- CoreBluetooth errors (macOS)
- WinRT errors (Windows)
- All logged with full error messages

## Testing Considerations

### Real Hardware Required
- E2E tests need real BLE adapter or simulator
- Simulated BLE devices:
  - **Linux**: `bluetoothctl` with virtual devices
  - **macOS**: Xcode's Bluetooth Simulator (iOS/tvOS targets)
  - **Windows**: Limited simulator options
  - **Cross-platform**: Nordic nRF Connect app for testing

### Test Strategies

1. **Unit tests**: Action parsing, EventType construction (no BLE hardware)
2. **Integration tests**: Mock ble-peripheral-rust if possible
3. **E2E tests**: Real BLE client (nRF Connect, smartphone app, btleplug)

### LLM Call Budget
- E2E tests should minimize LLM calls (< 10 per suite)
- Use scripting mode for predictable sequences
- Test one service type per test case

### CI/CD Challenges
- GitHub Actions runners don't have Bluetooth adapters
- E2E tests must be run locally or on special runners
- Consider marking E2E tests as `#[ignore]` for CI

## Example Usage Scenarios

### Scenario 1: Heart Rate Monitor
```
User: "Act as a BLE heart rate monitor. Start at 72 BPM and increase by 1 every 2 seconds."

LLM Actions:
1. add_service(uuid="180D", characteristics=[{ uuid="2A37", properties=["notify"], ... }])
2. start_advertising(device_name="NetGet-HR")
3. [Client subscribes to notifications]
4. send_notification(uuid="2A37", value="0048")  // 72 BPM
5. [2 seconds later]
6. send_notification(uuid="2A37", value="0049")  // 73 BPM
... continues
```

### Scenario 2: Temperature Sensor
```
User: "Pretend to be a BLE thermometer. Start at 20°C, simulate random variations."

LLM Actions:
1. add_service(uuid="181A", characteristics=[{ uuid="2A6E", properties=["read", "notify"], ... }])
2. start_advertising(device_name="NetGet-Temp")
3. [Client reads characteristic]
4. bluetooth_read_request → respond_to_read(value="0C80")  // 20.0°C in Bluetooth format
5. [Client subscribes]
6. send_notification(uuid="2A6E", value="0C85")  // 20.5°C
```

### Scenario 3: Custom Interactive Device
```
User: "Create a BLE-controlled LED strip. Reading state returns on/off, writing 0x01 turns on."

LLM Actions:
1. add_service(uuid="12345678-1234-5678-1234-567812345678", characteristics=[
     { uuid="...-0001", properties=["read", "write"], ... }  // State
   ])
2. start_advertising(device_name="NetGet-LED")
3. [Client writes 0x01]
4. bluetooth_write_request(value="01") → LLM updates internal state
5. [Client reads state]
6. bluetooth_read_request → respond_to_read(value="01")  // ON
```

## Future Enhancements

- **Manufacturer data**: Custom advertising payload
- **Connection parameters**: Request faster/slower intervals
- **Bonding/pairing**: Secure connections with PIN/passkey
- **Descriptors**: Client Characteristic Configuration Descriptor (CCCD) control
- **MTU negotiation**: Larger data transfers
- **Multiple services**: Dynamic service add/remove
- **Beacon protocols**: iBeacon, Eddystone support

## Known Issues

- ble-peripheral-rust v0.2 is early development, expect bugs
- macOS may require app bundle for full functionality
- Windows adapter compatibility varies
- No connection-level granularity (all clients treated uniformly)
- Advertising customization limited

## References

- ble-peripheral-rust: https://crates.io/crates/ble-peripheral-rust
- Bluetooth SIG GATT specifications: https://www.bluetooth.com/specifications/specs/
- Bluetooth Core Specification: https://www.bluetooth.com/specifications/bluetooth-core-specification/
- BlueZ (Linux): http://www.bluez.org/
- CoreBluetooth (macOS/iOS): https://developer.apple.com/documentation/corebluetooth
- Standard UUIDs: https://www.bluetooth.com/specifications/assigned-numbers/
