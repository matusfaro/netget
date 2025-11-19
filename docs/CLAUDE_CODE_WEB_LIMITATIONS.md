# Claude Code for Web - System Dependency Limitations

**Last Updated:** 2025-11-19
**Environment:** Claude Code for Web (cloud sandbox environment)
**Status:** Comprehensive analysis complete

## Overview

NetGet has **~100 protocol features**, of which **~75 are available** in Claude Code for Web. The remaining **~32 features** require system libraries that are not available in the web-based sandbox environment.

This document provides a comprehensive guide to understanding these limitations, working around them, and knowing which features are available for development in Claude Code for Web.

---

## Quick Reference

### ✅ Available Features (~75)

You can build and test these features in Claude Code for Web:

```bash
tcp, socket_file, http, http2, http3, pypi, maven, udp, datalink, arp, dc, dns, dot, doh, dhcp, bootp, ntp, whois, snmp, igmp, syslog, ssh, ssh-agent, svn, irc, xmpp, telnet, smtp, mdns, mysql, ipp, postgresql, redis, rss, proxy, webdav, nfs, cassandra, smb, stun, turn, webrtc, sip, ldap, imap, pop3, nntp, mqtt, amqp, socks5, elasticsearch, dynamo, s3, sqs, npm, openai, ollama, oauth2, jsonrpc, wireguard, openvpn, ipsec, bgp, ospf, isis, rip, bitcoin, mcp, xmlrpc, tor, vnc, openapi, openid, git, mercurial, torrent-tracker, torrent-dht, torrent-peer, tls, saml-idp, saml-sp, embedded-llm
```

### ❌ Unavailable Features (~32)

These features require system libraries NOT available in Claude Code for Web:

**Bluetooth (18):** `bluetooth-ble`, `bluetooth-ble-client`, `bluetooth-ble-keyboard`, `bluetooth-ble-mouse`, `bluetooth-ble-beacon`, `bluetooth-ble-remote`, `bluetooth-ble-battery`, `bluetooth-ble-heart-rate`, `bluetooth-ble-thermometer`, `bluetooth-ble-environmental`, `bluetooth-ble-proximity`, `bluetooth-ble-gamepad`, `bluetooth-ble-presenter`, `bluetooth-ble-file-transfer`, `bluetooth-ble-data-stream`, `bluetooth-ble-cycling`, `bluetooth-ble-running`, `bluetooth-ble-weight-scale`

**USB (7):** `usb`, `usb-keyboard`, `usb-mouse`, `usb-serial`, `usb-msc`, `usb-fido2`, `usb-smartcard`

**NFC (2):** `nfc`, `nfc-client`

**Protobuf-based (4):** `etcd`, `grpc`, `kubernetes`, `zookeeper`

**Other (1):** `kafka` (may require system dependencies)

---

## Detailed Breakdown by Category

### 1. Bluetooth Features (18 features)

**System Dependency:** `libdbus-1-dev` (D-Bus system library)
**Error:** `The system library 'dbus-1' required by crate 'libdbus-sys' was not found`

#### Why Required
Bluetooth Low Energy (BLE) on Linux uses BlueZ, which communicates via D-Bus. All Rust BLE libraries (`btleplug`, `ble-peripheral-rust`) depend on D-Bus system bindings.

#### Unavailable Features
- `bluetooth-ble` - BLE GATT server (peripheral mode)
- `bluetooth-ble-client` - BLE client for connecting to BLE devices
- `bluetooth-ble-keyboard` - BLE HID keyboard server
- `bluetooth-ble-mouse` - BLE HID mouse server
- `bluetooth-ble-beacon` - BLE beacon (iBeacon, Eddystone)
- `bluetooth-ble-remote` - BLE media remote control
- `bluetooth-ble-battery` - BLE Battery Service
- `bluetooth-ble-heart-rate` - BLE Heart Rate Service
- `bluetooth-ble-thermometer` - BLE Health Thermometer
- `bluetooth-ble-environmental` - BLE Environmental Sensing
- `bluetooth-ble-proximity` - BLE Proximity/Find Me
- `bluetooth-ble-gamepad` - BLE HID gamepad
- `bluetooth-ble-presenter` - BLE presentation clicker
- `bluetooth-ble-file-transfer` - BLE file transfer
- `bluetooth-ble-data-stream` - BLE real-time data streaming
- `bluetooth-ble-cycling` - BLE Cycling Speed and Cadence
- `bluetooth-ble-running` - BLE Running Speed and Cadence
- `bluetooth-ble-weight-scale` - BLE Weight Scale

#### Dependency Chain
```
bluetooth-ble-client
└── btleplug (Rust BLE library)
    └── bluez-generated (BlueZ bindings for Linux)
        └── dbus (D-Bus Rust library)
            └── libdbus-sys (D-Bus system bindings)
                └── ❌ libdbus-1-dev (system library NOT in web environment)
```

#### Workarounds
- **Local Development:** Install `libdbus-1-dev` (`sudo apt-get install libdbus-1-dev`)
- **Emulation:** Not feasible - requires actual Bluetooth hardware and kernel support
- **Alternative:** Use network-based protocols (TCP, HTTP) to simulate device communication in tests

---

### 2. USB Features (7 features)

**System Dependency:** `libusb-1.0-dev` (USB access library)
**Error:** `failed to run custom build command for libusb1-sys v0.4.4`

#### Why Required
USB device access requires kernel-level permissions and the libusb system library. No pure-Rust alternative exists for USB hardware access.

#### Unavailable Features
- `usb` - USB client for connecting to USB devices
- `usb-keyboard` - USB HID Keyboard virtual device
- `usb-mouse` - USB HID Mouse virtual device
- `usb-serial` - USB CDC ACM Serial virtual device
- `usb-msc` - USB Mass Storage Class virtual device (flash drive)
- `usb-fido2` - USB FIDO2/U2F Security Key virtual device
- `usb-smartcard` - USB Smart Card (CCID) virtual device

#### Dependency Chain
```
usb
└── nusb (Rust USB library)
    └── libusb1-sys (libusb Rust bindings)
        └── ❌ libusb-1.0-dev (system library NOT in web environment)
```

#### Workarounds
- **Local Development:** Install `libusb-1.0-0-dev` (`sudo apt-get install libusb-1.0-0-dev`)
- **Emulation:** Not feasible - requires USB subsystem and hardware
- **Alternative:** Use network-based HID protocols or USB/IP over network

---

### 3. NFC Features (2 features)

**System Dependency:** `pcsclite` (PC/SC Smart Card library)
**Error:** `pcsc` crate requires PC/SC system library

#### Why Required
NFC card readers use the PC/SC (Personal Computer/Smart Card) standard, which requires system-level drivers and the pcsclite daemon.

#### Unavailable Features
- `nfc` - NFC card emulation server via PC/SC (ISO14443, MIFARE, NFC tags)
- `nfc-client` - NFC reader/writer client via PC/SC (ISO14443, APDU, NDEF)

#### Dependency Chain
```
nfc, nfc-client
└── pcsc (PC/SC Rust library)
    └── ❌ pcsclite (system library NOT in web environment)
```

#### Workarounds
- **Local Development:** Install `pcsclite-dev` (`sudo apt-get install pcsclite-dev`)
- **Emulation:** Not feasible - requires NFC reader hardware
- **Alternative:** Use network-based APDU tunneling or virtual smart card protocols

---

### 4. Protocol Buffer Features (4 features)

**System Dependency:** `protoc` (Protocol Buffer compiler)
**Error:** `Could not find 'protoc'. If 'protoc' is installed, try setting the 'PROTOC' environment variable`

#### Why Required
These crates use `prost-build` or `tonic-build` which require the Protocol Buffer compiler (`protoc`) at build time to generate Rust code from `.proto` files.

#### Unavailable Features
- `etcd` - etcd distributed key-value store client
- `grpc` - gRPC framework for RPC communication
- `kubernetes` - Kubernetes API client
- `zookeeper` - Apache ZooKeeper client

#### Dependency Chain
```
etcd
└── etcd-client (etcd Rust client)
    └── prost-build (build dependency for protobuf)
        └── ❌ protoc (Protocol Buffer compiler NOT in web environment)
```

#### Workarounds
- **Potential Fix:** Research if vendored protoc solutions exist (e.g., `protobuf-src` crate)
- **Alternative:** Use REST APIs instead of gRPC where available (e.g., etcd HTTP API)
- **Local Development:** Install `protobuf-compiler` (`sudo apt-get install protobuf-compiler`)

#### Future Investigation
The `protobuf-src` crate can vendor protoc, but requires build-time integration. This is a potential area for improvement.

---

### 5. Kafka Feature (1 feature)

**System Dependency:** Unknown (untested)
**Status:** Excluded preemptively, needs testing

#### Unavailable Features
- `kafka` - Apache Kafka message broker client

#### Investigation Needed
The Kafka feature was excluded during testing but not confirmed to fail. It may:
- Require system libraries (e.g., librdkafka for native client)
- Compile successfully with pure-Rust client
- Require protoc for schema registry features

**Action Required:** Test in isolation to determine exact limitations.

---

## Detection and Build Commands

### Detecting Claude Code for Web

Use the provided detection script:

```bash
./am_i_claude_code_for_web.sh
```

This checks multiple environment variables:
1. `CLAUDE_CODE_REMOTE=true` (primary)
2. `CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE=cloud_default` (secondary)
3. `CLAUDE_CODE_ENTRYPOINT=remote` or `IS_SANDBOX=yes` (tertiary)

### Recommended Build Commands

#### Maximum Features for Web Environment
```bash
./cargo-isolated.sh build --no-default-features --features \
tcp,socket_file,http,http2,http3,pypi,maven,udp,datalink,arp,dc,dns,dot,doh,dhcp,bootp,ntp,whois,snmp,igmp,syslog,ssh,ssh-agent,svn,irc,xmpp,telnet,smtp,mdns,mysql,ipp,postgresql,redis,rss,proxy,webdav,nfs,cassandra,smb,stun,turn,webrtc,sip,ldap,imap,pop3,nntp,mqtt,amqp,socks5,elasticsearch,dynamo,s3,sqs,npm,openai,ollama,oauth2,jsonrpc,wireguard,openvpn,ipsec,bgp,ospf,isis,rip,bitcoin,mcp,xmlrpc,tor,vnc,openapi,openid,git,mercurial,torrent-tracker,torrent-dht,torrent-peer,tls,saml-idp,saml-sp,embedded-llm
```

#### Single Protocol Testing
```bash
./cargo-isolated.sh build --no-default-features --features tcp
```

#### Multiple Protocols
```bash
./cargo-isolated.sh build --no-default-features --features tcp,http,dns,ssh
```

#### ❌ What NOT to Do
```bash
# This WILL FAIL in Claude Code for Web
./cargo-isolated.sh build --all-features
```

---

## Local Development Setup

To build ALL features including system-dependent ones, you need a local Linux environment with system packages installed.

### Ubuntu/Debian
```bash
sudo apt-get update
sudo apt-get install \
    libdbus-1-dev \
    libusb-1.0-0-dev \
    pcsclite-dev \
    protobuf-compiler \
    pkg-config

./cargo-isolated.sh build --all-features
```

### Fedora/RHEL
```bash
sudo dnf install \
    dbus-devel \
    libusb-devel \
    pcsc-lite-devel \
    protobuf-compiler \
    pkgconf-pkg-config

./cargo-isolated.sh build --all-features
```

### macOS
```bash
brew install \
    dbus \
    libusb \
    pcsc-lite \
    protobuf

./cargo-isolated.sh build --all-features
```

---

## Testing Strategy

### E2E Tests in Claude Code for Web

When running E2E tests in web environment, use feature flags to exclude unavailable protocols:

```bash
# Test specific available protocol
./test-e2e.sh tcp

# Test multiple available protocols
./cargo-isolated.sh test --no-default-features --features tcp,http,dns

# DO NOT test unavailable protocols
./test-e2e.sh bluetooth-ble  # Will fail to compile
```

### Mock Testing

All protocols should support mock testing (no Ollama required). Mocks work regardless of system dependencies because they don't require actual protocol implementation to compile:

```bash
# Test with mocks (works for ALL protocols including unavailable ones)
./test-e2e.sh bluetooth-ble  # Would work if we had mock-only testing

# But compilation still requires system dependencies
# So mocks don't help for unavailable features
```

---

## Future Improvements

### Potential Solutions

1. **Vendored protoc**
   - Use `protobuf-src` crate to vendor Protocol Buffer compiler
   - Would enable etcd, grpc, kubernetes, zookeeper in web environment
   - Requires build script modifications in dependent crates

2. **Pure-Rust Alternatives**
   - Research pure-Rust D-Bus implementations (e.g., `zbus` instead of `dbus`)
   - May enable bluetooth features without system dependencies
   - Performance and compatibility trade-offs

3. **Feature-Gated Tests**
   - Implement mock-only test mode that doesn't compile protocol implementations
   - Would allow testing LLM integration logic without system dependencies
   - Requires refactoring to separate LLM logic from protocol I/O

4. **Conditional Compilation**
   - Add platform-specific feature gates (e.g., `bluetooth-ble-linux`, `bluetooth-ble-mock`)
   - Would make limitations explicit in Cargo.toml
   - Better error messages for unsupported platforms

---

## FAQ

### Q: Why can't I use `--all-features` in Claude Code for Web?

**A:** `--all-features` includes all features in `Cargo.toml`, including bluetooth, USB, NFC, and protobuf-based features. These require system libraries that are not installed in the web sandbox environment. The build will fail with "library not found" errors.

### Q: Can I test bluetooth features in Claude Code for Web?

**A:** No, bluetooth features require D-Bus system libraries. You must test them in a local Linux environment with `libdbus-1-dev` installed.

### Q: Will these limitations be fixed?

**A:** Some can be improved (e.g., vendored protoc for etcd/grpc), but hardware-dependent features (bluetooth, USB, NFC) will always require local development with actual hardware.

### Q: How do I know which features I can use?

**A:** Run `./am_i_claude_code_for_web.sh` to get a list of available and unavailable features. Copy the recommended build command for maximum available features.

### Q: What if I need to modify a bluetooth/USB/NFC protocol?

**A:** Development must be done in a local environment with system dependencies installed. See "Local Development Setup" section above.

### Q: Are there workarounds for testing unavailable features?

**A:** Not really. You can mock the LLM interactions, but the protocol implementation itself won't compile without system libraries. For protocol development, use local environment.

---

## Related Documentation

- **COMPILATION_ERROR_REPORT.md** - Detailed error analysis from build attempts
- **CLAUDE.md** - Main project documentation (see "Claude Code for Web Environment" section)
- **am_i_claude_code_for_web.sh** - Environment detection script
- **Cargo.toml** - Feature definitions and dependencies

---

## Summary

**Claude Code for Web is suitable for:**
- ✅ Developing and testing ~75 network protocols (TCP, HTTP, DNS, SSH, etc.)
- ✅ LLM integration development and testing
- ✅ Protocol action system development
- ✅ TUI and CLI development
- ✅ State management and event system

**Claude Code for Web is NOT suitable for:**
- ❌ Developing bluetooth, USB, NFC, or hardware protocols
- ❌ Building with `--all-features`
- ❌ Testing protocols that require system dependencies
- ❌ Protobuf-based protocols (etcd, grpc, kubernetes, zookeeper)

**Bottom Line:** Use Claude Code for Web for most NetGet development, but switch to a local environment with system packages installed when working on hardware-dependent protocols.

---

**Document Status:** Complete and comprehensive
**Maintenance:** Update when new features are added or dependencies change
**Last Verified:** 2025-11-19
