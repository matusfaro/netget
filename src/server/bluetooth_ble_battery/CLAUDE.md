# BLE Battery Service Implementation

## Overview

Standard Bluetooth Battery Service (0x180F) that reports battery level as a percentage (0-100%). This is one of the simplest BLE GATT services.

## Service Structure

- **Service UUID**: `0x180F` (Battery Service)
- **Characteristic UUID**: `0x2A19` (Battery Level)
- **Properties**: Read, Notify
- **Value Format**: 1 byte (0-100)

## LLM Actions

### set_battery_level
Set battery level percentage.

```json
{
  "type": "set_battery_level",
  "level": 75
}
```

### simulate_drain
Gradually decrease battery level.

```json
{
  "type": "simulate_drain",
  "amount": 10,
  "interval_ms": 5000
}
```

### simulate_charge
Gradually increase battery level.

```json
{
  "type": "simulate_charge",
  "amount": 20,
  "interval_ms": 2000
}
```

## Example Usage

```
User: "Act as a Bluetooth battery. Start at 100%, drain by 1% every 10 seconds until 20%, then alert."

LLM:
set_battery_level(100)
simulate_drain(80, 10000)
```

## References

- Battery Service Spec: https://www.bluetooth.com/specifications/specs/battery-service-1-0/
