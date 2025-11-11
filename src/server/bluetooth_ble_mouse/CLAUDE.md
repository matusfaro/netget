# BLE HID Mouse Implementation

## Overview

Bluetooth Low Energy (BLE) HID mouse server that allows devices to pair with NetGet and receive mouse movements. Built
on top of the `bluetooth-ble` protocol, providing a high-level mouse interface with connection tracking and targeted
messaging.

## Architecture

### Layered Design

```
bluetooth-ble-mouse (High-level)
    ↓
bluetooth-ble (Low-level GATT)
    ↓
ble-peripheral-rust (Platform backends)
```

### Connection Tracking

Like the keyboard, the mouse tracks individual client connections for targeted messaging.

## HID over GATT Profile

### Service Structure

- **Service UUID**: `0x1812` (HID Service)
- **Characteristics**: Same as keyboard but with mouse report descriptor

### HID Report Descriptor

Standard HID mouse with:

- 3 buttons (left, right, middle)
- Relative X/Y movement
- Scroll wheel

### Report Format (4 bytes)

```
[0]: Buttons (bit 0=left, bit 1=right, bit 2=middle)
[1]: X movement (-127 to 127)
[2]: Y movement (-127 to 127)
[3]: Wheel scroll (-127 to 127)
```

**Example**: Move right 10 pixels, no buttons

```
00 0A 00 00
││ ││ ││ ││
││ ││ ││ └─ Wheel: 0
││ ││ └─── Y: 0
││ └───── X: +10
└─────── Buttons: none
```

**Example**: Left click at current position

```
01 00 00 00
││
└─ Button 1 (left)
```

## LLM Actions

### move_cursor

Move the cursor by relative amounts.

```json
{
  "type": "move_cursor",
  "dx": 10,
  "dy": -5,
  "client_id": 1  // Optional
}
```

### click

Click a mouse button.

```json
{
  "type": "click",
  "button": "left"
}
```

Buttons: `left`, `right`, `middle`

### scroll

Scroll the mouse wheel.

```json
{
  "type": "scroll",
  "amount": 3  // Positive=up, negative=down
}
```

### drag

Drag with button held.

```json
{
  "type": "drag",
  "button": "left",
  "dx": 50,
  "dy": 30
}
```

### send_to_client

Send to a specific client.

```json
{
  "type": "move_cursor",
  "dx": 10,
  "dy": 0,
  "client_id": 2
}
```

### list_clients

List connected clients.

```json
{
  "type": "list_clients"
}
```

## Events

### mouse_client_connected

```json
{
  "event": "mouse_client_connected",
  "client_id": 1
}
```

### mouse_client_disconnected

```json
{
  "event": "mouse_client_disconnected",
  "client_id": 1
}
```

## Example Usage

### Draw a Circle

```
User: "Act as a Bluetooth mouse. When a device connects, move the cursor in a circle."

LLM:
move_cursor(10, 0)
move_cursor(7, 7)
move_cursor(0, 10)
move_cursor(-7, 7)
move_cursor(-10, 0)
move_cursor(-7, -7)
move_cursor(0, -10)
move_cursor(7, -7)
```

### Click and Drag

```
User: "Click and drag from current position 100 pixels to the right"

LLM: drag("left", 100, 0)
```

### Scroll Page

```
User: "Scroll down 5 notches"

LLM: scroll(-5)
```

## Limitations

- **BLE only**: No Bluetooth Classic
- **Relative movement**: No absolute positioning
- **127-pixel limit**: Large movements need multiple reports
- **3 buttons only**: No extra buttons (forward/back)
- **No acceleration**: Linear movement only

## Platform Requirements

Same as `bluetooth-ble`:

- **Linux**: BlueZ daemon
- **macOS**: Bluetooth enabled
- **Windows**: Windows 10+ with Bluetooth

## References

- HID over GATT Profile: https://www.bluetooth.com/specifications/specs/hid-over-gatt-profile-1-0/
- USB HID Usage Tables: https://www.usb.org/sites/default/files/documents/hut1_12v2.pdf
