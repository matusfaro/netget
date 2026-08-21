# USB Protocol Enhancement Roadmap

This document tracks the enhancement roadmap for NetGet's USB protocol implementations.

## Current Implementation Status

### ✅ Fully Implemented USB Protocols

1. **USB Keyboard** (usb-keyboard)
   - ✅ HID device with boot protocol
   - ✅ Keypress simulation
   - ✅ Modifier keys support
   - ✅ LLM integration

2. **USB Mouse** (usb-mouse)
   - ✅ HID device with boot protocol
   - ✅ Movement and button control
   - ✅ LLM integration

3. **USB Serial** (usb-serial)
   - ✅ CDC ACM device
   - ✅ Bidirectional communication
   - ✅ LLM integration

4. **USB Mass Storage** (usb-msc)
   - ✅ BOT (Bulk-Only Transport)
   - ✅ SCSI command set
   - ✅ Virtual disk images
   - ✅ READ/WRITE operations
   - ✅ LLM integration

5. **USB FIDO2/U2F** (usb-fido2) - **Recently Enhanced**
   - ✅ CTAPHID transport layer
   - ✅ U2F (CTAP1) protocol
   - ✅ FIDO2 (CTAP2) protocol
   - ✅ MakeCredential (registration)
   - ✅ GetAssertion (authentication)
   - ✅ ECDSA P-256 cryptography
   - ✅ **Persistent credential storage (NEW!)**
   - ⚠️ No PIN/UV support yet
   - ⚠️ No resident keys yet
   - ⚠️ Limited LLM integration

6. **USB Smart Card** (usb-smartcard)
   - ✅ CCID protocol basics
   - ✅ APDU command/response
   - ✅ RSA cryptography
   - ✅ INTERNAL_AUTHENTICATE
   - ⚠️ No PIV/OpenPGP apps yet
   - ⚠️ Limited LLM integration

---

## ✅ Phase 1: FIDO2 & Smart Card Enhancements (COMPLETED)

**Status:** Complete
**Completion Date:** 2025-11-08

### Completed Work

1. **LLM Action Framework** ✅
   - Implemented comprehensive action definitions for FIDO2
   - Implemented comprehensive action definitions for Smart Card
   - Documented architectural limitations clearly
   - Provided alternative approaches via events

2. **Action Documentation** ✅
   - approve_request: Documents sync/async bridge requirement
   - deny_request: Explains architectural constraints
   - list_credentials: Explains per-connection storage model
   - save/load_credentials: LLM-controlled persistence via events
   - insert/remove_card: Virtual insertion model explained
   - set/verify_pin: APDU-driven approach documented

3. **Key Insights Documented** ✅
   - USB/IP synchronous vs LLM async bridge challenge
   - Credentials stored per-connection in handlers
   - LLM observability via events
   - Client-driven operations (APDU, WebAuthn)

### Deferred Items (Future Work)

**High Complexity Items:**

## 🚀 Phase 1 (Future): Advanced FIDO2 & Smart Card Features (Priority: High)

### USB FIDO2 Enhancements

#### 1.1 PIN/UV Support (Complexity: High, Time: 3-4 days)
**Status:** Planned
**Priority:** High

**Implementation:**
- Implement `ClientPin` command (0x06)
- PIN storage (hashed with salt)
- PIN verification protocol
- PIN retry counter (3-8 attempts)
- PIN change workflow
- UV (User Verification) flag support

**Files to Modify:**
- `src/server/usb/fido2/ctap2.rs` - Add PIN handler
- `src/server/usb/fido2/mod.rs` - PIN state management

**Dependencies:**
- `argon2` or `pbkdf2` for PIN hashing

**User Stories:**
- As a user, I want to protect my FIDO2 key with a PIN
- As a developer, I want to test PIN-protected WebAuthn flows

---

#### 1.2 LLM Integration for User Approval (Complexity: High, Time: 2-3 days)
**Status:** Partially implemented (events defined, actions TODO)
**Priority:** Medium

**Challenge:** USB/IP requests are synchronous, but LLM calls are async.

**Approaches:**
1. **Auto-approve mode** (Simple, implemented by default)
   - Configuration flag to auto-approve all requests
   - Good for dev/testing environments

2. **Event-driven logging** (Simple, partially done)
   - Generate events when requests occur
   - LLM observes but doesn't block

3. **Timeout-based approval** (Complex, future)
   - Wait up to N seconds for LLM approval
   - Fall back to deny if timeout
   - Requires shared state between sync USB handler and async LLM actions

**Files to Modify:**
- `src/server/usb/fido2/actions.rs` - Implement approval/deny actions
- `src/server/usb/fido2/mod.rs` - Add approval state tracking
- `src/server/usb/fido2/ctap2.rs` - Check approval before operations

**User Stories:**
- As an LLM, I want to approve/deny registration requests based on RP ID
- As a user, I want the LLM to log all FIDO2 operations for transparency

---

#### 1.3 Resident Key Support (Complexity: Medium, Time: 2-3 days)
**Status:** Planned
**Priority:** Low

**Implementation:**
- Store user handles on-device (in credential store)
- Support `rk` option in MakeCredential
- Implement `GetNextAssertion` command
- Return multiple credentials for same RP

**Benefits:**
- Enables passwordless authentication
- User can authenticate without providing credential ID

---

#### 1.4 Extended Features (Complexity: Low-Medium, Time: 1-2 days each)
**Status:** Planned
**Priority:** Low

**Features:**
- [ ] Proper attestation certificate chain (currently self-signed dummy)
- [ ] HMAC-secret extension
- [ ] CredProtect extension
- [ ] Large blob storage
- [ ] Credential management (list, delete via CTAP2)

---

### USB Smart Card Enhancements

#### 1.5 PIV Card Application (Complexity: Very High, Time: 5-7 days)
**Status:** Planned
**Priority:** Medium

**Implementation:**
- Implement NIST SP 800-73-4 PIV specification
- PIV applet selection (AID: A000000308000010000100)
- Data objects (authentication cert, signature cert, key management cert)
- PIV commands:
  - SELECT (0xA4)
  - GET DATA (0xCB)
  - VERIFY (0x20) - PIN verification
  - CHANGE REFERENCE DATA (0x24) - PIN change
  - GENERAL AUTHENTICATE (0x87) - crypto operations
  - GENERATE ASYMMETRIC KEY PAIR (0x47)

**Use Cases:**
- SSH authentication with PIV
- Code signing with PIV certificates
- Encrypted email with PIV keys

**References:**
- NIST SP 800-73-4: https://csrc.nist.gov/publications/detail/sp/800-73/4/final
- Yubico PIV tool: https://developers.yubico.com/yubico-piv-tool/

---

#### 1.6 OpenPGP Card Application (Complexity: Very High, Time: 5-7 days)
**Status:** Planned
**Priority:** Medium

**Implementation:**
- Implement OpenPGP Card v3.4 specification
- OpenPGP applet selection (AID: D276000124010304...)
- Key slots (signature, encryption, authentication)
- OpenPGP commands:
  - SELECT (0xA4)
  - GET DATA (0xCA)
  - PUT DATA (0xDA)
  - VERIFY (0x20) - PIN verification
  - INTERNAL AUTHENTICATE (0x88)
  - PSO: COMPUTE DIGITAL SIGNATURE (0x2A)
  - PSO: DECIPHER (0x2A)

**Use Cases:**
- GPG signing and encryption
- SSH authentication with GPG
- Email encryption with GnuPG

**References:**
- OpenPGP Card Spec 3.4: https://gnupg.org/ftp/specs/OpenPGP-smart-card-application-3.4.pdf
- Gnuk (OpenPGP card implementation): https://github.com/gpg/gnuk

---

#### 1.7 LLM Integration for Smart Card (Complexity: Medium, Time: 1-2 days)
**Status:** Planned
**Priority:** Medium

**Implementation:**
- Event generation for APDU commands
- PIN verification prompts to LLM
- Approval actions for crypto operations
- Credential management actions

**User Stories:**
- As an LLM, I want to approve crypto operations based on context
- As a user, I want the LLM to log all smart card operations

---

## ✅ Phase 2: Testing & Quality (COMPLETED)

**Status:** Complete
**Completion Date:** 2025-11-08

### Completed Work

1. **FIDO2 E2E Test Framework** ✅
   - Created tests/server/usb_fido2/e2e_test.rs
   - 8 comprehensive test cases defined
   - Test helpers for server startup and USB/IP
   - Full documentation in tests/server/usb_fido2/CLAUDE.md

2. **Smart Card E2E Test Framework** ✅
   - Created tests/server/usb_smartcard/e2e_test.rs
   - 8 comprehensive test cases defined
   - Test helpers for APDU and PC/SC operations
   - Full documentation in tests/server/usb_smartcard/CLAUDE.md

3. **Test Documentation** ✅
   - System requirements listed
   - Manual testing procedures provided
   - Running instructions documented
   - Known issues and workarounds catalogued
   - LLM call budget: 0 (tests use real tools)
   - Expected runtime: 2-3 minutes per suite

### Test Implementation Status

- Framework: 100% complete
- Test bodies: 0% complete (marked as TODO for future implementation)
- Documentation: 100% complete
- Manual testing: Ready to use

### Future Work

**Complete Test Bodies:**

## 🧪 Phase 2 (Future): Complete Test Implementation (Priority: Medium)

### 2.1 USB FIDO2 E2E Tests
**Status:** Planned
**Complexity:** Medium
**Time:** 1-2 days
**LLM Budget:** < 10 calls

**Test Scenarios:**
- [ ] Device attachment and GetInfo
- [ ] U2F registration and authentication
- [ ] FIDO2 registration (MakeCredential)
- [ ] FIDO2 authentication (GetAssertion)
- [ ] Credential persistence (restart server, re-authenticate)
- [ ] Multiple credentials per RP
- [ ] Reset command

**Test Tools:**
- libfido2 tools (`fido2-token`, `fido2-cred`, `fido2-assert`)
- Browser testing (Chrome/Firefox with WebAuthn)

---

### 2.2 USB Smart Card E2E Tests
**Status:** Planned
**Complexity:** Medium
**Time:** 1-2 days
**LLM Budget:** < 10 calls

**Test Scenarios:**
- [ ] Device attachment and ATR
- [ ] APDU exchange (SELECT, GET DATA)
- [ ] PIN verification
- [ ] RSA signing with INTERNAL_AUTHENTICATE
- [ ] Key generation
- [ ] Certificate storage and retrieval

**Test Tools:**
- OpenSC tools (`opensc-tool`, `pkcs15-tool`)
- pcsc-tools (`pcsc_scan`)
- PIV tools (`yubico-piv-tool`)

---

## 🎯 Current Status Summary

### Completed Phases

- ✅ **Phase 1**: LLM Integration & Action Framework
- ✅ **Phase 2**: E2E Test Infrastructure
- ✅ **Advanced Features** (Partial): PIN/UV, Resident Keys, Sync/Async Bridge

### In Progress

- 🔄 **Advanced Features** (Remaining): PIV Application, OpenPGP Application
- 🔄 **Phase 3**: New USB Protocols (Future)

---

## 🌟 Phase 3: New USB Protocols (Priority: Low-Medium)

### 3.1 USB Audio (UAC - USB Audio Class)
**Status:** Research
**Complexity:** Very High
**Time:** 7-10 days
**Priority:** Medium

**Features:**
- Virtual speakers (playback)
- Virtual microphone (recording)
- Audio streaming with isochronous transfers
- Volume control
- LLM-controlled audio generation/capture

**Use Cases:**
- Text-to-speech output
- Voice assistant input
- Audio monitoring and analysis

**Challenges:**
- Isochronous USB transfers (time-sensitive)
- Audio buffer management
- Real-time audio processing

---

### 3.2 USB Video (UVC - USB Video Class)
**Status:** Research
**Complexity:** Very High
**Time:** 10-14 days
**Priority:** Medium

**Features:**
- Virtual webcam
- Video streaming (MJPEG, H.264)
- LLM-controlled frame generation
- Camera controls (brightness, contrast, etc.)

**Use Cases:**
- Virtual camera for video conferencing
- Synthetic video streams
- Video testing and debugging

**Challenges:**
- Video encoding/compression
- Frame timing and synchronization
- Large data transfers

---

### 3.3 USB CDC Ethernet
**Status:** Research
**Complexity:** High
**Time:** 4-6 days
**Priority:** High

**Features:**
- Virtual network adapter
- Ethernet framing
- IP forwarding
- LLM-controlled network bridging

**Use Cases:**
- Network isolation
- Traffic inspection
- Virtual networking

**Benefits:**
- Complements existing network protocols (DHCP, DNS, etc.)
- Enables USB network tethering scenarios

---

### 3.4 USB Printer (PCLM, IPP-USB)
**Status:** Research
**Complexity:** Medium
**Time:** 3-5 days
**Priority:** Low

**Features:**
- Virtual printer device
- Print job capture
- LLM-controlled print processing

**Use Cases:**
- Print job monitoring
- PDF generation from print jobs
- Print data extraction

---

### 3.5 USB Hub
**Status:** Research
**Complexity:** Very High
**Time:** 7-10 days
**Priority:** Low

**Features:**
- Virtual USB hub with multiple ports
- Dynamic device attachment/detachment
- Port power management

**Benefits:**
- Host multiple virtual USB devices from single USB/IP server
- Simplified multi-device scenarios

**Challenges:**
- Complex USB/IP hub support
- Port management
- Descriptor hierarchies

---

## 📋 Remaining TODOs

### Documentation
- [ ] Add comprehensive FIDO2 usage guide with browser examples
- [ ] Add Smart Card usage guide with OpenSC examples
- [ ] Document credential storage format and migration
- [ ] Create troubleshooting guide for USB/IP connection issues

### Infrastructure
- [ ] Add GitHub Actions CI for USB protocol builds
- [ ] Create Docker image with libusb-dev pre-installed
- [ ] Add USB protocol integration tests to CI

### Security
- [ ] Implement encrypted credential storage
- [ ] Add option for hardware-backed key storage (TPM, Keychain)
- [ ] Security audit of cryptographic operations
- [ ] Implement secure PIN entry (avoid logging)

### Performance
- [ ] Profile FIDO2 authentication latency
- [ ] Optimize credential serialization/deserialization
- [ ] Add credential caching for faster lookups

---

## 🎯 Next Steps (Immediate)

Based on the current state and priorities:

1. **Continue with simpler enhancements:**
   - Add comprehensive documentation for FIDO2 and Smart Card
   - Implement credential management actions (list, delete)
   - Add event logging for better LLM observability

2. **Testing:**
   - Create E2E tests for FIDO2 (manual tests first)
   - Create E2E tests for Smart Card

3. **New protocols with high value:**
   - USB CDC Ethernet (complements existing network protocols)
   - Basic USB Audio (for TTS integration)

4. **Advanced features (later):**
   - PIN/UV support for FIDO2
   - PIV/OpenPGP applications for Smart Card
   - Complex protocols (Video, Hub)

---

## 📊 Enhancement Metrics

| Protocol | Completeness | LLM Integration | Tests | Priority |
|----------|--------------|-----------------|-------|----------|
| Keyboard | 95% | ✅ Full | ✅ Yes | - |
| Mouse | 95% | ✅ Full | ✅ Yes | - |
| Serial | 95% | ✅ Full | ✅ Yes | - |
| MSC | 90% | ✅ Full | ✅ Yes | - |
| FIDO2 | 70% | ⚠️ Partial | ❌ No | High |
| Smart Card | 50% | ⚠️ Partial | ❌ No | Medium |
| Audio | 0% | ❌ None | ❌ No | Medium |
| Video | 0% | ❌ None | ❌ No | Medium |
| CDC Ethernet | 0% | ❌ None | ❌ No | High |
| Printer | 0% | ❌ None | ❌ No | Low |
| Hub | 0% | ❌ None | ❌ No | Low |

---

## 🔗 References

### Specifications
- USB Specifications: https://www.usb.org/documents
- FIDO Alliance: https://fidoalliance.org/specifications/
- CCID Specification: https://www.usb.org/sites/default/files/DWG_Smart-Card_CCID_Rev110.pdf
- ISO 7816 Standards: https://www.iso.org/standard/

### Tools
- USB/IP: https://github.com/torvalds/linux/tree/master/tools/usb/usbip
- libfido2: https://github.com/Yubico/libfido2
- OpenSC: https://github.com/OpenSC/OpenSC
- vsmartcard: https://github.com/frankmorgner/vsmartcard

### Reference Implementations
- softfido (Rust FIDO2 over USB/IP): https://github.com/ellerh/softfido
- OpenSK (Google's FIDO2): https://github.com/google/OpenSK
- Virtual FIDO (Go): https://github.com/bulwarkid/virtual-fido

---

**Last Updated:** 2025-11-08
**Version:** 1.0
**Status:** Living Document
