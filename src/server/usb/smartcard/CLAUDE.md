# USB Smart Card Reader (CCID) Server Implementation

## Overview

The USB Smart Card Reader (CCID) server creates a virtual smart card reader using the USB/IP protocol. This allows an
LLM to control a virtual smart card that can be accessed by applications through PC/SC (Personal Computer/Smart Card)
middleware, enabling smart card authentication, digital signatures, and secure storage operations.

## Architecture

### USB/IP + CCID Protocol Stack

```
┌─────────────────┐                    ┌──────────────────┐
│  NetGet Server  │                    │  Linux Client    │
│  (USB/IP CCID)  │ ◄────── TCP ─────► │  (vhci-hcd)      │
│  Port: 3240     │                    │  usbip attach    │
└─────────────────┘                    └──────────────────┘
         │                                      │
         │ Creates virtual                     │ Sees as
         │ smart card reader                   │ PC/SC reader
         ▼                                     ▼
    [CCID Descriptors]                    [pcscd daemon]
    [CCID Protocol]                       [Smart Card Apps]
    [ISO 7816-4 APDUs]
    [Virtual Smart Card]
```

### Protocol Layers

1. **USB Layer**: USB/IP protocol for device virtualization
2. **CCID Layer**: Chip Card Interface Device class (0x0B)
3. **T=0/T=1 Layer**: ISO 7816-3 transmission protocols
4. **APDU Layer**: ISO 7816-4 command/response format
5. **Card Layer**: Virtual smart card chip (JavaCard, PIV, etc.)
6. **App Layer**: Card applications (authentication, signatures, encryption)

## Protocol Specifications

### USB CCID Device

**Device Class**: Smart Card (0x0B)
**Subclass**: 0x00
**Protocol**: 0x00 (CCID)
**bcdCCID**: 0x0110 (CCID version 1.10)

**Supported Features:**

- Automatic parameter configuration
- Automatic activation on insert
- Automatic ICC voltage selection
- Automatic ICC clock frequency change
- Automatic baud rate change
- Short APDU level exchange
- Extended APDU level exchange (optional)

### CCID Commands (Host → Reader)

- **PC_to_RDR_IccPowerOn** (0x62): Power on the ICC (smart card)
- **PC_to_RDR_IccPowerOff** (0x63): Power off the ICC
- **PC_to_RDR_GetSlotStatus** (0x65): Get slot status
- **PC_to_RDR_XfrBlock** (0x6F): Transfer APDU command block
- **PC_to_RDR_GetParameters** (0x6C): Get T=0 or T=1 parameters
- **PC_to_RDR_ResetParameters** (0x6D): Reset parameters to defaults
- **PC_to_RDR_SetParameters** (0x61): Set T=0 or T=1 parameters
- **PC_to_RDR_Escape** (0x6B): Vendor-specific command
- **PC_to_RDR_IccClock** (0x6E): Stop/restart ICC clock
- **PC_to_RDR_Abort** (0x72): Abort current operation
- **PC_to_RDR_SetDataRateAndClockFrequency** (0x73): Adjust speed

### CCID Responses (Reader → Host)

- **RDR_to_PC_DataBlock** (0x80): Response data block
- **RDR_to_PC_SlotStatus** (0x81): Slot status information
- **RDR_to_PC_Parameters** (0x82): T=0 or T=1 parameters
- **RDR_to_PC_Escape** (0x83): Vendor-specific response
- **RDR_to_PC_DataRateAndClockFrequency** (0x84): Current speed

### Notifications (Reader → Host, Interrupt Endpoint)

- **RDR_to_PC_NotifySlotChange** (0x50): Card insertion/removal
- **RDR_to_PC_HardwareError** (0x51): Hardware error occurred

### ISO 7816-4 APDU Format

**Command APDU** (host → card):

```
CLA  INS  P1   P2   [Lc] [Data] [Le]
1B   1B   1B   1B   [1B] [Lc]   [1B]

CLA: Class byte (instruction category)
INS: Instruction byte (specific command)
P1, P2: Parameters
Lc: Length of command data (optional)
Data: Command data (optional)
Le: Maximum length of response data (optional)
```

**Response APDU** (card → host):

```
[Data] SW1  SW2
[...]  1B   1B

Data: Response data (optional)
SW1, SW2: Status words (e.g., 0x90 0x00 = success)
```

### ISO 7816-3 ATR (Answer To Reset)

When a card is powered on, it responds with an ATR (Answer To Reset) that describes:

- Supported transmission protocols (T=0, T=1)
- Supported voltage levels
- Clock frequency
- Baud rate
- Historical bytes (card type, manufacturer)

Example ATR (minimal):

```
3B 00  (T=0 protocol, no historical bytes)
```

Example ATR (typical):

```
3B 9F 95 80 1F C7 80 31 E0 73 FE 21 1B 66 D0 01 83 07 90 00 80
(T=1 protocol, historical bytes indicate card type)
```

## Current Status: **Not Yet Implemented**

### What's Required

#### Phase 1: USB CCID Device Handler (High Priority)

- ❌ Custom `UsbInterfaceHandler` for CCID
- ❌ CCID class-specific descriptors
- ❌ Bulk IN endpoint (0x81) for responses
- ❌ Bulk OUT endpoint (0x01) for commands
- ❌ Interrupt IN endpoint (0x83) for notifications

#### Phase 2: CCID Protocol (High Priority)

- ❌ PC_to_RDR_IccPowerOn (send ATR)
- ❌ PC_to_RDR_IccPowerOff
- ❌ PC_to_RDR_GetSlotStatus
- ❌ PC_to_RDR_XfrBlock (APDU exchange)
- ❌ PC_to_RDR_GetParameters
- ❌ PC_to_RDR_SetParameters
- ❌ RDR_to_PC_DataBlock (APDU response)
- ❌ RDR_to_PC_SlotStatus
- ❌ RDR_to_PC_NotifySlotChange (card insertion)

#### Phase 3: ISO 7816-3 Transmission Protocol (Medium Priority)

- ❌ ATR (Answer To Reset) generation
- ❌ T=0 protocol (byte-oriented)
- ❌ T=1 protocol (block-oriented, optional)
- ❌ PPS (Protocol and Parameters Selection)

#### Phase 4: ISO 7816-4 APDU Handler (High Priority)

- ❌ APDU parsing (CLA, INS, P1, P2, Lc, Data, Le)
- ❌ APDU response formatting (Data, SW1, SW2)
- ❌ Status word generation
- ❌ Command chaining support (optional)
- ❌ Extended APDU support (optional)

#### Phase 5: Virtual Smart Card (High Priority)

- ❌ File system (MF, DF, EF per ISO 7816-4)
- ❌ SELECT command (0xA4)
- ❌ READ BINARY command (0xB0)
- ❌ UPDATE BINARY command (0xD6)
- ❌ VERIFY command (0x20, PIN verification)
- ❌ GET RESPONSE command (0xC0)
- ❌ Security state management

#### Phase 6: Card Applications (Medium Priority)

- ❌ PIV (Personal Identity Verification) card
- ❌ OpenPGP card
- ❌ Generic PKI card
- ❌ Custom card applications

#### Phase 7: Cryptography (Optional)

- ❌ RSA key generation and operations
- ❌ ECC key generation and operations
- ❌ INTERNAL AUTHENTICATE command (0x88)
- ❌ EXTERNAL AUTHENTICATE command (0x82)
- ❌ GENERATE ASYMMETRIC KEY PAIR command (0x46)

#### Phase 8: LLM Integration (Low Priority)

- ❌ Card insertion/removal events
- ❌ PIN verification prompts
- ❌ APDU logging and interpretation
- ❌ Actions (insert_card, remove_card, set_pin, load_applet)

#### Phase 9: Testing (Deferred)

- ❌ E2E tests with real PC/SC applications
- ❌ pcscd daemon integration
- ❌ OpenSC tools testing
- ❌ PIV authentication testing

## Implementation Complexity

### Estimated Effort

- **Phase 1** (USB CCID handler): 2-3 days
- **Phase 2** (CCID protocol): 3-4 days
- **Phase 3** (ISO 7816-3): 2-3 days
- **Phase 4** (ISO 7816-4 APDU): 2-3 days
- **Phase 5** (Virtual card): 3-4 days
- **Phase 6** (Card applications): 4-6 days (per app)
- **Phase 7** (Cryptography): 3-4 days
- **Phase 8** (LLM integration): 1-2 days
- **Phase 9** (Testing): 2-3 days

**Total**: 18-28 days for basic implementation, +4-6 days per card application

### Complexity Rating: **VERY HIGH**

**Reasons:**

1. **Dual Protocol**: CCID (USB) + ISO 7816-4 (card)
2. **File System**: Must implement ISO 7816-4 file hierarchy
3. **Security**: PIN verification, access controls, crypto operations
4. **Card Apps**: Each card type (PIV, OpenPGP) has detailed specs
5. **Standards**: Complex ISO standards (7816-1 through 7816-15)

## Library Choices

### Available Rust Implementations

#### 1. **usbd-ccid** (Rust CCID Device Implementation)

- **Crate**: https://crates.io/crates/usbd-ccid
- **Docs**: https://docs.rs/usbd-ccid/
- **Purpose**: CCID communication to USB host

**Features:**

- Implements CCID device-side protocol
- Sends APDUs to an Interchange
- USB device framework integration

**Pros:**

- ✅ Rust implementation of CCID
- ✅ Device-side (what we need!)
- ✅ Available on crates.io

**Cons:**

- ⚠️ Designed for embedded USB device framework
- ⚠️ May need adaptation for USB/IP
- ⚠️ Documentation sparse

**Verdict:** ⭐⭐⭐⭐ **BEST OPTION for CCID layer** - needs evaluation for USB/IP compatibility

#### 2. **vpicc** (Rust Virtual Smart Card)

- **Crate**: https://crates.io/crates/vpicc
- **Docs**: https://docs.rs/vpicc/
- **Purpose**: Connect to vpcd daemon and implement virtual smart card

**Features:**

- Implements `VSmartCard` trait
- Connects to vsmartcard's vpcd daemon
- Handles APDU exchange

**Pros:**

- ✅ Pure Rust smart card implementation
- ✅ Works with existing vsmartcard infrastructure
- ✅ Mature protocol (vpcd)

**Cons:**

- ⚠️ Requires vpcd daemon (separate process)
- ⚠️ Not USB/IP based (uses TCP to vpcd)
- ⚠️ Additional dependency (vsmartcard)

**Verdict:** ⭐⭐⭐⭐ **BEST OPTION for card layer** - different architecture than USB/IP

#### 3. **pcsc** + **pcsc-sys** (PC/SC Bindings)

- **Crate**: https://crates.io/crates/pcsc (high-level)
- **Crate**: https://crates.io/crates/pcsc-sys (FFI)
- **GitHub**: https://github.com/bluetech/pcsc-rust
- **Purpose**: Client-side PC/SC communication

**Features:**

- Bindings to PC/SC lite (Linux), WinSCard (Windows), PCSC framework (macOS)
- APDU transmission to cards
- Reader enumeration

**Pros:**

- ✅ Cross-platform
- ✅ Well-maintained
- ✅ Safe Rust API

**Cons:**

- ❌ **CLIENT-SIDE** (for using cards, not emulating readers)
- ❌ Not for device implementation
- ❌ Wrong direction (we're the device, not the client)

**Verdict:** ⭐ Not suitable - wrong side of protocol

#### 4. **iso7816_tx** (ISO 7816 T=1 Protocol)

- **Crate**: https://crates.io/crates/iso7816_tx
- **Purpose**: ISO 7816 T=1 transmission protocol
- **Features**: `TransmissionBuilder` for no_std environments

**Pros:**

- ✅ Handles T=1 protocol (block transmission)
- ✅ Embedded-friendly (no_std)

**Cons:**

- ⚠️ Only T=1, not T=0
- ⚠️ Low-level, needs integration

**Verdict:** ⭐⭐⭐ Useful for T=1 support

#### 5. **apdu** crate (APDU Handling)

- **Crate**: https://crates.io/crates/apdu
- **Docs**: https://docs.rs/apdu/
- **Purpose**: APDU command/response types

**Features:**

- High-level APDU API
- Helper functions for command composition
- Cross-platform

**Pros:**

- ✅ Simplifies APDU handling
- ✅ Type-safe APDU construction

**Cons:**

- ⚠️ Just types, not full protocol

**Verdict:** ⭐⭐⭐ Useful utility crate

#### 6. **openpgp-card** (OpenPGP Card Client)

- **Crate**: https://crates.io/crates/openpgp-card
- **Purpose**: Client library for OpenPGP cards

**Features:**

- OpenPGP card 3.4 specification
- Works with Gnuk, Nitrokey, YubiKey

**Pros:**

- ✅ Good reference for OpenPGP card commands
- ✅ Rust implementation

**Cons:**

- ❌ Client-side (for using cards)
- ❌ Not for card emulation

**Verdict:** ⭐⭐ Reference for OpenPGP app layer only

### C-Based References

#### 7. **vsmartcard Project** (C/Python)

- **GitHub**: https://github.com/frankmorgner/vsmartcard
- **Components**: virtualsmartcard, vpcd, ccid-emulator
- **Status**: Production-quality, widely used

**Architecture:**

```
┌──────────┐     TCP      ┌──────┐     PC/SC    ┌─────────┐
│  vpicc   │ ◄────────────► vpcd  │ ◄───────────► pcscd   │
│ (card)   │   port 35963  │(drv) │              │ daemon  │
└──────────┘               └──────┘              └─────────┘
```

**Pros:**

- ✅ Complete virtual smart card solution
- ✅ Works with real PC/SC stack
- ✅ Mature and tested

**Cons:**

- ❌ C/Python implementation
- ❌ Requires vpcd daemon
- ❌ Not USB/IP based

**Verdict:** ⭐⭐⭐⭐ Excellent reference, different architecture

### C Library - Not Recommended

**libccid** (Official CCID driver):

- ❌ Reader driver (wrong direction)
- ❌ For reading cards, not emulating readers
- ⭐ Not suitable

### Recommended Approach (UPDATED)

Based on research findings, **two viable approaches exist**:

#### **Approach A: Use vpicc + vsmartcard (Recommended for Simplicity)**

This avoids implementing USB CCID entirely by using the existing vsmartcard infrastructure.

**Architecture:**

```
┌───────────┐    TCP     ┌──────┐    PC/SC    ┌─────────┐
│  NetGet   │ ◄─────────► vpcd  │ ◄──────────► pcscd   │
│  + vpicc  │  port 35963│ drv  │             │ daemon  │
└───────────┘            └──────┘             └─────────┘
```

**Steps:**

1. Use `vpicc` crate to implement smart card logic (2-3 days)
2. Implement card filesystem (ISO 7816-4) (3-4 days)
3. Add basic APDU commands (SELECT, READ, VERIFY) (2-3 days)
4. Add one card application (PIV or OpenPGP) (4-6 days)
5. LLM integration (events, PIN prompts) (1-2 days)

**Total Effort**: 12-18 days (no USB/IP needed!)

**Pros:**

- ✅ Reuses mature vsmartcard infrastructure
- ✅ No USB CCID implementation needed
- ✅ Works with real PC/SC stack
- ✅ Pure Rust card logic via vpicc crate

**Cons:**

- ⚠️ Requires vpcd daemon (external dependency)
- ⚠️ Not true USB device (uses TCP proxy)
- ⚠️ Less integrated with NetGet's USB/IP approach

**Verdict:** ⭐⭐⭐⭐⭐ **RECOMMENDED** - Much simpler than USB/IP CCID

#### **Approach B: Use usbd-ccid + USB/IP (Consistent with NetGet)**

This implements true USB CCID device like other NetGet USB protocols.

**Steps:**

1. Evaluate `usbd-ccid` for USB/IP compatibility (1 day)
2. Adapt usbd-ccid to work with USB/IP (3-4 days)
3. Implement ISO 7816-3 (ATR, T=0/T=1) (2-3 days)
4. Implement ISO 7816-4 (file system, APDUs) (3-4 days)
5. Add card application (4-6 days)
6. LLM integration (1-2 days)

**Total Effort**: 14-20 days

**Pros:**

- ✅ Consistent with NetGet's USB/IP approach
- ✅ True USB device (no external daemon)
- ✅ Rust implementation via usbd-ccid

**Cons:**

- ⚠️ usbd-ccid designed for embedded, may need adaptation
- ⚠️ More complex than vpicc approach
- ⚠️ Less tested path

**Verdict:** ⭐⭐⭐⭐ Viable but more complex

**Recommendation: Start with Approach A (vpicc)** unless USB/IP consistency is critical

## Required Crates (UPDATED)

### Approach A: vpicc + vsmartcard (Recommended)

```toml
# Virtual smart card implementation
vpicc = "0.1"              # VSmartCard trait for card logic

# APDU handling
apdu = "0.1"               # APDU command/response types

# ISO 7816 (if needed)
iso7816_tx = "0.1"         # T=1 transmission protocol

# Cryptography (for card applications)
rsa = "0.9"                # RSA operations
p256 = "0.13"              # ECC P-256
sha2 = "0.10"              # SHA-256
aes = "0.8"                # AES encryption

# Reference for card apps
openpgp-card = "*"         # Reference implementation (client-side)
```

**System Requirements:**

```bash
# Install vsmartcard's vpcd daemon
sudo apt-get install vpcd  # Ubuntu/Debian
# OR compile from source: https://github.com/frankmorgner/vsmartcard
```

### Approach B: usbd-ccid + USB/IP

```toml
# CCID device implementation
usbd-ccid = "*"            # CCID protocol (needs evaluation)

# USB gadget (if needed)
usb-gadget = "0.7"         # Linux USB gadget API

# ISO 7816 protocols
iso7816_tx = "0.1"         # T=1 transmission
apdu = "0.1"               # APDU types

# Cryptography (same as Approach A)
rsa = "0.9"
p256 = "0.13"
sha2 = "0.10"
aes = "0.8"
```

### Not Recommended (Client-Side)

```toml
# These are for USING cards, not implementing them:
# pcsc = "*"              # PC/SC client library
# pcsc-sys = "*"          # PC/SC FFI
# openpgp-card = "*"      # OpenPGP card client
```

## CCID Class Descriptor

The CCID class-specific descriptor provides device capabilities:

```rust
pub fn build_ccid_class_descriptor() -> Vec<u8> {
    vec![
        0x36,                    // bLength (54 bytes)
        0x21,                    // bDescriptorType (CCID)
        0x10, 0x01,              // bcdCCID (version 1.10)
        0x00,                    // bMaxSlotIndex (1 slot)
        0x07,                    // bVoltageSupport (5V, 3V, 1.8V)
        0x01, 0x00, 0x00, 0x00,  // dwProtocols (T=0 supported)
        0x10, 0x0E, 0x00, 0x00,  // dwDefaultClock (3.6 MHz)
        0x10, 0x0E, 0x00, 0x00,  // dwMaximumClock (3.6 MHz)
        0x00,                    // bNumClockSupported (default only)
        0x80, 0x25, 0x00, 0x00,  // dwDataRate (9600 bps)
        0x80, 0x25, 0x00, 0x00,  // dwMaxDataRate (9600 bps)
        0x00,                    // bNumDataRatesSupported (default)
        0xFE, 0x00, 0x00, 0x00,  // dwMaxIFSD (254 bytes)
        0x00, 0x00, 0x00, 0x00,  // dwSynchProtocols
        0x00, 0x00, 0x00, 0x00,  // dwMechanical
        0xBA, 0x04, 0x01, 0x00,  // dwFeatures
        0x0F, 0x01, 0x00, 0x00,  // dwMaxCCIDMessageLength (271 bytes)
        0x00,                    // bClassGetResponse (echo)
        0x00,                    // bClassEnvelope (echo)
        0x00, 0x00,              // wLcdLayout (no LCD)
        0x00,                    // bPINSupport (no PIN pad)
        0x01,                    // bMaxCCIDBusySlots (1 slot)
    ]
}
```

## CCID Message Format

### PC_to_RDR_XfrBlock (APDU Exchange)

```
Offset  Size  Description
0       1     bMessageType (0x6F)
1       4     dwLength (length of abData)
5       1     bSlot (slot number, 0)
6       1     bSeq (sequence number)
7       1     bBWI (block waiting time extension)
8       2     wLevelParameter (0x0000 for character level)
10      N     abData (APDU command)
```

### RDR_to_PC_DataBlock (APDU Response)

```
Offset  Size  Description
0       1     bMessageType (0x80)
1       4     dwLength (length of abData)
5       1     bSlot (slot number, 0)
6       1     bSeq (sequence number from command)
7       1     bStatus (0x00 = success, 0x40 = time extension)
8       1     bError (error code if status != 0)
9       1     bChainParameter (0x00 = single block)
10      N     abData (APDU response with SW1/SW2)
```

## Example APDU Commands

### SELECT Master File

```
Command:  00 A4 00 0C 02 3F 00
Response: 90 00

00: CLA (interindustry command)
A4: INS (SELECT)
00: P1 (select by file ID)
0C: P2 (no response data)
02: Lc (2 bytes of data)
3F 00: File ID of Master File (MF)
90 00: SW1/SW2 (success)
```

### VERIFY PIN

```
Command:  00 20 00 00 08 31 32 33 34 35 36 37 38
Response: 90 00

00: CLA
20: INS (VERIFY)
00: P1 (no specific qualification)
00: P2 (reference data number 0)
08: Lc (8 bytes of PIN)
31-38: PIN "12345678" (ASCII)
90 00: Success
```

### READ BINARY

```
Command:  00 B0 00 00 10
Response: [16 bytes of data] 90 00

00: CLA
B0: INS (READ BINARY)
00 00: P1/P2 (offset 0)
10: Le (read 16 bytes)
```

## ISO 7816-4 File System

Smart cards use a hierarchical file system:

```
MF (Master File, 3F00)
├── EF (Elementary File, e.g., 2F01 - Application label)
├── DF (Dedicated File, e.g., A000 - Application directory)
│   ├── EF (e.g., 5031 - Key file)
│   └── EF (e.g., 5032 - Certificate file)
└── DF (Another application)
```

**File Types:**

- **MF**: Root of file system (one per card)
- **DF**: Directory file (can contain DFs and EFs)
- **EF**: Elementary file (contains actual data)
    - **Transparent**: Binary data
    - **Linear Fixed**: Fixed-size records
    - **Linear Variable**: Variable-size records
    - **Cyclic**: Ring buffer of records

## Limitations

### Server Side (Implementation)

- **No Built-in Support**: usbip crate has no CCID handlers
- **Extremely Complex**: CCID + ISO 7816 + card apps
- **Binary Protocols**: CCID messages, APDU parsing
- **File System**: Must implement full ISO 7816-4 FS
- **Very High Effort**: 18-28+ days implementation time

### Client Side (Same as other USB protocols)

- **Linux Only**: Requires vhci-hcd kernel module
- **Root Access**: Client must run `sudo usbip attach`
- **pcscd Required**: PC/SC daemon must be running
- **No Windows/macOS Client**: Limited to Linux hosts

### Protocol

- **Single Card**: One card per reader
- **No Multi-App**: Complex to support multiple applets
- **No Real Crypto**: Cryptographic operations simulated
- **Limited Compatibility**: May not work with all PC/SC apps

## LLM Integration Challenges

### Challenge 1: Card Insertion

CCID requires card insertion/removal events. LLM must:

- Trigger `insert_card` action to make card available
- Trigger `remove_card` action to eject card
- Handle hot-plug notifications

### Challenge 2: PIN Verification

Smart cards require PIN entry. LLM should:

- Intercept VERIFY APDUs
- Prompt user for PIN (or use stored PIN)
- Handle PIN retry counters
- Block card after failed attempts

### Challenge 3: APDU Interpretation

LLM receives raw APDUs. To be useful:

- Parse APDU structure (CLA, INS, etc.)
- Interpret command meaning (SELECT, READ, SIGN)
- Provide human-readable descriptions
- Log operations for transparency

### Challenge 4: Application Logic

Card applications have complex logic:

- PIV: Authentication, digital signature, key management
- OpenPGP: Encryption, signing, decryption
- Custom apps: Application-specific commands

## Testing Strategy

### Manual Testing

1. **Compile** (after implementation):
   ```bash
   ./cargo-isolated.sh build --no-default-features --features usb-smartcard
   ```

2. **Start Server**:
   ```bash
   ./target-claude/*/debug/netget --protocol usb-smartcard --listen 0.0.0.0:3240
   ```

3. **Attach from Linux Client**:
   ```bash
   sudo modprobe vhci-hcd
   sudo usbip list -r <server_ip>
   sudo usbip attach -r <server_ip>:3240 -b 1-1
   ```

4. **Verify PC/SC Detection**:
   ```bash
   pcsc_scan
   # Should show virtual reader with card
   ```

5. **Test with OpenSC Tools**:
   ```bash
   opensc-tool --list-readers
   opensc-tool --reader 0 --send-apdu 00:A4:00:0C:02:3F:00
   ```

6. **Test PIV Operations** (if PIV implemented):
   ```bash
   yubico-piv-tool -a status
   pkcs11-tool --module /usr/lib/opensc-pkcs11.so -l -t
   ```

### E2E Tests (Deferred)

**Not yet implemented** due to extreme complexity. Future tests should:

- Test card insertion/removal
- Test SELECT, READ, VERIFY APDUs
- Test PIV authentication workflow
- Test certificate storage and retrieval
- **Budget**: < 10 LLM calls (most operations are APDU exchanges)

## Build Requirements

### System Dependencies

Same as other USB protocols:

```bash
# Ubuntu/Debian
sudo apt-get install libusb-1.0-0-dev pkg-config pcscd pcsc-tools

# Fedora/RHEL
sudo dnf install libusb1-devel pkgconfig pcsc-lite pcsc-tools

# macOS
brew install libusb pkg-config pcsc-lite
```

### Cargo.toml

```toml
[features]
usb-smartcard = ["usb-common"]  # Add crypto crates if implementing card apps

[dependencies]
# No specific CCID crates - custom implementation
```

## Security Considerations

### PIN Protection

- Hash PINs before storage (SHA-256)
- Implement retry counters (3-5 attempts)
- Block card permanently after exhaustion
- Support PIN change operations

### File Access Control

- Implement security attributes per ISO 7816-4
- CHV (Card Holder Verification) required for sensitive files
- Read/write permissions per file
- Secure messaging (optional)

### Cryptographic Operations

- Use secure random for key generation
- Never expose private keys
- Implement proper key usage restrictions
- Support key backup (encrypted)

### Transport Security

- USB/IP is unencrypted (local network only)
- Consider TLS wrapper for remote scenarios
- Validate all APDU parameters
- Sanitize responses (no data leaks)

## Future Enhancements

### Phase 2: Advanced Features

- Multiple card slots
- Contactless card support (ISO 14443)
- T=1 protocol support
- Extended APDU (up to 65535 bytes)

### Phase 3: Card Applications

- Complete PIV implementation (NIST SP 800-73)
- Complete OpenPGP card (OpenPGP Card spec)
- Banking cards (EMV)
- eID cards (national ID)

### Phase 4: Cryptography

- RSA 2048/4096 key generation
- ECC P-256/P-384 operations
- AES encryption/decryption
- Secure key storage (hardware-backed)

### Phase 5: LLM Features

- Automatic card provisioning
- Certificate enrollment
- Key backup and recovery
- Usage analytics and monitoring

## References

- **USB CCID Specification**: https://www.usb.org/sites/default/files/DWG_Smart-Card_CCID_Rev110.pdf
- **ISO/IEC 7816-1**: Physical characteristics
- **ISO/IEC 7816-2**: Dimensions and location of contacts
- **ISO/IEC 7816-3**: Electrical interface and transmission protocols
- **ISO/IEC 7816-4**: Organization, security and commands for interchange
- **ISO/IEC 7816-8**: Commands for security operations
- **PC/SC Workgroup**: https://pcscworkgroup.com/
- **vsmartcard Project**: https://github.com/frankmorgner/vsmartcard
- **OpenSC Project**: https://github.com/OpenSC/OpenSC
- **PIV Standard (NIST SP 800-73)**: https://csrc.nist.gov/publications/detail/sp/800-73/4/final
- **OpenPGP Card Spec**: https://gnupg.org/ftp/specs/OpenPGP-smart-card-application-3.4.pdf

## Recommendation (UPDATED)

**Implementation Priority**: **MEDIUM-LOW** (upgraded from LOW)

**Rationale for Upgrade:**

- **vpicc crate exists** - Rust interface to mature vsmartcard
- Effort reduced from 18-28+ days to **12-18 days** with vpicc
- Can reuse vpcd daemon (no USB CCID needed)
- Good demonstration of LLM managing crypto operations

**Updated Assessment:**

**WITH vpicc + vsmartcard (Approach A):**

- ✅ Feasible in moderate timeframe (12-18 days)
- ✅ Mature vsmartcard infrastructure
- ✅ No USB CCID implementation needed
- ⚠️ Requires external vpcd daemon
- ⚠️ Still complex (card filesystem, applications)
- ⚠️ Different architecture than USB/IP

**WITH usbd-ccid (Approach B):**

- ⚠️ More complex (14-20 days)
- ⚠️ usbd-ccid adaptation needed
- ✅ Consistent with NetGet's USB/IP approach
- ⚠️ Less tested path

**Implementation Recommendation:**

**Phase 1: Verify approach viability** (1-2 days)

1. Test vpicc crate with vpcd daemon
2. Evaluate usbd-ccid for USB/IP compatibility
3. Choose approach based on compatibility

**Phase 2A: If using vpicc** (11-16 days)

1. Implement basic virtual card with vpicc (2-3 days)
2. Add ISO 7816-4 file system (3-4 days)
3. Implement core APDU commands (2-3 days)
4. Add one card application (PIV or OpenPGP) (4-6 days)
5. LLM integration and testing (1-2 days)

**Phase 2B: If using usbd-ccid** (13-18 days)

1. Adapt usbd-ccid for USB/IP (3-4 days)
2. Implement ISO 7816-3/7816-4 (5-7 days)
3. Add card application (4-6 days)
4. LLM integration and testing (1-2 days)

**Best suited for:**

- PKI and certificate management demos
- Smart card protocol education
- Secure authentication workflows
- Development environments without physical cards
- Custom card application testing

**Prerequisite:** Implement simpler protocols first (✅ keyboard, ✅ mouse, ✅ storage, ⚠️ FIDO2)

**Final Verdict:** More feasible than initially assessed, but still complex. Consider after FIDO2.
