# SSH Agent Protocol Research & Implementation Guide for NetGet

## Executive Summary

The SSH Agent Protocol is a request-response protocol for cryptographic key management. Unlike SSH itself (which is a remote access protocol), SSH Agent enables local and remote clients to request cryptographic operations (like signing) on keys stored securely in an agent process. This document provides a comprehensive specification for implementing SSH Agent server and client in NetGet.

---

## 1. Protocol Specification

### 1.1 Overview

**Purpose**: SSH Agent is a key management service that:
- Stores private keys securely in memory
- Performs cryptographic operations (signing, decryption) on behalf of clients
- Prevents private keys from being exposed to clients
- Supports key access control through constraints and restrictions

**Specification Documents**:
- **Primary**: IETF Draft - [draft-ietf-sshm-ssh-agent-05](https://datatracker.ietf.org/doc/draft-ietf-sshm-ssh-agent/)
- **Extensions**: OpenSSH PROTOCOL.agent (vendor extensions)
- **Related RFCs**: RFC 4251 (SSH Architecture), RFC 4253 (SSH Transport)

**Status**: Widely implemented, standardized via IETF draft, used universally in SSH infrastructure

### 1.2 Wire Format & Encoding

**Core Structure**: All SSH Agent messages use SSH wire format (RFC 4251) with length-prefixed encoding:

```
[uint32: message_length][byte: message_type][byte[message_length - 1]: contents]
```

**Data Type Encoding** (RFC 4251):
- `byte` (uint8) - Single octet (0-255)
- `boolean` - Encoded as `byte` (0 = false, 1 = true)
- `uint32` - 32-bit unsigned integer (big-endian)
- `uint64` - 64-bit unsigned integer (big-endian)
- `string` - `[uint32: length][byte[length]: data]`
- `name-list` - Comma-separated string of algorithm/type names

**Example Message Flow**:
```
Client Request (SSH_AGENTC_REQUEST_IDENTITIES):
  byte 0-3:   0x00000001          (length = 1 byte of content)
  byte 4:     0x0B                (SSH_AGENTC_REQUEST_IDENTITIES)

Server Response (SSH_AGENT_IDENTITIES_ANSWER):
  byte 0-3:   0x0000001C          (length = 28 bytes)
  byte 4:     0x0C                (SSH_AGENT_IDENTITIES_ANSWER)
  byte 5-8:   0x00000001          (nkeys = 1)
  byte 9-12:  0x0000000B          (key blob length = 11)
  byte 13-23: "ssh-ed25519..."    (public key blob)
  byte 24-27: 0x00000003          (comment length = 3)
  byte 28-30: "key"               (comment)
```

---

## 2. Transport

### 2.1 Connection Types

SSH Agent supports multiple transport mechanisms:

#### **Unix Domain Sockets** (Primary on Linux/macOS)
- **Path Pattern**: `/tmp/ssh-XXXXXXXXXX/agent.YYYY`
  - `XXXXXXXXXX`: Random alphanumeric string (10 chars)
  - `YYYY`: Process ID of ssh-agent
- **Permissions**: Socket file has mode 0700 (rwx------)
- **Environment Variable**: `SSH_AUTH_SOCK` points to socket path
- **Example**: `/tmp/ssh-abcdef1234/agent.5678`

**Connection Setup**:
```bash
# Check socket location
echo $SSH_AUTH_SOCK
# Output: /tmp/ssh-abcdef1234/agent.5678

# Connection is persistent TCP-like stream over Unix socket
# Messages are sent back-and-forth continuously
# Socket remains open for agent's lifetime
```

#### **Windows Named Pipes** (Windows)
- **Path Pattern**: `\\.\pipe\openssh-ssh-agent`
- **Example**: `\\.\pipe\openssh-ssh-agent` or custom named pipes
- **Access Control**: Named pipe security descriptors control access

#### **TCP Sockets** (Less common, possible for remote agents)
- **Port**: No standard port (user-configured)
- **Security**: Typically only used with SSH tunnel forwarding
- **Transport**: Plain TCP socket

### 2.2 Session Model

**Connection Semantics**:
- **Stateful**: Single connection carries multiple request-response pairs
- **Synchronous**: Agent responds to each request before processing next
- **Framing**: Length-prefixed messages (no delimiter characters needed)
- **No Multiplexing**: One outstanding request at a time per connection
- **Persistent**: Connection stays open for multiple operations

**Example Session**:
```
Client connects to socket
Client: [length=1][type=11]  → REQUEST_IDENTITIES
Agent:  [length=n][type=12][nkeys=1][...key data...]  → IDENTITIES_ANSWER
Client: [length=m][type=25][...add key data...]  → ADD_ID_CONSTRAINED
Agent:  [length=1][type=6]  → SUCCESS
Client: (continues using same connection)
```

---

## 3. Message Operations & Types

### 3.1 Message Type Codes

**Request Types (SSH_AGENTC_*, client → agent)**:

| Code | Name | Purpose |
|------|------|---------|
| 11 | REQUEST_IDENTITIES | Get list of available keys |
| 13 | SIGN_REQUEST | Request signature on data |
| 17 | ADD_IDENTITY | Add a key (without constraints) |
| 18 | REMOVE_IDENTITY | Remove specific key |
| 19 | REMOVE_ALL_IDENTITIES | Delete all keys |
| 20 | ADD_SMARTCARD_KEY | Add hardware token keys |
| 21 | REMOVE_SMARTCARD_KEY | Remove smartcard keys |
| 22 | LOCK | Lock agent with passphrase |
| 23 | UNLOCK | Unlock agent |
| 25 | ADD_ID_CONSTRAINED | Add key with constraints |
| 26 | ADD_SMARTCARD_KEY_CONSTRAINED | Add smartcard key with constraints |
| 27 | EXTENSION | Vendor extensions (OpenSSH) |

**Response Types (SSH_AGENT_*, agent → client)**:

| Code | Name | Purpose |
|------|------|---------|
| 5 | FAILURE | Generic failure |
| 6 | SUCCESS | Generic success |
| 12 | IDENTITIES_ANSWER | Reply with key list |
| 14 | SIGN_RESPONSE | Return signature data |
| 28 | EXTENSION_FAILURE | Extension not supported |
| 29 | EXTENSION_RESPONSE | Extension-specific reply |

### 3.2 Core Operations

#### **A. REQUEST_IDENTITIES (0x0B)**

**Request Format**:
```
byte type = 11
(no additional data)
```

**Response Format** (IDENTITIES_ANSWER, type 12):
```
byte type = 12
uint32 nkeys                          (number of keys)
  for each key:
    string public_key_blob            (SSH wire format key)
    string comment                    (UTF-8 string)
```

**Example**:
```
REQUEST:  [len=1][type=11]

RESPONSE: [len=100][type=12]
          [nkeys=2]
          [keyblob_len=50][ed25519 key blob]
          [comment_len=8]["my-key1"]
          [keyblob_len=30][rsa key blob]
          [comment_len=8]["my-key2"]
```

#### **B. SIGN_REQUEST (0x0D)**

**Request Format**:
```
byte type = 13
string public_key_blob              (which key to use)
string data                         (data to be signed)
uint32 flags                        (signature flags)
```

**Flags**:
- Bit 0: Use SHA-256 instead of SHA-1/SHA-512 (RSA keys only)
- Bit 1: Use SHA-512 instead of SHA-1/SHA-256 (RSA keys only)

**Response Format** (SIGN_RESPONSE, type 14):
```
byte type = 14
string signature_blob               (SSH wire format signature)
```

**Security Note**: Private key never exposed; signature returned instead.

#### **C. ADD_IDENTITY / ADD_ID_CONSTRAINED (0x11 / 0x19)**

**ADD_IDENTITY (type 17) - Unconditional**:
```
byte type = 17
string key_type                     (e.g., "ssh-ed25519")
string public_key_blob              (public key material)
string private_key_blob             (private key material)
string comment                      (UTF-8 description)
```

**ADD_ID_CONSTRAINED (type 25) - With Restrictions**:
```
byte type = 25
string key_type
string public_key_blob
string private_key_blob
string comment
constraints                         (see section 3.3)
```

**Response**: SSH_AGENT_SUCCESS (type 6) or FAILURE (type 5)

#### **D. REMOVE_IDENTITY (0x12)**

**Request Format**:
```
byte type = 18
string public_key_blob              (which key to remove)
```

**Response**: SUCCESS or FAILURE

#### **E. REMOVE_ALL_IDENTITIES (0x13)**

**Request Format**:
```
byte type = 19
(no additional data)
```

**Response**: SUCCESS or FAILURE

#### **F. LOCK / UNLOCK (0x16 / 0x17)**

**LOCK (type 22)**:
```
byte type = 22
string passphrase                   (lock password)
```

**UNLOCK (type 23)**:
```
byte type = 23
string passphrase                   (unlock password)
```

**Response**: SUCCESS or FAILURE

Agent becomes locked after LOCK; all operations return FAILURE until UNLOCK succeeds.

#### **G. SMARTCARD OPERATIONS (0x14-0x1A)**

**ADD_SMARTCARD_KEY (type 20)**:
```
byte type = 20
string reader_id                    (PKCS#11 module path)
string pin                          (smartcard PIN)
```

**ADD_SMARTCARD_KEY_CONSTRAINED (type 26)**:
```
byte type = 26
string reader_id
string pin
constraints
```

**REMOVE_SMARTCARD_KEY (type 21)**:
```
byte type = 21
string reader_id
string pin
```

**Response**: SUCCESS or FAILURE

### 3.3 Key Constraints

Constraints restrict when/how keys can be used. Specified in ADD_ID_CONSTRAINED and ADD_SMARTCARD_KEY_CONSTRAINED:

**Wire Format**:
```
constraint_type: byte
constraint_data: variable (depends on type)

Supported Types:
  1 = SSH_AGENT_CONSTRAIN_LIFETIME
  2 = SSH_AGENT_CONSTRAIN_CONFIRM
  255 (0xFF) = SSH_AGENT_CONSTRAIN_EXTENSION (OpenSSH)
```

#### **A. Lifetime Constraint (type 1)**

```
byte type = 1
uint32 seconds              (seconds until key expires)
```

Agent automatically deletes key after specified duration. Used to implement temporary access.

#### **B. Confirm Constraint (type 2)**

```
byte type = 2
(no additional data)
```

Agent requires explicit user confirmation (e.g., dialog box) before using key for signing. Each operation requires separate confirmation.

#### **C. Extension Constraints (type 255 / 0xFF)**

OpenSSH-specific constraints via extension mechanism:

```
byte type = 255
string extension_name       (e.g., "restrict-destination-v00@openssh.com")
extension-specific data
```

**Common Extensions**:

1. **session-bind@openssh.com**:
   ```
   string "session-bind@openssh.com"
   string hostkey              (server's public host key)
   string session_id           (key exchange session identifier)
   string signature            (server's signature)
   boolean is_forwarding       (forwarding flag)
   ```

2. **restrict-destination-v00@openssh.com**:
   ```
   string "restrict-destination-v00@openssh.com"
   constraints[]               (per-hop constraints)
   ```

3. **associated-certs-v00@openssh.com** (smartcard certs):
   ```
   string "associated-certs-v00@openssh.com"
   boolean certs_only          (load only certificates, not keys)
   string certsblob            (certificate data)
   ```

### 3.4 Extension Mechanism (type 27 / 0x1B)

**EXTENSION Request (type 27)**:
```
byte type = 27
string extension_name       (e.g., "query@openssh.com", "session-bind@openssh.com")
extension-specific data
```

**Responses**:
- `SSH_AGENT_EXTENSION_RESPONSE` (type 29): Extension understood
- `SSH_AGENT_EXTENSION_FAILURE` (type 28): Extension not supported

**Example - Query Extension**:
```
REQUEST:
  type = 27
  name = "query@openssh.com"

RESPONSE:
  type = 29
  extensions[] = ["session-bind@openssh.com", "restrict-destination-v00@openssh.com"]
```

---

## 4. Rust Libraries & Ecosystem

### 4.1 Recommended Libraries

#### **1. ssh-agent-lib** ⭐ (Best for Server)

**Repository**: https://github.com/wiktor-k/ssh-agent-lib

**Features**:
- Complete agent server implementation
- Client library included
- Async/await support (Tokio)
- Unix sockets + Windows named pipes
- Trait-based `Session` interface for custom agents
- Key types from `ssh_key` crate
- Production-ready, actively maintained

**API**:
```rust
// Implement custom agent
#[async_trait]
impl Session for MyAgent {
    async fn request_identities(&self) 
        -> Result<Vec<Identity>, AgentError>;
    
    async fn sign(&self, request: SignRequest)
        -> Result<Signature, AgentError>;
}

// Start listening
agent::listen(MyAgent::new(), "./ssh-agent.sock").await?;
```

**Dependencies**:
- `tokio` - Async runtime
- `ssh_key` - SSH key types
- `service_binding` - Socket abstraction
- `secrecy` - Sensitive data handling

**License**: MIT/Apache 2.0

#### **2. ssh-agent-client-rs**

**Features**:
- Pure Rust SSH agent client
- Synchronous API (no async)
- Unix socket + Windows named pipe support
- Based on draft-miller-ssh-agent-04
- Cross-platform

**API**:
```rust
let agent = Client::connect()?;
let identities = agent.request_identities()?;
let signature = agent.sign(&key, &data)?;
```

**Use Case**: Connecting to existing agents for signing operations

**License**: MIT/Apache 2.0

#### **3. russh / russh-keys** (Already in NetGet)

**Current Usage**: SSH server protocol
**Agent Support**: Limited; primarily for SSH client auth
**Recommendation**: Use alongside ssh-agent-lib for full protocol

---

## 5. Implementation Recommendations for NetGet

### 5.1 Architecture Overview

**SSH Agent Server**:
- Listens on Unix domain socket (Unix/Linux/macOS) or named pipe (Windows)
- Stores keys in memory (virtual, LLM-controlled)
- Responds to client requests with cryptographic operations
- LLM controls key storage, access decisions, and signing logic

**SSH Agent Client**:
- Connects to existing agent via socket
- Requests signatures/keys from agent
- Used for testing agent implementations or interacting with real agents

### 5.2 Server Implementation Strategy

**Recommended Approach**:
1. Use `ssh-agent-lib` for protocol handling
2. Implement `Session` trait for LLM integration
3. Store keys in-memory with LLM-controlled lifecycle
4. Implement signing operations via LLM

**Example Structure**:
```
src/server/ssh_agent/
├── mod.rs              (Server setup, LLM integration)
├── actions.rs          (Protocol actions)
├── session.rs          (Session trait implementation)
└── key_store.rs        (In-memory key storage)

src/client/ssh_agent/
├── mod.rs              (Client setup)
└── actions.rs          (Client actions)
```

### 5.3 LLM Control Points

#### **Server-Side**:

1. **On Identity Request** (REQUEST_IDENTITIES):
   - Event: `ssh_agent_list_keys_requested`
   - LLM decides: Which keys to return, their comments
   - Action: `ssh_agent_return_identities`
   - Example: Return hardcoded keys or dynamically generate based on instruction

2. **On Key Add** (ADD_IDENTITY/ADD_ID_CONSTRAINED):
   - Event: `ssh_agent_add_key_requested`
   - LLM decides: Accept or reject key, apply constraints
   - Action: `ssh_agent_add_key_result` (success/failure)
   - Example: LLM learns new keys, applies lifetime constraints

3. **On Sign Request** (SIGN_REQUEST):
   - Event: `ssh_agent_sign_requested`
   - LLM gets: Public key, data to sign, flags
   - LLM decides: Approve or deny, generate signature
   - Action: `ssh_agent_return_signature`
   - Example: LLM validates operation, generates Ed25519 signature

4. **On Lock** (LOCK):
   - Event: `ssh_agent_lock_requested`
   - LLM decides: Accept passphrase, lock keys
   - Action: `ssh_agent_lock_result`

5. **On Unlock** (UNLOCK):
   - Event: `ssh_agent_unlock_requested`
   - LLM decides: Validate passphrase
   - Action: `ssh_agent_unlock_result`

#### **Client-Side**:

1. **Connect**:
   - Event: `ssh_agent_client_connected`
   - LLM decides: Which operations to request
   - Action: `ssh_agent_client_request_identities` or similar

2. **Process Response**:
   - Event: `ssh_agent_response_received`
   - LLM decides: Whether to sign data, add keys, etc.
   - Action: `ssh_agent_client_sign_request`

### 5.4 Message Format Details for LLM Integration

**Important**: Follow NetGet's design principle - structure all action parameters as JSON objects, NOT binary data.

**AVOID** ❌:
```json
{
  "type": "ssh_agent_return_signature",
  "signature": "AQAA...=="  (base64 binary)
}
```

**USE** ✅:
```json
{
  "type": "ssh_agent_return_signature",
  "signature": {
    "algorithm": "ssh-ed25519",
    "data_hex": "3045022100abcd..."  (hex-encoded binary)
  }
}
```

---

## 6. Protocol Implementation Checklist

### Core Features
- [ ] Unix domain socket server listening
- [ ] REQUEST_IDENTITIES operation
- [ ] SIGN_REQUEST operation
- [ ] ADD_IDENTITY/ADD_ID_CONSTRAINED operations
- [ ] REMOVE_IDENTITY operation
- [ ] REMOVE_ALL_IDENTITIES operation
- [ ] LOCK/UNLOCK operations
- [ ] Generic SUCCESS/FAILURE responses

### Advanced Features
- [ ] SMARTCARD key operations
- [ ] Key constraints (lifetime, confirmation)
- [ ] Extension support (query@openssh.com)
- [ ] Session binding (OpenSSH extension)
- [ ] Destination constraints (OpenSSH extension)
- [ ] Windows named pipe support
- [ ] Remote agent forwarding via SSH

### LLM Integration
- [ ] Event generation for all operations
- [ ] Action parsing and execution
- [ ] Key storage via LLM memory
- [ ] Signature generation via LLM
- [ ] Dual logging (trace + status_tx)

### Testing
- [ ] Unit tests for message parsing
- [ ] E2E tests with real SSH clients (ssh, ssh-keygen)
- [ ] Stress testing (concurrent requests)
- [ ] OpenSSH agent compatibility tests

---

## 7. Example Prompts for NetGet SSH Agent

### Basic Agent Server
```
Start an SSH agent on Unix socket $SSH_AUTH_SOCK
Allow any client to list keys
Provide 3 pre-configured keys:
  - my-ed25519 (Ed25519 key)
  - my-rsa (RSA key)
  - temporary-key (expires in 60 seconds)
Allow signing requests and return valid signatures
```

### Agent Honeypot
```
SSH agent that logs all requests
Pretend to have sensitive keys (defense.key, backup.key)
When clients request signatures, log: timestamp, key, data size
Deny any key removal requests
After 5 failed sign attempts on same key, lock the agent
Return signatures that appear valid but are invalid (detection trap)
```

### Agent Client
```
Connect to running SSH agent at $SSH_AUTH_SOCK
Request list of available keys
For each key, request a signature on test data
Log results of signature verification
Close connection
```

---

## 8. Key Data Structures

### Identity (Key) Format

Public key blob in SSH wire format:
```
string key_type              (e.g., "ssh-ed25519")
string type-specific_data    (depends on key type)
  - Ed25519: string (32-byte public key)
  - RSA: mpint (e), mpint (n)
  - ECDSA: string (curve), string (Q)
  - DSA: mpint (p), mpint (q), mpint (g), mpint (y)
```

### Signature Format

SSH wire format signature:
```
string algorithm             (e.g., "ssh-ed25519")
string signature_bytes       (raw signature data)
```

---

## 9. Security Considerations

1. **Private Key Protection**: Never transmit private keys; perform operations server-side
2. **Access Control**: Restrict socket file permissions (0700)
3. **Constraint Enforcement**: Validate lifetime and confirmation constraints
4. **Session Binding**: Verify SSH session identifiers for agent forwarding
5. **Destination Restrictions**: Enforce per-hop constraints on usage
6. **Passphrase Security**: Hash passphrases, never store plaintext
7. **Memory Protection**: Zero sensitive data before freeing
8. **Operation Logging**: Log all signature requests for audit trails

---

## 10. References

1. **IETF SSH Agent Protocol**: https://datatracker.ietf.org/doc/draft-ietf-sshm-ssh-agent/
2. **OpenSSH PROTOCOL.agent**: https://github.com/openssh/openssh-portable/blob/master/PROTOCOL.agent
3. **ssh-agent-lib**: https://github.com/wiktor-k/ssh-agent-lib
4. **RFC 4251**: SSH Protocol Architecture
5. **RFC 4253**: SSH Transport Layer Protocol
6. **OpenSSH Agent Restrictions**: https://www.openssh.org/agent-restrict.html

---

## 11. Crate Comparison Matrix

| Feature | ssh-agent-lib | ssh-agent-client-rs | russh-keys |
|---------|--------------|-------------------|-----------|
| Server impl | ✅ Full | ❌ No | ⚠️ Limited |
| Client impl | ✅ Full | ✅ Full | ⚠️ Limited |
| Async | ✅ Tokio | ❌ Blocking | ✅ Yes |
| Unix socket | ✅ Yes | ✅ Yes | N/A |
| Named pipes | ✅ Yes | ✅ Yes | N/A |
| Constraints | ✅ Yes | ⚠️ Basic | ❌ No |
| Extensions | ✅ Yes | ⚠️ Basic | ❌ No |
| Maintained | ✅ Active | ✅ Active | ✅ Active |

**Recommendation**: Use **ssh-agent-lib** for NetGet SSH Agent Server implementation.

