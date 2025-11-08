# USB Mouse Server Implementation

## Overview

Virtual USB HID mouse using USB/IP protocol. LLM controls cursor movement, button clicks, and wheel scrolling.

## HID Mouse Protocol

**Report Format** (4 bytes):
- Byte 0: Buttons (bit 0: left, bit 1: right, bit 2: middle)
- Byte 1: X movement (-127 to 127, signed)
- Byte 2: Y movement (-127 to 127, signed)
- Byte 3: Wheel (-127 to 127, signed, vertical scroll)

## LLM Actions

**move_relative**: Move cursor by offset
```json
{"type": "move_relative", "x": 10, "y": -5}
```

**move_absolute**: Move to screen position (converted to relative movements)
```json
{"type": "move_absolute", "x": 960, "y": 540, "screen_width": 1920, "screen_height": 1080}
```

**click**: Press and release button
```json
{"type": "click", "button": "left"}
```

**scroll**: Scroll wheel
```json
{"type": "scroll", "direction": "up", "amount": 3}
```

**drag**: Move with button held
```json
{"type": "drag", "start_x": 100, "start_y": 100, "end_x": 200, "end_y": 200, "duration_ms": 500}
```

## LLM Events

- `usb_mouse_attached`: Device imported by host
- `usb_mouse_detached`: Device removed

## Status

**Experimental**: Framework complete, USB/IP integration needed.

## Build

Same requirements as usb-keyboard (libusb-1.0-dev).

```bash
./cargo-isolated.sh build --no-default-features --features usb-mouse
```
