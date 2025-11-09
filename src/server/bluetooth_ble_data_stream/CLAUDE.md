# BLE Data Stream Service

Custom GATT service for real-time sensor/telemetry data streaming over BLE.

## Actions
- `start_stream` - Begin streaming with sample rate
- `send_stream_data` - Send data packet (JSON payload)
- `stop_stream` - Stop streaming

## Limitations
- Low throughput (~10-20 KB/s max)
- High latency (100+ ms)
- Not suitable for large data or real-time audio/video

## Use Cases
- IMU sensor data (accelerometer, gyroscope)
- GPS coordinates
- Biometric data streaming
- Environmental sensor readings
