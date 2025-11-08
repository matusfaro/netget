# USB FIDO2/U2F Security Key Server Implementation

## Overview

The USB FIDO2/U2F server creates a virtual security key (like a YubiKey) using the USB/IP protocol. This allows an LLM to control a virtual FIDO2 authenticator for passwordless authentication and two-factor authentication (2FA) with WebAuthn-compatible services.

## Architecture

### USB/IP + FIDO2 Protocol Stack

```
┌─────────────────┐                    ┌──────────────────┐
│  NetGet Server  │                    │  Linux Client    │
│  (USB/IP FIDO2) │ ◄────── TCP ─────► │  (vhci-hcd)      │
│  Port: 3240     │                    │  usbip attach    │
└─────────────────┘                    └──────────────────┘
         │                                      │
         │ Creates virtual                     │ Sees as
         │ FIDO2 security key                  │ /dev/hidraw*
         ▼                                     ▼
    [HID Descriptors]                     [Security Key]
    [CTAPHID Protocol]                    [WebAuthn/U2F]
    [CTAP1/CTAP2 Commands]
    [Credential Storage]
```

### Protocol Layers

1. **USB Layer**: USB/IP protocol for device virtualization
2. **HID Layer**: Human Interface Device class (0x03)
3. **CTAPHID Layer**: CTAP HID transport with packet framing
4. **CTAP Layer**: CTAP1 (U2F) and CTAP2 (FIDO2) commands
5. **Crypto Layer**: Public key cryptography (ECDSA, EdDSA)
6. **Storage Layer**: Credential database (encrypted)

## Protocol Specifications

### USB HID FIDO Device

**Device Class**: HID (0x03)
**Subclass**: None (0x00)
**Protocol**: None (0x00)
**Report Descriptor**: FIDO CTAPHID (64-byte packets)

### CTAPHID Protocol

CTAPHID is the HID transport protocol for CTAP messages:

- **Packet Size**: 64 bytes (includes 7-byte header + 57-byte payload)
- **Channel IDs**: 32-bit identifiers for routing
- **Commands**: INIT, MSG, PING, CANCEL, ERROR, KEEPALIVE, WINK, LOCK, CBOR

### CTAP1 (U2F) Commands

Legacy U2F protocol (APDU-like binary format):

- **U2F_REGISTER** (0x01): Register a new credential
- **U2F_AUTHENTICATE** (0x02): Authenticate with existing credential
- **U2F_VERSION** (0x03): Get U2F version string

### CTAP2 (FIDO2) Commands

Modern FIDO2 protocol (CBOR-encoded):

- **authenticatorMakeCredential** (0x01): Create a new credential
- **authenticatorGetAssertion** (0x02): Authenticate with credential
- **authenticatorGetInfo** (0x04): Get authenticator metadata
- **authenticatorClientPIN** (0x06): PIN management
- **authenticatorReset** (0x07): Factory reset
- **authenticatorGetNextAssertion** (0x08): Get next assertion
- **authenticatorCredentialManagement** (0x0A): Manage credentials
- **authenticatorSelection** (0x0B): User presence test
- **authenticatorBioEnrollment** (0x09): Biometric enrollment (optional)

## Current Status: **Not Yet Implemented**

### What's Required

#### Phase 1: USB HID Device Handler (High Priority)
- ❌ Custom `UsbInterfaceHandler` for FIDO HID
- ❌ HID report descriptor for FIDO (64-byte packets)
- ❌ Interrupt IN endpoint (0x81) for responses
- ❌ Interrupt OUT endpoint (0x01) for requests

#### Phase 2: CTAPHID Transport (High Priority)
- ❌ Packet framing (7-byte header + 57-byte data)
- ❌ Channel management (32-bit channel IDs)
- ❌ Message fragmentation and reassembly
- ❌ Command dispatcher (INIT, MSG, PING, etc.)

#### Phase 3: CTAP1 (U2F) Implementation (Medium Priority)
- ❌ U2F_REGISTER command
- ❌ U2F_AUTHENTICATE command
- ❌ U2F_VERSION command
- ❌ APDU parsing (ISO 7816-4 format)
- ❌ Counter management (signature counter)

#### Phase 4: CTAP2 (FIDO2) Implementation (Medium Priority)
- ❌ authenticatorMakeCredential (CBOR)
- ❌ authenticatorGetAssertion (CBOR)
- ❌ authenticatorGetInfo (device metadata)
- ❌ PIN protocol support (optional)
- ❌ Credential management (optional)
- ❌ CBOR encoding/decoding

#### Phase 5: Cryptography (High Priority)
- ❌ ECDSA signature generation (P-256)
- ❌ EdDSA signature generation (Ed25519, optional)
- ❌ Key pair generation
- ❌ SHA-256 hashing
- ❌ HMAC-SHA-256 (for attestation)

#### Phase 6: Credential Storage (High Priority)
- ❌ Credential database (in-memory or file-based)
- ❌ Encrypted storage with passphrase
- ❌ Credential ID generation
- ❌ Relying party (RP) ID management
- ❌ User handle association

#### Phase 7: LLM Integration (Low Priority)
- ❌ User presence approval (LLM prompt)
- ❌ User verification (PIN simulation)
- ❌ Event generation (register, authenticate, reset)
- ❌ Actions (approve_request, deny_request, set_pin)

#### Phase 8: Testing (Deferred)
- ❌ E2E tests with real browsers (Chrome, Firefox)
- ❌ U2F registration and authentication
- ❌ FIDO2/WebAuthn registration and authentication
- ❌ Multi-credential management

## Implementation Complexity

### Estimated Effort

- **Phase 1** (USB HID handler): 1-2 days
- **Phase 2** (CTAPHID transport): 2-3 days
- **Phase 3** (CTAP1/U2F): 2-3 days
- **Phase 4** (CTAP2/FIDO2): 3-4 days
- **Phase 5** (Cryptography): 2-3 days
- **Phase 6** (Credential storage): 1-2 days
- **Phase 7** (LLM integration): 1-2 days
- **Phase 8** (Testing): 2-3 days

**Total**: 14-22 days for full implementation

### Complexity Rating: **HIGH**

**Reasons:**
1. **Cryptography**: Requires robust public key cryptography (ECDSA, EdDSA)
2. **Binary Protocols**: Complex binary framing (CTAPHID) and encoding (CBOR)
3. **Security**: Must handle credentials securely (encryption, key storage)
4. **Dual Protocol**: Must support both CTAP1 (U2F) and CTAP2 (FIDO2)
5. **State Management**: Channel IDs, fragmentation, counter persistence

## Library Choices

### Option 1: Implement from Scratch

**Pros:**
- Full control over implementation
- Can tailor to LLM integration needs
- No external dependencies on authenticator libraries

**Cons:**
- Very complex (14-22 days effort)
- Requires cryptography expertise
- Hard to get right (security-critical)

### Option 2: Use OpenSK as Reference

**OpenSK** (Google's open-source FIDO2 authenticator in Rust):
- **GitHub**: https://github.com/google/OpenSK
- **Status**: FIDO Alliance certified
- **Features**: CTAP 2.0 implementation, credential management, PIN support

**Pros:**
- Production-quality reference implementation
- Written in Rust
- FIDO-certified implementation

**Cons:**
- Designed for embedded devices, not USB/IP
- Would need significant adaptation
- Large codebase to understand

### Option 3: Adapt Virtual FIDO

**Virtual FIDO** (bulwarkid/virtual-fido in Go):
- **GitHub**: https://github.com/bulwarkid/virtual-fido
- **Status**: Beta, active development
- **Features**: USB/IP, CTAP1/CTAP2, credential storage

**Pros:**
- Already uses USB/IP for virtualization
- Simpler implementation than OpenSK
- Designed for virtual devices

**Cons:**
- Written in Go, not Rust
- Would need full port to Rust
- Beta quality, APIs may change

### Recommended Approach

**Incremental Implementation:**
1. Start with USB HID device + CTAPHID transport (Phases 1-2)
2. Implement minimal U2F (CTAP1) for basic 2FA (Phase 3)
3. Add cryptography using `ring` or `ed25519-dalek` crates (Phase 5)
4. Add credential storage (Phase 6)
5. Extend to FIDO2 (CTAP2) if needed (Phase 4)

## Required Crates

### Cryptography

```toml
ring = "0.17"              # ECDSA P-256, SHA-256, random
# OR
p256 = "0.13"              # ECDSA P-256
ed25519-dalek = "2.0"      # EdDSA Ed25519 (optional)
sha2 = "0.10"              # SHA-256
```

### CBOR Encoding (for CTAP2)

```toml
serde_cbor = "0.11"        # CBOR serialization
# OR
ciborium = "0.2"           # Modern CBOR implementation
```

### HID Report Descriptor

```toml
# No specific crate needed - manually construct descriptor bytes
# Reference: FIDO Alliance CTAPHID specification
```

## HID Report Descriptor (FIDO)

The FIDO HID report descriptor defines 64-byte input/output reports:

```rust
pub fn build_fido_hid_report_descriptor() -> Vec<u8> {
    vec![
        0x06, 0xD0, 0xF1,  // Usage Page (FIDO Alliance)
        0x09, 0x01,        // Usage (U2F Authenticator Device)
        0xA1, 0x01,        // Collection (Application)
        0x09, 0x20,        //   Usage (Input Report Data)
        0x15, 0x00,        //   Logical Minimum (0)
        0x26, 0xFF, 0x00,  //   Logical Maximum (255)
        0x75, 0x08,        //   Report Size (8 bits)
        0x95, 0x40,        //   Report Count (64 bytes)
        0x81, 0x02,        //   Input (Data, Variable, Absolute)
        0x09, 0x21,        //   Usage (Output Report Data)
        0x15, 0x00,        //   Logical Minimum (0)
        0x26, 0xFF, 0x00,  //   Logical Maximum (255)
        0x75, 0x08,        //   Report Size (8 bits)
        0x95, 0x40,        //   Report Count (64 bytes)
        0x91, 0x02,        //   Output (Data, Variable, Absolute)
        0xC0,              // End Collection
    ]
}
```

## CTAPHID Packet Format

### Initialization Packet (first packet)

```
Byte 0-3:  Channel ID (CID) - 32-bit identifier
Byte 4:    Command byte | 0x80 (initialization flag)
Byte 5-6:  Payload length (big-endian, 16-bit)
Byte 7-63: Data (57 bytes)
```

### Continuation Packet (subsequent packets)

```
Byte 0-3:  Channel ID (CID) - same as initialization
Byte 4:    Sequence number (0x00 - 0x7F)
Byte 5-63: Data (59 bytes)
```

## CTAP1 (U2F) Example Flow

### Registration

1. **Browser → Authenticator**: U2F_REGISTER
   - Application parameter (32 bytes, SHA-256 of app ID)
   - Challenge parameter (32 bytes, SHA-256 of client data)

2. **Authenticator → LLM**: Request user presence approval

3. **Authenticator → Browser**: Registration response
   - Reserved byte (0x05)
   - Public key (65 bytes, uncompressed P-256)
   - Key handle length (1 byte)
   - Key handle (variable)
   - Attestation certificate (DER-encoded X.509)
   - Signature (ECDSA over registration data)

### Authentication

1. **Browser → Authenticator**: U2F_AUTHENTICATE
   - Control byte (0x07 = check-only, 0x03 = enforce-user-presence)
   - Application parameter (32 bytes)
   - Challenge parameter (32 bytes)
   - Key handle length (1 byte)
   - Key handle (variable)

2. **Authenticator → Browser**: Authentication response
   - User presence byte (0x01)
   - Counter (4 bytes, big-endian)
   - Signature (ECDSA over response data)

## CTAP2 (FIDO2) Example Flow

### MakeCredential (Registration)

**Request** (CBOR map):
```
0x01: clientDataHash (32 bytes)
0x02: rp (relying party map)
      - "id": string
      - "name": string
0x03: user (user map)
      - "id": bytes
      - "name": string
      - "displayName": string
0x04: pubKeyCredParams (array of algorithm descriptors)
      - "type": "public-key"
      - "alg": -7 (ES256) or -8 (EdDSA)
0x07: options (map, optional)
      - "rk": resident key
      - "uv": user verification
```

**Response** (CBOR map):
```
0x01: fmt (attestation format, "packed")
0x02: authData (authenticator data)
0x03: attStmt (attestation statement)
```

### GetAssertion (Authentication)

**Request** (CBOR map):
```
0x01: rpId (string)
0x02: clientDataHash (32 bytes)
0x03: allowList (array of credential descriptors, optional)
0x05: options (map, optional)
```

**Response** (CBOR map):
```
0x01: credential (credential descriptor, optional)
0x02: authData (authenticator data)
0x03: signature (ECDSA signature)
0x04: user (user map, if resident key)
0x05: numberOfCredentials (if multiple credentials)
```

## Limitations

### Server Side (Implementation)
- **No Built-in Support**: usbip crate has no FIDO2 handlers
- **Complex Cryptography**: Requires ECDSA, EdDSA, SHA-256
- **Binary Protocols**: CTAPHID framing, CBOR encoding
- **Security Critical**: Credential storage, key management
- **High Effort**: 14-22 days estimated implementation time

### Client Side (Same as other USB protocols)
- **Linux Only**: Requires vhci-hcd kernel module (Linux 3.17+)
- **Root Access**: Client must run `sudo usbip attach`
- **Manual Import**: User must run attach command
- **No Windows/macOS Client**: Limited to Linux hosts

### Protocol
- **User Presence**: LLM must approve every authentication
- **No Biometrics**: No fingerprint/face recognition support
- **No Resident Keys**: Credentials stored on server, not device
- **Single User**: No multi-user credential management

## LLM Integration Challenges

### Challenge 1: User Presence
FIDO2 requires user presence confirmation (button press). LLM must:
- Receive approval prompts for each authentication
- Respond within timeout (typically 30 seconds)
- Handle concurrent requests (multiple tabs)

### Challenge 2: Credential Management
LLM should be able to:
- List registered credentials
- Delete credentials
- View relying party information
- Export/import credential database

### Challenge 3: PIN Management
FIDO2 PIN support requires:
- Secure PIN storage (hashed)
- PIN verification protocol
- PIN retry counter
- PIN change workflow

## Testing Strategy

### Manual Testing

1. **Compile** (after implementation):
   ```bash
   ./cargo-isolated.sh build --no-default-features --features usb-fido2
   ```

2. **Start Server**:
   ```bash
   ./target-claude/*/debug/netget --protocol usb-fido2 --listen 0.0.0.0:3240
   ```

3. **Attach from Linux Client**:
   ```bash
   sudo modprobe vhci-hcd
   sudo usbip list -r <server_ip>
   sudo usbip attach -r <server_ip>:3240 -b 1-1
   ```

4. **Test U2F Registration** (using Chrome/Firefox):
   - Navigate to demo site (e.g., webauthn.io)
   - Register security key
   - Verify credential creation

5. **Test U2F Authentication**:
   - Attempt login with security key
   - Verify authentication succeeds

### E2E Tests (Deferred)

**Not yet implemented** due to complexity. Future E2E tests should:
- Test U2F registration and authentication
- Test FIDO2 registration and authentication
- Test credential management
- Test PIN operations
- **Budget**: < 10 LLM calls (reuse key, test multiple scenarios)

## Build Requirements

### System Dependencies

Same as other USB protocols:

```bash
# Ubuntu/Debian
sudo apt-get install libusb-1.0-0-dev pkg-config

# Fedora/RHEL
sudo dnf install libusb1-devel pkgconfig

# macOS
brew install libusb pkg-config
```

### Cargo.toml

```toml
[features]
usb-fido2 = ["usb-common", "ring", "serde_cbor"]

[dependencies]
ring = { version = "0.17", optional = true }
serde_cbor = { version = "0.11", optional = true }
```

## Security Considerations

### Credential Protection
- Store credentials encrypted at rest
- Use secure random for key generation
- Never log private keys or credentials

### Attestation
- Use self-signed attestation certificate
- Or batch attestation (shared cert)
- Or no attestation (anonymity mode)

### Counter Management
- Persist signature counter to prevent replay
- Increment on every authentication
- Handle counter overflow gracefully

### Transport Security
- USB/IP is unencrypted (local network only)
- Consider TLS wrapper for remote scenarios
- Validate all input parameters

## Future Enhancements

### Phase 2: Advanced Features
- Resident keys (credentials stored on device)
- Biometric enrollment (simulated)
- Multiple user accounts
- Credential backup/restore

### Phase 3: Extensions
- HMAC-secret extension
- CredProtect extension
- Large blob storage
- MinPinLength extension

### Phase 4: LLM Features
- Contextual approval (trust certain sites)
- Automatic renewal prompts
- Security analytics (unusual login patterns)
- Credential recommendations

## References

- **FIDO Alliance Specifications**: https://fidoalliance.org/specifications/
- **CTAP 2.1 Spec**: https://fidoalliance.org/specs/fido-v2.1-ps-20210615/fido-client-to-authenticator-protocol-v2.1-ps-20210615.html
- **CTAPHID Spec**: Included in CTAP specification
- **U2F Raw Message Formats**: https://fidoalliance.org/specs/fido-u2f-v1.2-ps-20170411/fido-u2f-raw-message-formats-v1.2-ps-20170411.html
- **WebAuthn API**: https://www.w3.org/TR/webauthn-2/
- **OpenSK (Google)**: https://github.com/google/OpenSK
- **Virtual FIDO (Go)**: https://github.com/bulwarkid/virtual-fido
- **ring crate (crypto)**: https://docs.rs/ring/
- **serde_cbor crate**: https://docs.rs/serde_cbor/

## Recommendation

**Implementation Priority**: **MEDIUM-LOW**

**Rationale:**
- Very complex implementation (14-22 days)
- Requires deep cryptography knowledge
- Security-critical (credential management)
- Existing solutions (hardware keys) work well
- Limited LLM integration value (mostly approvals)

**Better suited for**:
- Security research and testing
- FIDO2 protocol education
- Custom authentication workflows
- Situations where hardware keys unavailable

**Consider implementing simpler USB protocols first** (keyboard, mouse, storage) before attempting FIDO2.
