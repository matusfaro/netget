# NetGet Native Android App - Implementation Plan

## Overview
Build a full-featured Android app with Kotlin/Jetpack Compose UI, JNI bridge to Rust core, and support for all 50+ protocols including hardware-dependent ones (BLE, NFC, USB).

**Timeline**: 6-12 months | **Effort**: Very High | **Outcome**: Play Store-ready app

---

## Phase 1: Android App Foundation (4-6 weeks)

### 1.1 Project Structure
- Create Android project (Kotlin + Gradle)
- Set up JNI bridge architecture
- Configure Rust cross-compilation for Android targets (arm64-v8a, armeabi-v7a, x86_64)
- Integrate cargo with Gradle build system
- Create build variants (debug/release, feature-gated protocol builds)

### 1.2 Minimal JNI Bridge
- Define JNI interface for core functions (start/stop server, execute action, get status)
- Implement Rust FFI exports (`#[no_mangle] pub extern "C"`)
- Create Kotlin wrapper classes
- Test basic Rust ↔ Kotlin communication

### 1.3 Permissions & Runtime Setup
- Declare Android permissions (INTERNET, ACCESS_NETWORK_STATE, BLUETOOTH, NFC, USB_HOST, etc.)
- Implement runtime permission requests
- Create permission explanation UI
- Handle permission denial gracefully

**Deliverable**: Empty Android app that can call into Rust core via JNI

---

## Phase 2: Pure-Rust Protocols (6-8 weeks)

### 2.1 Headless Service Architecture
- Remove Crossterm TUI dependencies (feature-gate for CLI builds)
- Create headless mode (no terminal output)
- Implement Android logging bridge (Rust logs → Android logcat)
- Replace `status_tx` channel with JNI callbacks
- Add lifecycle management (start/pause/stop/resume)

### 2.2 Port Core Protocols (40+ protocols)
**Target protocols**: TCP, UDP, HTTP, DNS, SSH, SMTP, IMAP, MySQL, PostgreSQL, Redis, MongoDB, Elasticsearch, MQTT, AMQP, gRPC, WebSocket, and 25+ others

- Build with `--target aarch64-linux-android`
- Feature-gate problematic protocols (BLE, NFC, USB, packet capture)
- Test networking with Android's network stack
- Handle high-port requirements (>1024 for unprivileged operation)
- Create protocol enable/disable UI

### 2.3 LLM Integration
- Add configurable Ollama endpoint (remote URL or localhost:11434)
- Implement endpoint validation and connectivity testing
- Support both HTTP and HTTPS endpoints
- Add retry logic and timeout handling
- Create settings UI for LLM configuration

**Deliverable**: Android app running 40+ pure-Rust protocols with configurable LLM endpoint

---

## Phase 3: Hardware Protocol Rewrites (8-12 weeks)

### 3.1 Bluetooth BLE Protocols (15+ protocols)
**Current**: `ble-peripheral-rust` → BlueZ (Linux)
**Android**: Android Bluetooth API via JNI

- Create Rust trait for BLE operations (advertise, accept_connection, send_notification)
- Implement JNI bridge for Android BluetoothGattServer API
- Rewrite each BLE protocol to use new trait:
  - BLE Keyboard, Mouse, Joystick, Touchscreen (HID profiles)
  - Heart Rate Monitor, Blood Pressure, Glucose (Health profiles)
  - Battery Service, Device Info, Generic Access
  - Custom GATT services
- Test with real BLE clients (iOS/Android apps)

### 3.2 NFC Protocols
**Current**: `pcsc` → PC/SC library (smart cards)
**Android**: Android NFC API via JNI

- Create Rust trait for NFC operations (enable_reader, send_apdu, read_ndef)
- Implement JNI bridge for Android NfcAdapter API
- Rewrite NFC server/client protocols
- Support both card emulation and reader modes
- Test with physical NFC tags and devices

### 3.3 USB Protocols
**Current**: `usbip` → USB/IP kernel module
**Android**: Android USB Host API via JNI

- Create Rust trait for USB operations (claim_interface, bulk_transfer, control_transfer)
- Implement JNI bridge for Android UsbManager API
- Rewrite USB HID protocols (keyboard, mouse, FIDO2)
- Rewrite USB serial and smart card readers
- Test with USB OTG devices

**Deliverable**: Full protocol support including BLE, NFC, and USB via Android APIs

---

## Phase 4: Android UI (6-8 weeks)

### 4.1 Jetpack Compose UI Architecture
- Design 3-column layout (Servers | Clients | Events)
- Implement protocol selector (40+ protocols with search/filter)
- Create server configuration screens (port, protocol-specific params)
- Build real-time event log viewer
- Add LLM conversation history display

### 4.2 Key Screens
1. **Home**: Active servers/clients dashboard
2. **Protocols**: Browse and launch protocols with templates
3. **Server Detail**: Real-time logs, connections, actions
4. **Client Detail**: Connection status, send data, responses
5. **Settings**: LLM endpoint, logging level, permissions
6. **Help**: Protocol documentation, example prompts

### 4.3 UX Features
- Material 3 design system
- Dark/light theme support
- Swipe gestures for quick actions
- Notifications for important events
- Copy/share logs functionality
- Quick protocol templates (common configurations)

**Deliverable**: Full-featured Android UI replacing terminal TUI

---

## Phase 5: Android-Specific Features (4-6 weeks)

### 5.1 Background Service
- Implement foreground service for long-running servers
- Add persistent notification with quick controls
- Handle app lifecycle (background/foreground transitions)
- Support server persistence across app restarts
- Implement graceful shutdown on low memory

### 5.2 Network & Connectivity
- Detect network changes (WiFi/cellular/offline)
- Pause/resume servers on network changes
- Support IPv6 and dual-stack (IPv4+IPv6)
- Handle VPN and proxy scenarios
- Add network usage monitoring

### 5.3 Storage & Configuration
- Use Android app-private storage for logs/configs
- Implement configuration export/import (JSON/YAML)
- Add protocol preset library (save/load common configs)
- Support external storage for large files (with permissions)
- Implement automatic log rotation

### 5.4 Security & Privacy
- Implement certificate pinning for LLM connections
- Add option to disable LLM logging
- Support encrypted configuration storage
- Add network traffic inspection UI (for debugging)
- Implement rate limiting and abuse prevention

**Deliverable**: Production-ready Android app with platform-native features

---

## Phase 6: Testing & Release (4-6 weeks)

### 6.1 Testing Strategy
- Unit tests for JNI bridge
- Integration tests for protocol lifecycle
- UI tests (Espresso/Compose Testing)
- Manual testing on multiple devices (different Android versions, form factors)
- Performance testing (memory usage, battery impact)
- Network testing (WiFi, cellular, airplane mode)
- Hardware testing (BLE, NFC, USB on real devices)

### 6.2 Optimization
- Profile and optimize memory usage
- Reduce APK size (ProGuard, native library stripping)
- Optimize battery consumption
- Implement connection pooling and resource reuse
- Add crash reporting (Firebase Crashlytics or similar)

### 6.3 Documentation
- User guide (in-app and web)
- Protocol-specific tutorials
- Example prompts library
- Troubleshooting guide
- Privacy policy and terms of service

### 6.4 Release Preparation
- Create Play Store listing (screenshots, description, video)
- Set up alpha/beta testing tracks
- Implement update mechanism
- Add analytics (opt-in, privacy-respecting)
- Prepare open-source release (if applicable)

**Deliverable**: NetGet Android app on Google Play Store

---

## Technical Architecture Summary

```
Android App (Kotlin)
├── UI Layer (Jetpack Compose)
│   ├── Protocol Selector
│   ├── Server/Client Management
│   ├── Event Viewer
│   └── Settings
├── Service Layer
│   ├── Foreground Service (background servers)
│   ├── Notification Manager
│   └── Lifecycle Handler
├── JNI Bridge (C FFI)
│   ├── Protocol Management (start/stop/configure)
│   ├── Event Callbacks (Rust → Kotlin)
│   ├── Hardware APIs (BLE/NFC/USB)
│   └── Logging Bridge
└── Rust Core (NetGet)
    ├── Headless Mode (no TUI)
    ├── 40+ Pure Rust Protocols
    ├── Hardware Protocol Traits
    │   ├── BLE Trait → Android Bluetooth via JNI
    │   ├── NFC Trait → Android NFC via JNI
    │   └── USB Trait → Android USB via JNI
    ├── LLM Client (configurable endpoint)
    └── Event System
```

---

## Key Challenges & Solutions

| Challenge | Solution |
|-----------|----------|
| **TUI removal** | Replace with Jetpack Compose UI, headless Rust core |
| **BLE/NFC/USB** | Rewrite using Android APIs via JNI traits |
| **Permissions** | Request at runtime, explain each permission, graceful degradation |
| **LLM flexibility** | Configurable endpoint (remote or local Termux) |
| **Build complexity** | Gradle + cargo integration, automated cross-compilation |
| **Testing** | Feature-gated builds for subset testing, mock LLM for unit tests |

---

## Estimated Timeline

- **Month 1-2**: Phase 1 (Foundation) + Phase 2 start (core protocols)
- **Month 3-4**: Phase 2 completion + Phase 3 start (hardware rewrites)
- **Month 5-7**: Phase 3 completion (BLE/NFC/USB) + Phase 4 (UI)
- **Month 8-9**: Phase 5 (Android features)
- **Month 10-12**: Phase 6 (testing, optimization, release)

**Total**: 10-12 months for full implementation

---

## Recommended First Steps

1. **Create Android project** with Gradle + cargo integration
2. **Set up JNI bridge** with minimal Rust FFI
3. **Build headless NetGet** for Android target (disable TUI)
4. **Test single protocol** (TCP) end-to-end on Android device
5. **Iterate** and expand protocol support

---

## Alternative Approaches Considered

### Approach 1: Termux Binary (2-4 weeks)
- **Pros**: Minimal changes, keeps TUI, fast implementation
- **Cons**: Limited to Termux users, no native UI, limited hardware access
- **Verdict**: Good for quick proof-of-concept, not end-user friendly

### Approach 2: Headless + Web UI (2-3 months)
- **Pros**: Platform-independent UI, moderate effort, works in Termux or as app
- **Cons**: Not native Android experience, still limited hardware access
- **Verdict**: Good middle-ground, but doesn't leverage Android platform

### Approach 3: Native Android App (THIS PLAN)
- **Pros**: Full platform integration, all protocols supported, Play Store ready
- **Cons**: High effort, long timeline, requires Android expertise
- **Verdict**: **SELECTED** - Best long-term solution for all protocols

---

## Dependencies & Prerequisites

### Development Environment
- Android Studio (latest stable)
- Android NDK (r25c or later)
- Rust toolchain with Android targets:
  ```bash
  rustup target add aarch64-linux-android
  rustup target add armv7-linux-androideabi
  rustup target add x86_64-linux-android
  ```
- Java 17+ (for Gradle)

### Android Requirements
- Minimum SDK: 26 (Android 8.0) - for modern API support
- Target SDK: 34 (Android 14) - latest Play Store requirement
- Required permissions: INTERNET, BLUETOOTH, BLUETOOTH_ADMIN, NFC, USB_HOST

### Testing Devices
- Physical Android device with Android 8.0+ (for BLE/NFC/USB testing)
- Various Android emulators (different API levels, screen sizes)
- Devices with BLE, NFC, and USB OTG support

---

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| **JNI complexity** | High | Start with simple bridge, expand incrementally |
| **Hardware API rewrites** | Very High | Prototype each (BLE/NFC/USB) before full implementation |
| **Build system integration** | Medium | Use cargo-ndk plugin, automate with Gradle |
| **Performance on mobile** | Medium | Profile early, optimize incrementally, feature-gate heavy protocols |
| **Play Store approval** | Low | Follow guidelines, explain use cases clearly in listing |
| **Ollama on Android** | Medium | Support remote endpoint as primary, local as optional |
| **Testing complexity** | High | Mock LLM for unit tests, limit E2E tests, manual testing protocol |

---

## Success Metrics

### Technical Metrics
- [ ] All 50+ protocols functional on Android
- [ ] APK size < 50MB (stripped)
- [ ] App startup time < 2s
- [ ] Memory usage < 200MB for 5 concurrent servers
- [ ] Battery drain < 5% per hour (background service)
- [ ] No crashes with >99.9% stability rate

### User Metrics
- [ ] 4.0+ star rating on Play Store
- [ ] <5% uninstall rate within 7 days
- [ ] >50% DAU/MAU ratio (daily/monthly active users)
- [ ] <10s time to first server launch (onboarding)

### Development Metrics
- [ ] CI/CD pipeline with automated builds
- [ ] <5% test flakiness rate
- [ ] <24h bug fix turnaround for critical issues
- [ ] Full documentation coverage for all protocols

---

## Open Questions

1. **App branding**: Keep "NetGet" name or use Android-specific branding?
2. **Monetization**: Free and open-source, or freemium model with premium features?
3. **Model size**: Bundle small models in APK or require download on first launch?
4. **Cloud features**: Support cloud sync for configurations across devices?
5. **Community**: Open-source from day 1 or closed beta first?

---

## Next Action Items

- [ ] Set up Android Studio project with Kotlin + Gradle
- [ ] Configure Android NDK and Rust cross-compilation
- [ ] Create minimal JNI "hello world" bridge
- [ ] Build NetGet for `aarch64-linux-android` target
- [ ] Test basic FFI call from Kotlin to Rust
- [ ] Implement headless mode (disable TUI)
- [ ] Document JNI architecture and conventions

---

**Last Updated**: 2025-11-16
**Status**: Planning Phase
**Next Milestone**: Phase 1.1 - Project Structure Setup
