# Bluetooth Low Energy (BLE) Protocol Exploration

## What We Can Do Over BLE

Based on research and the capabilities of `ble-peripheral-rust`, here's a comprehensive overview of what we can implement for NetGet.

---

## ✅ Currently Implemented

### 1. **bluetooth-ble** (Low-level GATT Server)
- Generic GATT service/characteristic management
- Full LLM control over BLE stack
- Cross-platform (Win/Mac/Linux)

### 2. **bluetooth-ble-keyboard** (HID Keyboard)
- Type text, key combinations
- Connection tracking, targeted messaging
- Standard HID over GATT profile

### 3. **bluetooth-ble-mouse** (HID Mouse)
- Move cursor, click, scroll, drag
- Connection tracking, targeted messaging
- Standard HID over GATT profile

---

## 🎮 More HID Devices (Feasible - Similar to Keyboard/Mouse)

### bluetooth-ble-gamepad
**What it does**: Act as a Bluetooth game controller (Xbox, PlayStation-style)

**HID Profile**: HID over GATT (0x1812) with gamepad report descriptor

**Actions**:
- `press_button` - A, B, X, Y, L1, R1, L2, R2, Start, Select
- `move_stick` - Left/right analog stick (X, Y values)
- `move_dpad` - D-pad up/down/left/right
- `trigger` - Analog triggers (L2/R2 pressure)

**Report Format**: 8-16 bytes
```
[0-1]: Buttons (bit flags for 16 buttons)
[2-3]: Left stick X/Y
[4-5]: Right stick X/Y
[6-7]: L2/R2 trigger pressure
```

**Use Cases**:
- Remote game control
- Custom controller behaviors
- Input testing/automation
- Accessibility tools

**Complexity**: ⭐⭐ (Similar to mouse, slightly more buttons)

---

### bluetooth-ble-remote
**What it does**: Act as a media remote control (TV, media player)

**HID Profile**: Consumer Control (part of HID over GATT)

**Actions**:
- `play_pause`, `next_track`, `previous_track`
- `volume_up`, `volume_down`, `mute`
- `fast_forward`, `rewind`
- `power`, `menu`, `home`

**Use Cases**:
- Media player control
- Presentation remote
- Smart TV remote

**Complexity**: ⭐ (Very simple, just button presses)

---

### bluetooth-ble-presenter
**What it does**: Presentation clicker for slides

**HID Profile**: Keyboard (simplified) or Consumer Control

**Actions**:
- `next_slide` (Right arrow or Page Down)
- `previous_slide` (Left arrow or Page Up)
- `blank_screen` (B key)
- `laser_pointer` (simulated via mouse)

**Use Cases**:
- PowerPoint/Keynote control
- Webinar presentations
- Teaching

**Complexity**: ⭐ (Just a few key combinations)

---

## 📡 Beacons (Very Feasible - Just Advertising)

### bluetooth-ble-beacon
**What it does**: Broadcast beacon data for proximity/location tracking

Beacons are **advertisement-only** - they don't accept connections, just broadcast data. This makes them very simple to implement.

#### iBeacon (Apple Standard)
**Format**:
```json
{
  "type": "ibeacon",
  "uuid": "12345678-1234-5678-1234-567812345678",  // 128-bit UUID
  "major": 1,      // 16-bit identifier (e.g., store ID)
  "minor": 100,    // 16-bit identifier (e.g., department ID)
  "tx_power": -59  // Calibrated transmission power
}
```

**Actions**:
- `advertise_ibeacon` - Start broadcasting iBeacon
- `update_beacon` - Change UUID/major/minor
- `stop_beacon` - Stop advertising

**Use Cases**:
- Indoor positioning
- Proximity marketing
- Asset tracking
- Attendance tracking

#### Eddystone (Google Standard)
**Multiple Frame Types**:

1. **Eddystone-UID**: Unique beacon ID
   ```json
   {
     "type": "eddystone-uid",
     "namespace": "0123456789abcdef0123",  // 10 bytes
     "instance": "0123456789ab"             // 6 bytes
   }
   ```

2. **Eddystone-URL**: Broadcast a URL
   ```json
   {
     "type": "eddystone-url",
     "url": "https://example.com"  // Compressed format, max ~17 chars
   }
   ```

3. **Eddystone-TLM**: Telemetry data
   ```json
   {
     "type": "eddystone-tlm",
     "battery_voltage": 3000,  // mV
     "temperature": 22.5,      // °C
     "adv_count": 12345,
     "uptime": 3600            // seconds
   }
   ```

4. **Eddystone-EID**: Encrypted ephemeral ID (rotates periodically)
   ```json
   {
     "type": "eddystone-eid",
     "eid": "encrypted_rotating_id"
   }
   ```

**Use Cases**:
- Physical web (URL broadcast)
- Indoor navigation
- Contactless information sharing
- Security/anti-tracking (EID)

**Complexity**: ⭐ (Very simple - just advertising packets, no connections)

---

## 🔋 Standard GATT Services (Moderate Complexity)

These are official Bluetooth SIG GATT services. Very well-documented.

### bluetooth-ble-heart-rate
**Service UUID**: 0x180D (Heart Rate Service)

**Characteristics**:
- 0x2A37 - Heart Rate Measurement (notify)
- 0x2A38 - Body Sensor Location (read)
- 0x2A39 - Heart Rate Control Point (write)

**Actions**:
- `set_bpm` - Set current BPM value
- `set_sensor_location` - Chest, wrist, finger, etc.
- `start_monitoring` - Send periodic notifications

**Use Cases**:
- Fitness app testing
- Health monitoring simulation
- Sports equipment emulation

**Complexity**: ⭐ (Single service, simple data format)

---

### bluetooth-ble-battery
**Service UUID**: 0x180F (Battery Service)

**Characteristics**:
- 0x2A19 - Battery Level (read, notify) - 0-100%

**Actions**:
- `set_battery_level` - Set percentage
- `simulate_drain` - Gradually decrease battery
- `notify_low_battery` - Alert when < 20%

**Use Cases**:
- Battery-powered device simulation
- Testing battery indicators
- Power management testing

**Complexity**: ⭐ (Single characteristic, one byte)

---

### bluetooth-ble-thermometer
**Service UUID**: 0x1809 (Health Thermometer)

**Characteristics**:
- 0x2A1C - Temperature Measurement (notify)
- 0x2A1D - Temperature Type (read)

**Actions**:
- `set_temperature` - Set current temp (Celsius/Fahrenheit)
- `set_type` - Body, room, food, etc.
- `simulate_fever` - Gradual temp increase

**Use Cases**:
- Health app testing
- Smart home simulation
- Food safety monitoring

**Complexity**: ⭐ (Simple service, well-documented)

---

### bluetooth-ble-environmental
**Service UUID**: 0x181A (Environmental Sensing)

**Characteristics**:
- 0x2A6E - Temperature (notify)
- 0x2A6F - Humidity (notify)
- 0x2A76 - UV Index (notify)
- 0x2A6D - Pressure (notify)

**Actions**:
- `set_temperature`, `set_humidity`, `set_pressure`, `set_uv_index`
- `simulate_weather` - Realistic weather patterns
- `climate_control` - HVAC simulation

**Use Cases**:
- Weather station emulation
- Smart home sensors
- Environmental monitoring

**Complexity**: ⭐⭐ (Multiple characteristics, but straightforward)

---

### bluetooth-ble-proximity
**Service UUID**: 0x1802 (Immediate Alert) + 0x1803 (Link Loss) + 0x1804 (Tx Power)

**Find Me Profile (FMP)**: Alert when device moves away

**Actions**:
- `alert_mild`, `alert_high` - Trigger alerts
- `set_link_loss_alert` - Alert on disconnection
- `set_tx_power` - Calibrate range detection

**Use Cases**:
- Anti-loss tags (like Tile, AirTag)
- Pet trackers
- Child safety devices
- Proximity marketing triggers

**Complexity**: ⭐ (Simple alert service)

---

### bluetooth-ble-cycling
**Service UUID**: 0x1816 (Cycling Speed and Cadence)

**Characteristics**:
- 0x2A5B - CSC Measurement (notify) - Speed + cadence
- 0x2A5C - CSC Feature (read)

**Actions**:
- `set_speed` - km/h or mph
- `set_cadence` - RPM
- `simulate_ride` - Realistic speed/cadence patterns

**Use Cases**:
- Fitness app testing
- Indoor trainer simulation
- Cycling computer emulation

**Complexity**: ⭐⭐ (Slightly complex data format)

---

### bluetooth-ble-running
**Service UUID**: 0x1814 (Running Speed and Cadence)

**Characteristics**:
- 0x2A53 - RSC Measurement (notify)
- 0x2A54 - RSC Feature (read)

**Actions**:
- `set_speed` - Pace (min/km)
- `set_cadence` - Steps per minute
- `set_stride_length` - Meters
- `simulate_run` - Realistic running patterns

**Complexity**: ⭐⭐ (Similar to cycling)

---

### bluetooth-ble-weight-scale
**Service UUID**: 0x181D (Weight Scale)

**Characteristics**:
- 0x2A9D - Weight Measurement (notify)

**Actions**:
- `set_weight` - kg or lbs
- `set_bmi` - Calculated BMI
- `multi_user` - Support multiple users

**Use Cases**:
- Health app testing
- Weight tracking simulation
- Fitness tracking

**Complexity**: ⭐ (Simple measurement)

---

## 📁 Custom Data Transfer (Advanced)

### bluetooth-ble-file-transfer
**What it does**: Transfer files over custom GATT service

**Service**: Custom UUID (e.g., 128-bit)

**Characteristics**:
- File metadata (name, size, type)
- Data chunks (20-247 bytes each)
- Transfer control (start, pause, resume, cancel)
- Status/progress

**Actions**:
- `send_file` - Transfer file to connected device
- `receive_file` - Accept file from device
- `list_files` - Share directory listing
- `chunk_data` - Automatic chunking for large files

**Implementation**:
```
File Transfer Protocol:
1. Send metadata: {"name": "photo.jpg", "size": 102400, "chunks": 400}
2. Send chunks: [chunk_0], [chunk_1], ... [chunk_399]
3. Client reassembles and verifies
4. Send completion/error status
```

**Limitations**:
- **Slow**: ~1-2 KB/s (BLE is not designed for bulk data)
- **Small MTU**: 20-247 bytes per packet (depending on negotiation)
- **Connection interval**: Limits throughput
- **Not suitable for**: Large files (>1 MB), video, real-time streaming

**Use Cases**:
- Config file transfer
- Small image/document sharing
- Sensor data logs
- Firmware updates (small binaries)

**Complexity**: ⭐⭐⭐ (Requires chunking, reassembly, error handling)

---

### bluetooth-ble-data-stream
**What it does**: Real-time data streaming over BLE

**Service**: Custom streaming service

**Characteristics**:
- Stream control (start/stop)
- Data channel (notifications)
- Metadata (sample rate, format)

**Actions**:
- `start_stream` - Begin data transmission
- `set_sample_rate` - Control data rate
- `send_sensor_data` - Continuous sensor readings

**Use Cases**:
- IMU data (accelerometer, gyroscope)
- Audio samples (very low quality)
- GPS coordinates
- Biometric data

**Limitations**:
- Low throughput (~10-20 KB/s max)
- High latency (100+ ms)
- Packet loss possible

**Complexity**: ⭐⭐⭐ (Real-time constraints, buffering)

---

## ❌ What We CANNOT Do (Bluetooth Classic Required)

These require **Bluetooth Classic (BR/EDR)**, not BLE. No cross-platform Rust support exists for peripheral mode.

### ❌ High-Quality Audio Streaming
- **A2DP** (Advanced Audio Distribution Profile)
- **Headphones/speakers**
- Requires Bluetooth Classic, not BLE
- LE Audio exists but extremely complex

### ❌ Serial Port Communication
- **SPP** (Serial Port Profile)
- **RFCOMM** protocol
- Requires Bluetooth Classic

### ❌ Traditional File Transfer
- **OBEX** (Object Exchange)
- **FTP** (File Transfer Profile)
- **OPP** (Object Push Profile)
- Requires Bluetooth Classic
- BLE alternative: Custom GATT service (slow)

### ❌ Phone Calls
- **HFP** (Hands-Free Profile)
- **HSP** (Headset Profile)
- Requires Bluetooth Classic + audio stack

### ❌ Network Access
- **PAN** (Personal Area Network)
- **DUN** (Dial-up Networking)
- Requires Bluetooth Classic

---

## 🎯 Recommended Implementation Priority

Based on feasibility, usefulness, and complexity:

### **Tier 1: Easy Wins** ⭐
1. **bluetooth-ble-beacon** - Very simple, high utility
2. **bluetooth-ble-remote** - Media control, useful for demos
3. **bluetooth-ble-battery** - Single characteristic, educational

### **Tier 2: Standard Services** ⭐⭐
4. **bluetooth-ble-heart-rate** - Popular, well-documented
5. **bluetooth-ble-thermometer** - Health monitoring
6. **bluetooth-ble-environmental** - Multi-sensor simulation
7. **bluetooth-ble-proximity** - Find Me functionality

### **Tier 3: HID Devices** ⭐⭐
8. **bluetooth-ble-gamepad** - Gaming use case
9. **bluetooth-ble-presenter** - Presentation tool

### **Tier 4: Advanced** ⭐⭐⭐
10. **bluetooth-ble-file-transfer** - Custom protocol, complex
11. **bluetooth-ble-data-stream** - Real-time streaming

### **Tier 5: Specialized** ⭐⭐
12. **bluetooth-ble-cycling** - Fitness tracking
13. **bluetooth-ble-running** - Fitness tracking
14. **bluetooth-ble-weight-scale** - Health monitoring

---

## 📊 Summary Table

| Protocol | Complexity | Usefulness | Implementation Time | Status |
|----------|-----------|------------|---------------------|--------|
| bluetooth-ble | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | - | ✅ Done |
| bluetooth-ble-keyboard | ⭐⭐ | ⭐⭐⭐⭐⭐ | - | ✅ Done |
| bluetooth-ble-mouse | ⭐⭐ | ⭐⭐⭐⭐⭐ | - | ✅ Done |
| **bluetooth-ble-beacon** | ⭐ | ⭐⭐⭐⭐⭐ | 1 day | 🎯 **Recommended** |
| bluetooth-ble-remote | ⭐ | ⭐⭐⭐⭐ | 1 day | 🎯 **Recommended** |
| bluetooth-ble-gamepad | ⭐⭐ | ⭐⭐⭐⭐ | 2 days | 🎯 **Recommended** |
| bluetooth-ble-heart-rate | ⭐ | ⭐⭐⭐ | 1 day | Useful |
| bluetooth-ble-battery | ⭐ | ⭐⭐⭐ | 0.5 day | Educational |
| bluetooth-ble-thermometer | ⭐ | ⭐⭐⭐ | 1 day | Useful |
| bluetooth-ble-environmental | ⭐⭐ | ⭐⭐⭐ | 1-2 days | Useful |
| bluetooth-ble-proximity | ⭐ | ⭐⭐⭐⭐ | 1 day | Useful |
| bluetooth-ble-file-transfer | ⭐⭐⭐ | ⭐⭐ | 3-4 days | Advanced |
| bluetooth-ble-cycling | ⭐⭐ | ⭐⭐ | 1-2 days | Specialized |
| bluetooth-ble-running | ⭐⭐ | ⭐⭐ | 1-2 days | Specialized |

---

## 💡 Most Interesting Use Cases

### 1. **Smart Home Hub Simulation**
Combine multiple services:
- Environmental sensors
- Proximity detection
- Battery monitoring
- Remote control

### 2. **Fitness Tracker Emulation**
- Heart rate
- Running/cycling speed
- Location beacons
- Data streaming

### 3. **IoT Device Testing**
- Battery-powered sensors
- Environmental monitoring
- Data collection
- Firmware updates (file transfer)

### 4. **Indoor Positioning System**
- Multiple beacons (iBeacon/Eddystone)
- RSSI-based triangulation
- Proximity alerts
- Way-finding applications

### 5. **Accessibility Tools**
- Custom HID devices
- Adaptive controllers
- Alternative input methods
- Assistive technology

---

## 🚀 Next Steps

Would you like me to implement any of these? Top recommendations:

1. **bluetooth-ble-beacon** - Extremely useful, very simple
2. **bluetooth-ble-gamepad** - Gaming use case, similar to keyboard/mouse
3. **bluetooth-ble-remote** - Media control, universal appeal
4. **bluetooth-ble-heart-rate** - Standard service, good example
5. **bluetooth-ble-file-transfer** - Advanced, shows custom GATT capabilities

Let me know which direction interests you most!
