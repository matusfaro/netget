# BLE Beacon Implementation

## Overview

Bluetooth Low Energy (BLE) beacon server that broadcasts proximity/location data. Supports both iBeacon (Apple) and
Eddystone (Google) standards. Beacons are advertisement-only - they do not accept connections, just broadcast data.

## Architecture

### Layered Design

```
bluetooth-ble-beacon (High-level)
    ↓
bluetooth-ble (Low-level GATT)
    ↓
ble-peripheral-rust (Platform backends)
```

### Advertisement-Only Protocol

Unlike keyboard/mouse, beacons:

- **Do not accept connections** - purely broadcast
- **No GATT services** - only advertising packets
- **Low power** - designed for battery-powered devices
- **Proximity-based** - RSSI (signal strength) indicates distance

## Supported Beacon Types

### 1. iBeacon (Apple Standard)

**Format**: Company ID (Apple) + 128-bit UUID + Major + Minor + TX Power

**Use Cases**:

- Indoor positioning
- Proximity marketing
- Asset tracking
- Attendance tracking

**Advertising Data (30 bytes)**:

```
[0-2]:   Flags (0x02, 0x01, 0x06)
[3-4]:   Manufacturer specific data length (0x1A, 0xFF)
[5-6]:   Apple company ID (0x4C, 0x00)
[7]:     iBeacon type (0x02)
[8]:     iBeacon length (0x15 = 21 bytes)
[9-24]:  UUID (16 bytes)
[25-26]: Major (2 bytes, big-endian)
[27-28]: Minor (2 bytes, big-endian)
[29]:    TX Power (1 byte, signed dBm)
```

### 2. Eddystone-UID (Google Standard)

**Format**: Service UUID (Eddystone) + Namespace (10 bytes) + Instance (6 bytes)

**Use Cases**:

- Indoor navigation
- Asset identification
- Location-based services

**Advertising Data (31 bytes)**:

```
[0-1]:   Complete 16-bit UUID list (0x03, 0x03)
[2-3]:   Eddystone UUID (0xAA, 0xFE)
[4-5]:   Service data length (0x17, 0x16)
[6-7]:   Eddystone UUID (0xAA, 0xFE)
[8]:     Frame type UID (0x00)
[9]:     TX Power (signed dBm)
[10-19]: Namespace (10 bytes)
[20-25]: Instance (6 bytes)
[26-27]: RFU reserved (0x00, 0x00)
```

### 3. Eddystone-URL

**Format**: Service UUID + URL scheme code + compressed URL

**Use Cases**:

- Physical web (broadcast URLs)
- Contactless information sharing
- Marketing campaigns

**URL Scheme Codes**:

- `0x00`: http://www.
- `0x01`: https://www.
- `0x02`: http://
- `0x03`: https://

**Limitations**:

- Max ~17 characters after scheme
- No URL encoding for special characters

### 4. Eddystone-TLM (Telemetry)

**Format**: Battery voltage + Temperature + Advertisement count + Uptime

**Use Cases**:

- Beacon health monitoring
- Battery status tracking
- Environmental monitoring

**Advertising Data (25 bytes)**:

```
[0-7]:   Eddystone header
[8]:     Frame type TLM (0x20)
[9]:     TLM version (0x00)
[10-11]: Battery voltage (mV, big-endian)
[12-13]: Temperature (8.8 fixed point, big-endian)
[14-17]: Advertisement count (big-endian)
[18-21]: Uptime (0.1s resolution, big-endian)
```

## LLM Actions

### advertise_ibeacon

Start advertising as an iBeacon.

```json
{
  "type": "advertise_ibeacon",
  "uuid": "12345678-1234-5678-1234-567812345678",
  "major": 1,
  "minor": 100,
  "tx_power": -59
}
```

**Parameters**:

- `uuid`: 128-bit UUID (identifies beacon family)
- `major`: 16-bit identifier (e.g., store ID)
- `minor`: 16-bit identifier (e.g., department ID)
- `tx_power`: Calibrated TX power at 1m (default: -59 dBm)

### advertise_eddystone_uid

Start advertising as Eddystone-UID.

```json
{
  "type": "advertise_eddystone_uid",
  "namespace": "0123456789abcdef0123",
  "instance": "0123456789ab",
  "tx_power": -20
}
```

**Parameters**:

- `namespace`: 10-byte namespace ID (hex string)
- `instance`: 6-byte instance ID (hex string)
- `tx_power`: Calibrated TX power at 0m (default: -20 dBm)

### advertise_eddystone_url

Start advertising as Eddystone-URL.

```json
{
  "type": "advertise_eddystone_url",
  "url": "https://example.com",
  "tx_power": -20
}
```

**Parameters**:

- `url`: URL to broadcast (max ~17 chars after scheme)
- `tx_power`: Calibrated TX power at 0m (default: -20 dBm)

**URL Requirements**:

- Must start with `http://` or `https://`
- Body limited to ~17 characters
- No special character encoding

### advertise_eddystone_tlm

Start advertising as Eddystone-TLM.

```json
{
  "type": "advertise_eddystone_tlm",
  "battery_voltage": 3000,
  "temperature": 22.5,
  "adv_count": 0,
  "uptime": 0
}
```

**Parameters**:

- `battery_voltage`: Voltage in mV (0-65535)
- `temperature`: Temperature in Celsius
- `adv_count`: Advertisement count since boot
- `uptime`: Uptime in seconds

### stop_beacon

Stop beacon advertising.

```json
{
  "type": "stop_beacon"
}
```

## Events

### beacon_started

```json
{
  "event": "beacon_started",
  "beacon_type": "ibeacon"
}
```

### beacon_stopped

```json
{
  "event": "beacon_stopped"
}
```

## Example Usage

### Indoor Positioning System

```
User: "Act as an iBeacon for store ID 5, department 12. Use UUID 12345678-1234-5678-1234-567812345678"

LLM: advertise_ibeacon("12345678-1234-5678-1234-567812345678", 5, 12, -59)
```

### Physical Web URL Broadcast

```
User: "Broadcast the URL https://example.com as a beacon"

LLM: advertise_eddystone_url("https://example.com", -20)
```

### Asset Tracking

```
User: "Act as an Eddystone beacon with namespace 0123456789abcdef0123 and instance 112233445566"

LLM: advertise_eddystone_uid("0123456789abcdef0123", "112233445566", -20)
```

## Implementation Notes

### No Connection Handling

Beacons are advertisement-only, so:

- No connection tracking needed
- No client management
- No bidirectional communication

### TX Power Calibration

TX power is the measured RSSI at a reference distance (1m for iBeacon, 0m for Eddystone). This allows receivers to
estimate distance using the path loss formula:

```
distance ≈ 10 ^ ((TX_Power - RSSI) / (10 * n))
```

Where `n` is the path loss exponent (typically 2-4 depending on environment).

### Advertising Interval

Beacons typically advertise at:

- **100ms**: High update rate, higher power consumption
- **1000ms (1s)**: Standard rate, balanced
- **10000ms (10s)**: Low power, infrequent updates

### Platform Support

Same as `bluetooth-ble`:

- **Linux**: BlueZ daemon
- **macOS**: Bluetooth enabled
- **Windows**: Windows 10+ with Bluetooth

## Limitations

- **BLE only**: No Bluetooth Classic
- **Advertisement data size**: Max 31 bytes
- **No encryption**: Data broadcast in plaintext (except Eddystone-EID)
- **No authentication**: Anyone can scan beacons
- **Range**: Typically 10-100m depending on TX power and environment
- **Eddystone-URL**: Limited to ~17 characters after scheme
- **No bidirectional communication**: Broadcast only

## Security Considerations

### Privacy

Beacons broadcast constantly, which can enable:

- **Tracking**: Devices can be tracked by their beacon signature
- **Fingerprinting**: Unique UUID/namespace combinations identify devices

**Mitigation**: Use rotating IDs (Eddystone-EID) or randomize identifiers periodically

### Spoofing

Anyone can broadcast beacon data with any UUID/namespace. There is no authentication in standard beacon protocols.

**Mitigation**: Use server-side validation and encrypted ephemeral IDs (Eddystone-EID)

## References

- iBeacon Specification: https://developer.apple.com/ibeacon/
- Eddystone Specification: https://github.com/google/eddystone
- BLE Advertising: https://www.bluetooth.com/specifications/specs/core-specification-5-3/
