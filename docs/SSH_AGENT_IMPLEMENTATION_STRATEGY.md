# SSH Agent Protocol Implementation Strategy for NetGet

## Overview

This document outlines the recommended approach for implementing SSH Agent server and client protocols in NetGet, based on comprehensive research of the SSH Agent protocol specification and available Rust libraries.

---

## Executive Recommendations

### 1. Use ssh-agent-lib Crate

**Decision**: Use `ssh-agent-lib` (wiktor-k/ssh-agent-lib) as the foundation for both server and client implementations.

**Rationale**:
- Complete protocol implementation (all message types)
- Production-ready, actively maintained
- Async/await support via Tokio
- Cross-platform (Unix sockets, Windows named pipes)
- Trait-based `Session` interface perfect for LLM integration
- MIT/Apache 2.0 dual licensing
- Well-documented API

**Alternative**: If more control needed over protocol details, implement from scratch using low-level socket APIs, but this is NOT recommended.

### 2. Architecture: Hybrid Approach

**Server** (Stateful LLM Integration):
- Use `ssh-agent-lib` for protocol/socket handling
- Implement custom `Session` trait for LLM integration
- Store keys in-memory with LLM-controlled lifecycle
- Each operation (add/sign/remove) triggers LLM event and awaits action

**Client** (Stateless Querying):
- Use `ssh-agent-lib` client or `ssh-agent-client-rs`
- Connect to agent, request operations
- Simple pass-through to LLM for decision-making

---

## Feature Flag & Integration Points

### Cargo.toml Addition

```toml
# In [features] section:
ssh-agent = ["dep:ssh-agent-lib", "async-trait"]

# In [dependencies] section:
ssh-agent-lib = { version = "0.5", optional = true }

# Update all-protocols:
all-protocols = [
    # ... existing protocols ...
    "ssh-agent",
]
```

### Module Structure

```
src/server/ssh_agent/
├── mod.rs                 # Main server setup
├── actions.rs             # Protocol actions (12 methods)
├── session.rs             # Session trait implementation
└── handlers.rs            # Message handlers (add/sign/lock/etc)

src/client/ssh_agent/
├── mod.rs                 # Client connection
└── actions.rs             # Client actions

protocol/registry.rs       # Add SSH Agent server & client
cli/server_startup.rs      # Add SSH Agent server startup
cli/client_startup.rs      # Add SSH Agent client startup
state/server.rs            # Add SshAgentConnectionInfo variant
```

---

## Implementation Phases

### Phase 1: Foundation & Core (Week 1)

**Goal**: Basic protocol working with hardcoded keys

**Tasks**:
1. Add `ssh-agent-lib` dependency to Cargo.toml
2. Create `src/server/ssh_agent/mod.rs` with basic server setup
3. Implement `Session` trait with hardcoded identities
4. Create Unix socket listener
5. Handle REQUEST_IDENTITIES (0x0B) operation
6. Return mock keys and SUCCESS response
7. Create basic E2E test with real ssh-add client

**Expected Outcome**:
```bash
ssh-add -l  # Lists keys from NetGet agent
```

**Time**: 1-2 days

### Phase 2: Key Operations (Week 1-2)

**Goal**: Full key lifecycle management

**Tasks**:
1. Implement SIGN_REQUEST (0x0D) operation
   - Accept public key blob and data
   - Generate dummy signatures (Ed25519 format)
   - Return via SSH_AGENT_SIGN_RESPONSE
2. Implement ADD_IDENTITY (0x11) operation
   - Parse key data
   - Store in in-memory HashMap
   - Return SUCCESS
3. Implement REMOVE_IDENTITY (0x12)
4. Implement REMOVE_ALL_IDENTITIES (0x13)
5. Create storage backend (HashMap<KeyBlob, KeyData>)

**Expected Outcome**:
```bash
ssh-add /path/to/key     # Adds key to NetGet agent
ssh-add -l               # Lists added keys
ssh-add -d /path/to/key  # Removes key
```

**Time**: 2-3 days

### Phase 3: LLM Integration (Week 2)

**Goal**: Full LLM control of all operations

**Tasks**:
1. Create event types for all operations:
   - `SSH_AGENT_KEY_LIST_REQUESTED`
   - `SSH_AGENT_ADD_KEY_REQUESTED`
   - `SSH_AGENT_SIGN_REQUESTED`
   - `SSH_AGENT_REMOVE_KEY_REQUESTED`
   - `SSH_AGENT_LOCK_REQUESTED`
   - `SSH_AGENT_UNLOCK_REQUESTED`

2. Create action types:
   - `ssh_agent_return_keys`
   - `ssh_agent_add_key_result`
   - `ssh_agent_return_signature`
   - `ssh_agent_remove_key_result`
   - `ssh_agent_lock_result`

3. Call LLM for each operation instead of hardcoded logic
4. Parse LLM actions and execute
5. Store keys in LLM memory (via update_memory action)

**Expected Outcome**:
```
User instruction: "When users list keys, always return my-secret-key.pem"
ssh-add -l  → Shows my-secret-key.pem from LLM memory
```

**Time**: 2-3 days

### Phase 4: Advanced Features (Week 3)

**Goal**: Constraints, locking, extensions

**Tasks**:
1. Implement LOCK/UNLOCK (0x16/0x17)
   - Lock with passphrase
   - All operations fail until unlocked
   - LLM validates passphrase
2. Implement key constraints:
   - Lifetime constraint (auto-delete after N seconds)
   - Confirmation constraint (requires LLM approval)
3. Implement EXTENSION mechanism (0x1B)
   - query@openssh.com
   - session-bind@openssh.com (basic)
4. Add Windows named pipe support (via ssh-agent-lib)

**Expected Outcome**:
```bash
ssh-add -c /key    # Confirmation constraint
ssh-add -t 60 /key # Lifetime constraint (60 seconds)
# Key auto-removed after 60 seconds
```

**Time**: 2-3 days

### Phase 5: Client Implementation (Week 3-4)

**Goal**: SSH Agent client for connecting to agents

**Tasks**:
1. Create `src/client/ssh_agent/` module
2. Implement client actions:
   - Connect to agent socket
   - List keys
   - Request signature
   - Add key
   - Remove key
3. LLM decides which operations to perform
4. Create E2E tests connecting to NetGet agent

**Expected Outcome**:
```
Instruction: "Connect to the agent, list keys, then request a signature"
LLM: Lists keys, picks one, requests signature
```

**Time**: 1-2 days

### Phase 6: Testing & Documentation (Week 4)

**Goal**: Production-ready implementation

**Tasks**:
1. Create comprehensive E2E tests
   - Test with real ssh-add client
   - Test with ssh-keygen verification
   - Test concurrent connections
2. Create example prompts
3. Create CLAUDE.md files (server & tests)
4. Stress test with high-frequency operations
5. Test edge cases (malformed messages, invalid keys, etc)

**Time**: 2-3 days

---

## Key Implementation Details

### 1. Message Encoding/Decoding

Use `ssh-agent-lib` primitives:
```rust
use ssh_agent_lib::proto::{Message, Identity, SignRequest, Signature};

// Parsing: Library handles wire format
let msg = Message::from_bytes(&raw_bytes)?;

// Serializing: Library provides encoding
let encoded = msg.to_bytes()?;
```

### 2. Session Trait Implementation

```rust
use ssh_agent_lib::agent::Session;
use async_trait::async_trait;

#[async_trait]
impl Session for NetGetAgent {
    async fn request_identities(&self) -> Result<Vec<Identity>, AgentError> {
        // Call LLM with SSH_AGENT_KEY_LIST_REQUESTED event
        // LLM returns: ssh_agent_return_keys action with key list
        // Return identities to client
    }
    
    async fn sign(&self, request: SignRequest) -> Result<Signature, AgentError> {
        // Call LLM with SSH_AGENT_SIGN_REQUESTED event
        // Include: public_key, data_to_sign, flags
        // LLM returns: ssh_agent_return_signature with signature blob
        // Return signature to client
    }
}
```

### 3. Key Storage

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct StoredKey {
    key_type: String,
    public_key: Vec<u8>,
    private_key: Vec<u8>,
    comment: String,
    constraints: Vec<Constraint>,
    added_at: SystemTime,
}

// In-memory storage
let mut keys: HashMap<Vec<u8>, StoredKey> = HashMap::new();
```

### 4. LLM Integration Pattern

```rust
// On REQUEST_IDENTITIES
let event = Event::new(&SSH_AGENT_KEY_LIST_REQUESTED, json!({
    "current_keys": self.keys.len(),
}));

let result = call_llm(event, ...).await?;

// Parse action
match result.actions[0]["type"].as_str()? {
    "ssh_agent_return_keys" => {
        let keys = parse_keys_from_action(&result.actions[0])?;
        Ok(keys)
    }
    _ => Err("Unexpected action".into())
}
```

### 5. Socket Management

```rust
// Unix socket creation (via ssh-agent-lib)
use ssh_agent_lib::agent::listen;

let server = MyAgent::new(llm_client, app_state);
let socket_path = "./tmp/ssh-RANDOM/agent.PID";

// Listen on Unix socket
listen(server, socket_path).await?;

// Environment variable
std::env::set_var("SSH_AUTH_SOCK", socket_path);
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_request_identities() {
        let agent = MockAgent::new();
        let identities = agent.request_identities().await.unwrap();
        assert!(!identities.is_empty());
    }
    
    #[tokio::test]
    async fn test_sign_request() {
        let agent = MockAgent::with_key(test_key);
        let sig = agent.sign(test_request).await.unwrap();
        assert!(!sig.blob.is_empty());
    }
}
```

### E2E Tests

```rust
#[tokio::test]
async fn test_with_real_ssh_add() {
    // Start NetGet SSH agent on test socket
    let socket_path = temp_socket_path();
    spawn_agent(&socket_path).await;
    
    // Run ssh-add client
    let output = Command::new("ssh-add")
        .env("SSH_AUTH_SOCK", &socket_path)
        .arg("-l")
        .output()
        .expect("ssh-add failed");
    
    // Verify output contains key
    assert!(String::from_utf8_lossy(&output.stdout).contains("ssh-ed25519"));
}
```

### Compatibility Tests

```bash
# Test with OpenSSH utilities
export SSH_AUTH_SOCK=./tmp/netget-agent.sock
ssh-keygen -c -C "new comment" -f test_key  # Requires agent
ssh-add test_key
ssh-add -l
ssh-add -D
```

---

## Error Handling Strategy

### Protocol Errors
- Invalid message format → SSH_AGENT_FAILURE
- Unknown message type → SSH_AGENT_FAILURE
- Constraint violation → SSH_AGENT_FAILURE

### LLM Errors
- LLM timeout → SSH_AGENT_FAILURE (with logging)
- Unparseable action → SSH_AGENT_FAILURE
- Missing required action → SSH_AGENT_FAILURE

### State Errors
- Key not found → SSH_AGENT_FAILURE
- Agent locked → SSH_AGENT_FAILURE
- Constraint expired → SSH_AGENT_FAILURE

---

## Performance Characteristics

| Operation | Latency | Dependencies |
|-----------|---------|--------------|
| REQUEST_IDENTITIES | 50-100ms | 1 LLM call |
| SIGN_REQUEST | 50-100ms | 1 LLM call |
| ADD_IDENTITY | 50-100ms | 1 LLM call |
| REMOVE_IDENTITY | 50-100ms | 1 LLM call |

**Optimization**: Use scripting mode in LLM for predictable operations.

---

## Known Limitations & Workarounds

| Limitation | Workaround |
|------------|-----------|
| Signatures not crypto-valid | For testing/honeypot only, not for real SSH |
| Keys exist only in memory | Use LLM memory to persist across LLM calls |
| No hardware token support | Smartcard operations return dummy data |
| No agent forwarding encryption | Single-machine testing only |

---

## Example Prompts

### Basic Agent
```
Start SSH agent listening on Unix socket
Allow clients to list keys
Provide two keys: my-ed25519 and my-rsa
When clients request signatures, return valid-looking signatures
```

### Agent Honeypot
```
SSH agent that logs all authentication attempts
Pretend to have valuable keys: admin.pem, database.pem, backup.pem
Log: timestamp, client PID, requested key, data size
After 10 failed signature attempts, lock the agent
Return signatures that look valid but are invalid
```

### Agent with Time-Limited Keys
```
Agent that provides time-limited SSH keys
When user adds a key, add 10-minute lifetime constraint
Auto-remove expired keys
Log when keys are added and when they expire
```

---

## Success Criteria

1. ✅ Message handling: All 12 message types work correctly
2. ✅ Protocol compliance: Compatible with ssh-add, ssh-keygen
3. ✅ LLM integration: All operations controllable by LLM
4. ✅ Key storage: Keys persist during session (via LLM memory)
5. ✅ Testing: E2E tests with real OpenSSH tools pass
6. ✅ Documentation: CLAUDE.md files complete
7. ✅ Performance: < 200ms latency per operation
8. ✅ Concurrency: Multiple simultaneous client connections work

---

## References

1. ssh-agent-lib: https://github.com/wiktor-k/ssh-agent-lib
2. IETF SSH Agent Protocol: https://datatracker.ietf.org/doc/draft-ietf-sshm-ssh-agent/
3. OpenSSH PROTOCOL.agent: https://github.com/openssh/openssh-portable/blob/master/PROTOCOL.agent
4. Main research doc: `/docs/SSH_AGENT_PROTOCOL_RESEARCH.md`
5. Quick reference: `/docs/SSH_AGENT_QUICK_REFERENCE.md`

