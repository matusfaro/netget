# USB Client Implementation

## Overview

The USB client enables LLM-controlled interaction with USB devices through low-level USB transfers. This allows NetGet
to communicate with USB hardware devices like development boards, custom peripherals, sensors, and other USB-connected
equipment.

## Library Choices

**nusb v0.1** - Pure Rust USB library

- **Why chosen**: Modern, pure Rust implementation with no C dependencies
- **Advantages**:
    - Cross-platform (Windows, macOS, Linux)
    - Async-first design (no context object needed)
    - No dependency on libusb (unlike rusb)
    - Clean API for device enumeration and transfers
- **Comparison to alternatives**:
    - `rusb`: Requires libusb C library, more mature but has C dependency
    - `usb-device`: For USB device/gadget mode (peripheral), not host mode

## Architecture

### Device Connection

USB devices are identified using vendor ID and product ID:

```
Format: "VID:PID:INTERFACE"
Examples:
  - "1234:5678" (hex VID/PID, interface 0)
  - "0x1234:0x5678:1" (with 0x prefix, interface 1)
  - "vid:1234,pid:5678,interface:0" (named format)
```

### Connection Flow

1. **Device Enumeration**: List all connected USB devices
2. **Device Opening**: Find device by VID/PID and open it
3. **Interface Claiming**: Claim the specified interface for exclusive access
4. **LLM Integration**: Send connected event with device info
5. **Transfer Execution**: Execute control/bulk/interrupt transfers based on LLM actions

### Transfer Types Supported

1. **Control Transfers** (`control_transfer`):
    - Standard USB control requests (setup packets)
    - Vendor-specific requests
    - Parameters: request_type, request, value, index, data, length
    - Used for device configuration and status queries

2. **Bulk Transfers** (`bulk_transfer_out`, `bulk_transfer_in`):
    - Large data transfers (e.g., file transfers, data dumps)
    - OUT: Send data to device
    - IN: Receive data from device

3. **Interrupt Transfers** (`interrupt_transfer_in`):
    - Periodic data from device (e.g., HID devices like keyboards/mice)
    - Guaranteed latency for time-sensitive data

## LLM Integration

### Events

1. **usb_device_opened**: Triggered when device is successfully opened and interface claimed
    - Provides: vendor_id, product_id, manufacturer, product strings
    - LLM can decide which transfers to perform

2. **usb_control_response**: Response from control transfer
    - Provides: response data (hex), length
    - LLM can parse device responses and decide next actions

3. **usb_bulk_data_received**: Data received from bulk endpoint
    - Provides: data (hex), length, endpoint
    - LLM can process received data and continue communication

4. **usb_interrupt_data_received**: Data received from interrupt endpoint
    - Provides: data (hex), length, endpoint
    - LLM can handle periodic device events

### Actions

**Async Actions** (user-triggered):

- `list_usb_devices`: Enumerate connected USB devices
- `detach_device`: Detach from device and close connection

**Sync Actions** (response to events):

- `control_transfer`: Send USB control request
- `bulk_transfer_out`: Send data via bulk OUT endpoint
- `bulk_transfer_in`: Read data from bulk IN endpoint
- `interrupt_transfer_in`: Read data from interrupt IN endpoint
- `claim_interface`: Claim another USB interface
- `wait_for_more`: Wait before responding

## Data Encoding

All USB data is **hex-encoded** for LLM interaction:

- IN transfers: Device data → hex string → LLM
- OUT transfers: LLM provides hex string → decoded to bytes → device

This avoids binary data issues and makes LLM-generated data human-readable.

## Permissions

**Platform-specific requirements**:

- **Linux**: May require udev rules for non-root access
  ```bash
  # Example udev rule for device VID:1234 PID:5678
  SUBSYSTEM=="usb", ATTR{idVendor}=="1234", ATTR{idProduct}=="5678", MODE="0666"
  ```

- **Windows**: No special permissions typically needed
- **macOS**: May require entitlements for certain device classes

## Example Interactions

### Get Device Descriptor

```json
{
  "type": "control_transfer",
  "request_type": 0x80,
  "request": 0x06,
  "value": 0x0100,
  "index": 0,
  "length": 18
}
```

### Bulk Data Transfer

```json
{
  "type": "bulk_transfer_out",
  "endpoint": 0x02,
  "data_hex": "48656c6c6f20555342"
}
```

### Read Interrupt Data

```json
{
  "type": "interrupt_transfer_in",
  "endpoint": 0x83,
  "length": 8
}
```

## Limitations

1. **No isochronous transfers**: nusb doesn't support isochronous endpoints (audio/video streaming)
2. **Single interface at a time**: LLM controls one claimed interface per connection
3. **Synchronous transfers only**: All transfers block until completion (no async queue)
4. **No hotplug events**: LLM doesn't receive events when devices are plugged/unplugged
5. **Platform differences**: Some devices behave differently across platforms
6. **Requires physical device**: Cannot be fully tested without real USB hardware

## Common Use Cases

- **Custom USB peripherals**: Communicate with development boards (Arduino, STM32, etc.)
- **USB sensors**: Read data from temperature sensors, accelerometers, etc.
- **USB-to-serial adapters**: Low-level serial communication
- **HID devices**: Raw HID communication (keyboards, mice, game controllers)
- **USB storage**: Direct block-level access (be careful!)
- **Firmware updates**: Send firmware to devices via DFU protocol

## Error Handling

nusb errors are converted to anyhow errors and logged. Common errors:

- **Device not found**: VID/PID doesn't match any connected device
- **Permission denied**: Insufficient permissions to access device
- **Interface busy**: Another process has claimed the interface
- **Transfer timeout**: Device didn't respond within timeout (5 seconds)
- **Pipe error**: Invalid endpoint or endpoint stalled

## Future Enhancements

- [ ] Hotplug detection (device plug/unplug events)
- [ ] Async transfer queue (multiple pending transfers)
- [ ] Isochronous transfer support (if nusb adds it)
- [ ] Endpoint discovery (automatically find available endpoints)
- [ ] String descriptor reading (all descriptors, not just manufacturer/product)
- [ ] USB configuration switching (alternate settings)
