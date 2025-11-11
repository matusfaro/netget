# BLE Remote Control Implementation

## Overview

Bluetooth Low Energy (BLE) remote control server that allows devices to pair with NetGet and receive media control
commands. Built on top of the `bluetooth-ble` protocol, providing a high-level remote control interface.

## Architecture

### Layered Design

```
bluetooth-ble-remote (High-level)
    ↓
bluetooth-ble (Low-level GATT)
    ↓
ble-peripheral-rust (Platform backends)
```

### Consumer Control Profile

Uses HID Consumer Control (part of HID over GATT) for media and system control buttons.

## HID over GATT Profile

### Service Structure

- **Service UUID**: `0x1812` (HID Service)
- **Characteristics**: Same as keyboard/mouse but with Consumer Control report descriptor

### HID Report Descriptor

Standard HID Consumer Control with 16 button bits:

- Play/Pause
- Next/Previous Track
- Stop, Fast Forward, Rewind
- Volume Up/Down, Mute
- Power, Menu, Home
- 4 reserved bits for future expansion

### Report Format (2 bytes)

```
[0-1]: Button bits (16 buttons, bit flags)
```

**Example**: Play/Pause pressed

```
01 00
││
└─ Bit 0 set (Play/Pause)
```

**Example**: Volume Up pressed

```
40 00
││
└─ Bit 6 set (Volume Up)
```

## LLM Actions

### play_pause

Toggle play/pause.

```json
{
  "type": "play_pause"
}
```

### next_track

Skip to next track.

```json
{
  "type": "next_track"
}
```

### previous_track

Go to previous track.

```json
{
  "type": "previous_track"
}
```

### volume_up

Increase volume.

```json
{
  "type": "volume_up"
}
```

### volume_down

Decrease volume.

```json
{
  "type": "volume_down"
}
```

### mute

Toggle mute.

```json
{
  "type": "mute"
}
```

### fast_forward

Fast forward.

```json
{
  "type": "fast_forward"
}
```

### rewind

Rewind.

```json
{
  "type": "rewind"
}
```

### stop

Stop playback.

```json
{
  "type": "stop"
}
```

## Events

### remote_button_pressed

```json
{
  "event": "remote_button_pressed",
  "button": "play_pause"
}
```

## Example Usage

### Media Control

```
User: "Act as a Bluetooth remote. When connected, press play/pause, wait 5 seconds, then volume up twice."

LLM:
play_pause()
wait(5000)
volume_up()
volume_up()
```

### Presentation Control

```
User: "Act as a remote control. Press next track 5 times to advance slides."

LLM:
next_track()
next_track()
next_track()
next_track()
next_track()
```

## Implementation Notes

### No Connection Tracking

Unlike keyboard/mouse, this implementation doesn't track individual clients. All button presses are broadcast to all
connected devices.

### Button Release

Buttons are momentary - each action sends a button press followed immediately by release (all bits clear). This matches
standard remote control behavior.

### Platform Compatibility

**Works with**:

- Media players (VLC, Windows Media Player, iTunes, Spotify)
- Smart TVs
- Streaming devices (Roku, Fire TV, Apple TV)
- Presentation software (PowerPoint, Keynote)

**Platform support**:

- **Windows**: Native Consumer Control support
- **macOS**: Full support via CoreBluetooth
- **Linux**: BlueZ with Consumer Control mapping
- **Android/iOS**: Full support as BLE central

## Limitations

- **BLE only**: No Bluetooth Classic
- **16 buttons maximum**: Limited by report descriptor
- **No haptic feedback**: Cannot vibrate or provide tactile response
- **No display**: Cannot show information back to remote
- **No custom buttons**: Fixed button set per HID spec

## Platform Requirements

Same as `bluetooth-ble`:

- **Linux**: BlueZ daemon
- **macOS**: Bluetooth enabled
- **Windows**: Windows 10+ with Bluetooth

## References

- HID over GATT Profile: https://www.bluetooth.com/specifications/specs/hid-over-gatt-profile-1-0/
- HID Usage Tables (Consumer Page): https://www.usb.org/sites/default/files/documents/hut1_12v2.pdf
