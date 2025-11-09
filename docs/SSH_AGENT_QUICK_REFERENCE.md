# SSH Agent Protocol - Quick Reference Guide

## Message Types at a Glance

### Request Messages (Client → Agent)
```
11 (0x0B) - REQUEST_IDENTITIES         [no data]
13 (0x0D) - SIGN_REQUEST               [key_blob][data][flags]
17 (0x11) - ADD_IDENTITY               [key_type][pub][priv][comment]
18 (0x12) - REMOVE_IDENTITY            [key_blob]
19 (0x13) - REMOVE_ALL_IDENTITIES      [no data]
20 (0x14) - ADD_SMARTCARD_KEY          [reader][pin]
21 (0x15) - REMOVE_SMARTCARD_KEY       [reader][pin]
22 (0x16) - LOCK                       [passphrase]
23 (0x17) - UNLOCK                     [passphrase]
25 (0x19) - ADD_ID_CONSTRAINED         [key_type][pub][priv][comment][constraints]
26 (0x1A) - ADD_SMARTCARD_KEY_CONSTRAINED [reader][pin][constraints]
27 (0x1B) - EXTENSION                  [extension_name][extension_data]
```

### Response Messages (Agent → Client)
```
5  (0x05) - FAILURE                    [no data]
6  (0x06) - SUCCESS                    [no data]
12 (0x0C) - IDENTITIES_ANSWER          [nkeys][key_blob][comment]...
14 (0x0E) - SIGN_RESPONSE              [signature_blob]
28 (0x1C) - EXTENSION_FAILURE          [no data]
29 (0x1D) - EXTENSION_RESPONSE         [extension_data]
```

## Wire Format Template

All messages follow this structure:
```
[uint32: total_length][byte: type][variable: data]
```

Example:
```
REQUEST_IDENTITIES:
  00 00 00 01    (length = 1)
  0B             (type = 11)

IDENTITIES_ANSWER (2 keys):
  00 00 00 50    (length = 80)
  0C             (type = 12)
  00 00 00 02    (nkeys = 2)
  00 00 00 0B    (key1 blob length = 11)
  [11 bytes of key blob]
  00 00 00 08    (comment1 length = 8)
  [8 bytes of comment]
  ... (key2 data)
```

## Transport Details

| Aspect | Details |
|--------|---------|
| **Unix Socket** | `/tmp/ssh-XXXXXXXXXX/agent.YYYY` (mode 0700) |
| **Environment** | `$SSH_AUTH_SOCK` points to socket |
| **Windows** | `\\.\pipe\openssh-ssh-agent` (named pipe) |
| **Connection** | Persistent, stateful, TCP-like |
| **Request Mode** | Synchronous (one at a time) |
| **Message Framing** | Length-prefixed (no delimiters) |

## Key Constraints

```
Type 1 - LIFETIME:
  byte type = 1
  uint32 seconds    (key expires after N seconds)

Type 2 - CONFIRM:
  byte type = 2     (require user confirmation for each operation)

Type 255 (0xFF) - EXTENSION:
  byte type = 255
  string extension_name  (e.g., "session-bind@openssh.com")
  [extension-specific data]
```

## OpenSSH Extensions

### session-bind@openssh.com
```
Binds agent connection to SSH session for agent forwarding
Fields: hostkey, session_id, signature, is_forwarding
```

### restrict-destination-v00@openssh.com
```
Restricts key usage to specific authentication chains
Fields: from_hostname, to_hostname, from_hostkeys, to_hostkeys
```

### associated-certs-v00@openssh.com
```
Associates certificates with smartcard keys
Fields: certs_only, certsblob
```

## SSH Key Types Supported

| Type | String | Format |
|------|--------|--------|
| Ed25519 | `ssh-ed25519` | 32 bytes |
| RSA | `ssh-rsa` | e (exponent) + n (modulus) |
| ECDSA-256 | `ecdsa-sha2-nistp256` | curve + Q |
| ECDSA-384 | `ecdsa-sha2-nistp384` | curve + Q |
| ECDSA-521 | `ecdsa-sha2-nistp521` | curve + Q |
| DSA | `ssh-dss` | p, q, g, y |

## Recommended Rust Crates

| Crate | Purpose | Best For |
|-------|---------|----------|
| `ssh-agent-lib` | Full server + client | Server implementation |
| `ssh-agent-client-rs` | Client only | Connecting to agents |
| `ssh_key` | Key types & encoding | Key handling |
| `tokio` | Async runtime | Concurrent handling |

## Implementation Checklist (Priority Order)

### Phase 1: Core Protocol
- [x] Message type enums (11, 13, 5, 6, 12, 14)
- [x] Wire format encoding/decoding
- [x] Unix socket listener
- [ ] REQUEST_IDENTITIES handler
- [ ] SIGN_REQUEST handler

### Phase 2: Key Operations
- [ ] ADD_IDENTITY handler
- [ ] REMOVE_IDENTITY handler
- [ ] REMOVE_ALL_IDENTITIES handler
- [ ] Key storage (in-memory, LLM-controlled)

### Phase 3: Advanced Features
- [ ] LOCK/UNLOCK with passphrase
- [ ] Key constraints (lifetime, confirm)
- [ ] SMARTCARD operations
- [ ] EXTENSION mechanism

### Phase 4: LLM Integration
- [ ] Event generation for all operations
- [ ] Action parsing and execution
- [ ] Signature generation via LLM
- [ ] Memory-based key persistence

### Phase 5: Testing & Hardening
- [ ] E2E tests with ssh-keygen
- [ ] E2E tests with ssh-agent clients
- [ ] Concurrent request handling
- [ ] OpenSSH agent compatibility

## Common Pitfalls

| Problem | Solution |
|---------|----------|
| Private key exposed | Never send privkey to client; sign server-side |
| Message framing issues | Use length-prefixed encoding, not delimiters |
| Race conditions | One request at a time per connection (synchronous) |
| Socket permissions | Unix sockets must be mode 0700 |
| Key format mismatch | Use `ssh_key` crate for encoding/decoding |
| Missing constraint checks | Validate lifetime, confirm, destination on each op |
| Agent lock bypass | Lock must fail ALL operations until unlocked |

## Testing with Real OpenSSH

```bash
# Generate test key
ssh-keygen -t ed25519 -f test_key -N ""

# Start NetGet SSH agent on custom socket
export SSH_AUTH_SOCK="./netget-agent.sock"
# (start netget agent)

# Add key to agent
ssh-add test_key

# List keys
ssh-add -l

# Request signature (used by ssh internally)
ssh-keyscan localhost  # Uses agent internally

# Check agent socket permissions
ls -la $SSH_AUTH_SOCK  # Should be mode 700
```

## Debugging Tips

1. **Check Socket Exists**:
   ```bash
   ls -la /tmp/ssh-*/agent.* 2>/dev/null || echo "No agent socket"
   ```

2. **Monitor Messages**:
   ```bash
   # Use strace to see protocol traffic
   strace -e trace=write,read ssh-add -l
   ```

3. **Test Basic Connectivity**:
   ```rust
   // Try to connect via ssh-agent-lib
   let agent = Client::connect()?;
   let ids = agent.request_identities()?;
   println!("Found {} keys", ids.len());
   ```

4. **Check Environment**:
   ```bash
   echo "Socket: $SSH_AUTH_SOCK"
   echo "Agent pid: $(pgrep -f 'ssh-agent|netget')"
   ```

## Performance Notes

- **Latency**: ~100ms per operation (LLM dependent)
- **Throughput**: Sequential (one request at a time)
- **Memory**: Keys stored in-memory, proportional to key count
- **Scalability**: Limited by LLM response time, not protocol
- **Concurrency**: Multiple client connections supported (separate per-client queues)

## Security Considerations

1. ✅ Private keys never exposed to clients
2. ✅ Constraints enforced before operations
3. ✅ Passphrase-protected locks (LLM-validated)
4. ✅ Destination restrictions for agent forwarding
5. ✅ Operation logging for audit trails
6. ❌ **Not for production**: Virtual keys (can't verify signing)
7. ❌ **Test honeypots only**: Signatures not cryptographically valid

## References

- IETF Draft: https://datatracker.ietf.org/doc/draft-ietf-sshm-ssh-agent/
- OpenSSH Code: https://github.com/openssh/openssh-portable/blob/master/ssh-agent.c
- ssh-agent-lib: https://github.com/wiktor-k/ssh-agent-lib
- RFC 4251: SSH Protocol Architecture
