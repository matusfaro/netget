# BLE Running Speed and Cadence Service

Standard Running Speed and Cadence Service (0x1814) for fitness tracking.

## Actions
- `set_pace` - Set running pace (min/km)
- `set_cadence` - Set running cadence (steps per minute)
- `simulate_run` - Realistic run simulation (easy/tempo/interval/sprint)

## Service UUID: 0x1814
- Characteristic 0x2A53: RSC Measurement (notify)
- Characteristic 0x2A54: RSC Feature (read)

## References
- Running Speed and Cadence Spec: https://www.bluetooth.com/specifications/specs/running-speed-and-cadence-service-1-0/
