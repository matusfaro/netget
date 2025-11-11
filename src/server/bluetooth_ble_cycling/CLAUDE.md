# BLE Cycling Speed and Cadence Service

Standard Cycling Speed and Cadence Service (0x1816) for fitness tracking.

## Actions

- `set_speed` - Set cycling speed (km/h)
- `set_cadence` - Set pedaling cadence (RPM)
- `simulate_ride` - Realistic ride simulation (flat/hill/interval)

## Service UUID: 0x1816

- Characteristic 0x2A5B: CSC Measurement (notify)
- Characteristic 0x2A5C: CSC Feature (read)

## References

- Cycling Speed and Cadence Spec: https://www.bluetooth.com/specifications/specs/cycling-speed-and-cadence-service-1-0/
