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

## Current Status: Experimental (USB/IP Integrated)

### What Works
- ✅ Protocol registration and discovery
- ✅ Action/event definitions
- ✅ HID descriptor builders (mouse report descriptor)
- ✅ Server trait implementation (spawn, execute_action)
- ✅ TCP listener for USB/IP connections
- ✅ USB/IP server integration using usbip crate
- ✅ UsbHidMouseHandler from usbip crate
- ✅ Device creation with HID mouse interface
- ✅ LLM action execution (move_relative, click, scroll)
- ✅ Mouse event queue (pending_mouse_events)

### What's Limited (Known Issues)
- ⚠️ **Build Requirement**: Requires libusb-1.0-dev to compile
- ⚠️ **move_absolute**: Not yet implemented (requires position tracking)
- ⚠️ **drag**: Not yet implemented (requires smooth movement + position tracking)
- ⚠️ **Relative Movements Only**: HID boot protocol uses relative positioning
- ⚠️ **Testing**: Not yet tested with real usbip client

### Implementation Status

**Phase 1 Complete** (USB/IP Integration):
1. ✅ Integrated usbip crate (v0.3)
2. ✅ Device export using UsbIpServer::new_simulated()
3. ✅ Handler via usbip::hid::UsbHidMouseHandler
4. ✅ Endpoint setup (interrupt IN endpoint 0x81)
5. ✅ URB processing (handled by usbip crate)
6. ✅ LLM action execution (move_relative, click, scroll convert to HID reports)

## Build

Same requirements as usb-keyboard (libusb-1.0-dev).

```bash
# Ubuntu/Debian
sudo apt-get install libusb-1.0-0-dev pkg-config

# Build with USB mouse feature
./cargo-isolated.sh build --no-default-features --features usb-mouse
```

## OS Support

- **Server**: Linux/macOS/Windows (theoretically)
- **Client**: **Linux only** (requires vhci-hcd kernel module)
