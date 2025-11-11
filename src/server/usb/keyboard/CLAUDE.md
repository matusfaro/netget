# USB Keyboard Server Implementation

## Overview

The USB Keyboard server creates a virtual USB HID (Human Interface Device) keyboard using the USB/IP protocol. This
allows an LLM to control keyboard input on a remote system as if a physical keyboard were attached.

## Architecture

### USB/IP Protocol

- **What**: Network protocol that exports USB devices over TCP/IP
- **Why**: Allows creating virtual USB devices without kernel modules on the server side
- **How**: Server exports device via TCP, client imports with `usbip attach` command

```
┌─────────────────┐                    ┌──────────────────┐
│  NetGet Server  │                    │  Linux Client    │
│  (USB/IP)       │ ◄────── TCP ─────► │  (vhci-hcd)      │
│  Port: 3240     │                    │  usbip attach    │
└─────────────────┘                    └──────────────────┘
         │                                      │
         │ Creates virtual                     │ Sees as
         │ USB keyboard                        │ /dev/input/eventX
         ▼                                     ▼
    [HID Descriptors]                    [Real USB Device]
```

### Components

1. **Common Layer** (`src/server/usb/common.rs`)
    - USB/IP protocol constants and helpers
    - Device class codes, descriptor types
    - Request type/code definitions (standard, HID, CDC)
    - Logging utilities (hex dump, setup packet formatting)

2. **Descriptor Builders** (`src/server/usb/descriptors.rs`)
    - Device descriptor (vendor ID, product ID, device class)
    - HID keyboard report descriptor (boot protocol)
    - Configuration descriptor (interface, HID, endpoint)
    - String descriptors (manufacturer, product, serial)
    - Keyboard report structure (modifiers + 6 keys)
    - Character-to-HID-usage mapping

3. **Server Implementation** (`src/server/usb/keyboard/mod.rs`)
    - TCP server for USB/IP connections
    - Connection state machine (Idle/Processing/Accumulating)
    - Per-connection data (memory, LED status)
    - LLM integration hook (called on device attach)

4. **Protocol Actions** (`src/server/usb/keyboard/actions.rs`)
    - Action definitions (type_text, press_key, press_key_combo)
    - Event definitions (attached, detached, led_status)
    - Server trait implementation (spawn, execute_action)
    - Protocol metadata (experimental state, no privileges required)

## Build Requirements

### System Dependencies

The usbip crate requires `libusb-1.0` to be installed:

```bash
# Ubuntu/Debian
sudo apt-get install libusb-1.0-0-dev pkg-config

# Fedora/RHEL
sudo dnf install libusb1-devel pkgconfig

# macOS
brew install libusb pkg-config
```

**Build Command**:

```bash
./cargo-isolated.sh build --no-default-features --features usb-keyboard
```

## Library Choices

### Primary: `usbip` crate (v0.3)

**Repository**: https://github.com/jiegec/usbip
**License**: MIT
**Maturity**: Active development, API not finalized

**Why chosen**:

- Pure Rust implementation of USB/IP protocol
- No root privileges required on server side
- No kernel modules needed on server side
- Works on Linux/macOS/Windows (server)
- Cross-platform client support (Linux via vhci-hcd)

**Capabilities**:

- Device export/import handling
- URB (USB Request Block) processing
- Descriptor management
- Async/await support with tokio

**Limitations**:

- API stability: Marked as "not finalized", may have breaking changes
- Documentation: Relies heavily on examples
- Client requirements: Needs vhci-hcd kernel module and root access

**Alternatives considered**:

- **usb-gadget** crate: Requires root + kernel modules on server
- **Raw Gadget**: No Rust bindings, very low-level
- **usbip-device**: Alpha quality, development-only

## HID Keyboard Protocol

### Report Descriptor (Boot Protocol)

The keyboard implements USB HID boot protocol for maximum compatibility:

```
Byte 0: Modifiers (8 bits: L-Ctrl, L-Shift, L-Alt, L-GUI, R-Ctrl, R-Shift, R-Alt, R-GUI)
Byte 1: Reserved (always 0)
Bytes 2-7: Up to 6 simultaneous key presses (HID usage codes)
```

**Example**: Typing "a" with shift held:

```
[0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]
 ^^^^         ^^^^
 Shift        'a' key (usage 0x04)
```

### HID Usage Codes

Characters are mapped to HID keyboard usage codes (defined in USB HID specification):

- `a-z`: 0x04-0x1d
- `0-9`: 0x27, 0x1e-0x26
- Special keys: Enter (0x28), Escape (0x29), Backspace (0x2a), etc.
- Modifiers: Ctrl, Shift, Alt, GUI (Windows/Command key)

### LED Output Report

Host can set keyboard LEDs (1 byte):

- Bit 0: Num Lock
- Bit 1: Caps Lock
- Bit 2: Scroll Lock
- Bits 3-4: Reserved
- Bits 5-7: Padding

## LLM Integration

### Connection Flow

1. **Client Connects**: Linux host runs `sudo usbip attach -r <server_ip> -p 3240`
2. **Device Import**: USB/IP protocol exports virtual keyboard
3. **LLM Notified**: `usb_keyboard_attached` event sent to LLM with connection ID
4. **LLM Responds**: Can use actions like `type_text`, `press_key`, etc.
5. **URB Processing**: Server translates LLM actions to USB HID reports
6. **Host Reads**: Linux reads HID reports as keyboard input events

### LLM Actions

#### type_text

```json
{
  "type": "type_text",
  "text": "Hello, World!",
  "typing_speed_ms": 50
}
```

Converts text to sequence of HID reports with press/release cycles.

#### press_key

```json
{
  "type": "press_key",
  "key": "c",
  "modifiers": ["ctrl"]
}
```

Sends single keypress with optional modifiers (Ctrl+C).

#### press_key_combo

```json
{
  "type": "press_key_combo",
  "keys": ["ctrl", "alt", "delete"]
}
```

Presses multiple keys simultaneously (Ctrl+Alt+Delete).

#### release_all_keys

```json
{
  "type": "release_all_keys"
}
```

Releases all currently pressed keys (emergency reset).

### LLM Events

#### usb_keyboard_attached

Triggered when Linux host imports the device.

```json
{
  "type": "usb_keyboard_attached",
  "connection_id": "conn_123"
}
```

#### usb_keyboard_led_status

Triggered when host changes LED state (Caps Lock, Num Lock, etc.).

```json
{
  "type": "usb_keyboard_led_status",
  "connection_id": "conn_123",
  "num_lock": false,
  "caps_lock": true,
  "scroll_lock": false
}
```

## Current Status: Experimental (USB/IP Integrated)

### What Works

- ✅ Protocol registration and discovery
- ✅ Action/event definitions
- ✅ HID descriptor builders (device, config, HID report)
- ✅ Character-to-HID-usage mapping
- ✅ Server trait implementation (spawn, execute_action)
- ✅ TCP listener for USB/IP connections
- ✅ USB/IP server integration using usbip crate
- ✅ UsbHidKeyboardHandler from usbip crate
- ✅ Device creation with HID keyboard interface
- ✅ LLM action execution (type_text, press_key, release_all_keys)
- ✅ Keyboard event queue (pending_key_events)

### What's Limited (Known Issues)

- ⚠️ **Build Requirement**: Requires libusb-1.0-dev to compile (see Build Requirements above)
- ⚠️ **press_key_combo**: Not yet implemented (requires custom HID report construction)
- ⚠️ **LED status events**: Not yet implemented (requires URB output report parsing)
- ⚠️ **Modifier keys**: Currently limited by UsbHidKeyboardReport::from_ascii()
- ⚠️ **Special keys**: F-keys, arrow keys not yet supported (ASCII only)
- ⚠️ **Testing**: Not yet tested with real usbip client

### Implementation Status

**Phase 1 Complete** (USB/IP Integration):

1. ✅ Integrated usbip crate (v0.3)
2. ✅ Device export using UsbIpServer::new_simulated()
3. ✅ Descriptor handling via usbip::hid::UsbHidKeyboardHandler
4. ✅ Endpoint setup (interrupt IN endpoint 0x81)
5. ✅ URB processing (handled by usbip crate)
6. ✅ LLM action execution (type_text converts to HID reports)
7. ⏳ LED reading (deferred to future enhancement)

## Limitations

### Server Side

- **API Instability**: usbip crate API may change (use specific version)
- **Single Device**: One keyboard device per server instance
- **No Hot-Unplug**: Device remains until client detaches
- **Binary Protocol**: LLM cannot directly construct USB/IP messages

### Client Side

- **Linux Only**: Requires vhci-hcd kernel module (Linux 3.17+)
- **Root Access**: Client must run `sudo usbip attach`
- **Manual Import**: User must run attach command (not automatic)
- **No Windows/macOS Client**: Limited to Linux hosts for importing devices

### Protocol

- **Boot Protocol Only**: No advanced HID features (multimedia keys, N-key rollover)
- **6-Key Limit**: Maximum 6 simultaneous non-modifier keys
- **No Latency Guarantee**: Network delays affect typing responsiveness
- **No Device Discovery**: Client must know server IP:port

## Testing Strategy

See `tests/server/usb_keyboard/CLAUDE.md` for E2E testing approach.

**Key Principles**:

- < 10 LLM calls per test suite
- Use real `usbip` client tools
- Test on Linux VM or container
- Verify keyboard events with evtest or similar

## Future Enhancements

### Phase 2: Additional Device Types

- USB Mouse (usb-mouse protocol)
- USB Serial Port (usb-serial protocol, CDC ACM)

### Phase 3: Low-Level Control

- Custom USB devices (usb protocol)
- Full descriptor customization
- Vendor-specific requests

### Advanced Features

- N-key rollover (non-boot protocol)
- Multimedia keys (consumer control)
- LED indicator control
- Multiple simultaneous devices per server

## References

- **USB/IP Protocol**: https://docs.kernel.org/usb/usbip_protocol.html
- **USB HID Specification**: https://www.usb.org/hid
- **USB HID Usage Tables**: https://usb.org/sites/default/files/hut1_4.pdf
- **jiegec/usbip crate**: https://github.com/jiegec/usbip
- **Linux vhci-hcd**: https://docs.kernel.org/usb/usbip_protocol.html#vhci
