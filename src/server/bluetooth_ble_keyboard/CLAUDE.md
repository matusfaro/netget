# BLE HID Keyboard Implementation

## Overview

Bluetooth Low Energy (BLE) HID keyboard server that allows devices to pair with NetGet and receive keypresses. Built on
top of the `bluetooth-ble` protocol, this provides a high-level keyboard interface with connection tracking and targeted
messaging.

## Architecture

### Layered Design

```
bluetooth-ble-keyboard (High-level)
    ↓
bluetooth-ble (Low-level GATT)
    ↓
ble-peripheral-rust (Platform backends)
```

### Connection Tracking

Unlike the base `bluetooth-ble` protocol, the keyboard tracks individual client connections:

```rust
pub struct ClientConnection {
    pub id: ClientId,
    pub connected_at: std::time::Instant,
}
```

This allows:

- **Targeted messages**: Send keypresses to specific clients
- **Per-client state**: Track which devices are connected
- **Connection events**: Notify LLM when clients connect/disconnect

## HID over GATT Profile

### Service Structure

- **Service UUID**: `0x1812` (HID Service)
- **Characteristics**:
    - `0x2A4D` - HID Report Map (keyboard layout descriptor)
    - `0x2A4B` - HID Report (input reports - keypresses)
    - `0x2A4A` - HID Information
    - `0x2A4C` - HID Control Point

### HID Report Descriptor

The keyboard uses a standard USB HID report descriptor:

- **Modifier byte**: Ctrl, Shift, Alt, GUI
- **Reserved byte**: Always 0x00
- **Key array**: Up to 6 simultaneous keys

### Report Format (8 bytes)

```
[0]: Modifiers (bit flags)
[1]: Reserved (0x00)
[2-7]: Key codes (up to 6 keys)
```

**Example**: Pressing 'A' with Shift

```
02 00 04 00 00 00 00 00
││ ││ ││
││ ││ └─ Key code 0x04 ('A')
││ └─── Reserved
└───── Modifier 0x02 (Left Shift)
```

## LLM Actions

### type_text

Type a string of text, converting to HID reports automatically.

```json
{
  "type": "type_text",
  "text": "Hello, World!",
  "client_id": 1  // Optional: target specific client
}
```

### press_key

Press a single special key.

```json
{
  "type": "press_key",
  "key": "enter"
}
```

Supported keys: `enter`, `escape`, `tab`, `backspace`, `delete`, arrow keys, function keys, etc.

### key_combo

Press key combinations.

```json
{
  "type": "key_combo",
  "modifiers": ["ctrl"],
  "key": "c"
}
```

### send_to_client

Send to a specific connected client.

```json
{
  "type": "type_text",
  "text": "Message for you",
  "client_id": 2
}
```

### list_clients

Get all connected client IDs.

```json
{
  "type": "list_clients"
}
```

## Events

### keyboard_client_connected

Fired when a device pairs and connects.

```json
{
  "event": "keyboard_client_connected",
  "client_id": 1
}
```

### keyboard_client_disconnected

Fired when a device disconnects.

```json
{
  "event": "keyboard_client_disconnected",
  "client_id": 1
}
```

## Example Usage

### Basic Typing

```
User: "Act as a Bluetooth keyboard. Type 'Hello from NetGet!'"

LLM: type_text("Hello from NetGet!")
```

### Multi-Client Scenario

```
User: "Act as a keyboard. When clients connect, greet them individually."

[Client 1 connects]
LLM: type_text("Welcome, Client 1!", client_id=1)

[Client 2 connects]
LLM: type_text("Welcome, Client 2!", client_id=2)
```

### Keyboard Shortcuts

```
User: "Send Ctrl+C to all connected devices"

LLM: key_combo(modifiers=["ctrl"], key="c")
```

## Limitations

- **BLE only**: No Bluetooth Classic support
- **6-key rollover**: Maximum 6 simultaneous keys (USB HID standard)
- **US keyboard layout**: Key mapping assumes US QWERTY
- **No key state**: Cannot query current pressed keys
- **Platform differences**: Pairing UX varies by OS

## Platform Requirements

Same as `bluetooth-ble`:

- **Linux**: BlueZ daemon
- **macOS**: Bluetooth enabled, may need app bundle
- **Windows**: Windows 10+ with Bluetooth

## Testing

E2E tests require real devices to pair as keyboards. See `tests/server/bluetooth_ble_keyboard/CLAUDE.md`.

## References

- HID over GATT Profile: https://www.bluetooth.com/specifications/specs/hid-over-gatt-profile-1-0/
- USB HID Usage Tables: https://www.usb.org/sites/default/files/documents/hut1_12v2.pdf
