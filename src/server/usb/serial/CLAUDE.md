# USB Serial Server Implementation

## Overview

Virtual USB CDC ACM serial port using USB/IP protocol. Appears as `/dev/ttyACM0` on Linux.

## CDC ACM Protocol

**Two Interfaces**:

1. Communication Interface (control): Line coding, DTR/RTS signals
2. Data Interface (bulk): Bidirectional serial data transfer

## LLM Actions

**send_data**: Transmit data to serial port

```json
{"type": "send_data", "data": "Hello\n"}
```

**set_line_coding**: Configure baud rate and parameters

```json
{"type": "set_line_coding", "baud_rate": 115200, "data_bits": 8, "parity": "none", "stop_bits": 1}
```

## LLM Events

- `usb_serial_attached`: Device opened
- `usb_serial_detached`: Device closed
- `usb_serial_data_received`: Data from host

## Line Coding

Default: 115200 baud, 8 data bits, no parity, 1 stop bit (115200 8N1)

## Status

**Experimental**: Framework complete, USB/IP integration needed.

## Build

```bash
./cargo-isolated.sh build --no-default-features --features usb-serial
```
