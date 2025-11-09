# SSH Agent Server Implementation

## Overview

SSH Agent is a key management protocol that stores SSH private keys and performs signing operations without exposing the keys. This implementation provides an LLM-controlled SSH Agent server that responds to client requests (ssh-add, ssh, etc.).

## Protocol Specification

- **Standard**: IETF draft-ietf-sshm-ssh-agent-05
- **Transport**: Unix domain socket (Unix/Linux/macOS)
- **Wire Format**: SSH protocol binary format (uint32 length prefix + message type + data)
- **Stack**: Application layer

## Library Choices

### Custom Implementation

This implementation uses a **custom SSH Agent protocol parser** instead of `ssh-agent-lib` for the following reasons:

1. **Full LLM Control**: Direct access to all protocol messages for LLM integration
2. **Simplicity**: No need for complex trait implementations
3. **Flexibility**: Easy to customize behavior for NetGet's architecture
4. **Dependencies**: Minimal dependencies (only bytes, hex, tokio)

The protocol parser handles:
- Message type detection (11 types)
- SSH wire format parsing (uint32 length + string/blob encoding)
- Response generation with proper wire format

### Dependencies

- `tokio::net::UnixListener` - Unix domain socket server
- `bytes` - Efficient byte buffer manipulation
- `hex` - Encoding/decoding for LLM-friendly hex strings

## Architecture

### Connection Model

```
Unix Socket Listener
    ↓
Accept Connection
    ↓
Split Stream (read/write halves)
    ↓
Reader Task ──→ Parse Message ──→ Call LLM ──→ Execute Actions ──→ Write Response
```

### State Machine

Each connection maintains a state machine to prevent concurrent LLM calls:

- **Idle**: Ready to process new data
- **Processing**: LLM call in progress, queue incoming data
- **Accumulating**: Queuing data while processing

### Message Types Supported

| Type | Code | Operation | LLM Event |
|------|------|-----------|-----------|
| REQUEST_IDENTITIES | 11 | List keys | `ssh_agent_request_identities` |
| SIGN_REQUEST | 13 | Sign data | `ssh_agent_sign_request` |
| ADD_IDENTITY | 17 | Add key | `ssh_agent_add_identity` |
| REMOVE_IDENTITY | 18 | Remove key | `ssh_agent_remove_identity` |
| REMOVE_ALL | 19 | Clear all keys | `ssh_agent_remove_all_identities` |
| LOCK | 22 | Lock agent | `ssh_agent_lock` |
| UNLOCK | 23 | Unlock agent | `ssh_agent_unlock` |
| ADD_ID_CONSTRAINED | 25 | Add with constraints | `ssh_agent_add_identity` |

**Response Types**:
- SSH_AGENT_FAILURE (5)
- SSH_AGENT_SUCCESS (6)
- SSH_AGENT_IDENTITIES_ANSWER (12)
- SSH_AGENT_SIGN_RESPONSE (14)

## LLM Integration

### Control Points

1. **Connection Opened** (`ssh_agent_connection_opened`)
   - Triggered when client connects
   - LLM can initialize state, prepare keys

2. **Request Identities** (`ssh_agent_request_identities`)
   - Client wants list of available keys
   - LLM returns structured identities (key type, blob, comment)

3. **Sign Request** (`ssh_agent_sign_requested`)
   - Client wants to sign data with a specific key
   - LLM receives: public key blob (hex), data to sign (hex), flags
   - LLM returns: signature blob (hex)

4. **Add Identity** (`ssh_agent_add_identity`)
   - Client wants to add a key to the agent
   - LLM receives: key type, public key blob (hex), comment, constrained flag
   - LLM can store in memory, apply lifetime constraints

5. **Remove/Lock/Unlock** operations
   - LLM controls key lifecycle and access control

### Action Design (Structured Data)

**CRITICAL**: All action parameters use structured JSON, NOT binary/base64:

**Identities List** (CORRECT ✅):
```json
{
  "type": "send_identities_list",
  "identities": [
    {
      "key_type": "ssh-ed25519",
      "public_key_blob_hex": "0000000b7373682d656432353531390000002104...",
      "comment": "my-key-2025"
    }
  ]
}
```

**Sign Response** (CORRECT ✅):
```json
{
  "type": "send_sign_response",
  "signature_hex": "0000000b7373682d65643235353139000000400a1b2c3d..."
}
```

**AVOID** (INCORRECT ❌):
```json
{
  "type": "send_sign_response",
  "signature": "AQAA..."  // base64 binary - LLM can't construct this
}
```

### Memory Usage

The LLM can use memory to:
- Store added keys (key type, blob, comment)
- Track signing operations
- Implement access control (lock/unlock state)
- Apply key constraints (lifetime, confirmation)

## Logging Strategy

**Dual Logging** (CRITICAL):
- `trace!`, `debug!`, `info!`, `error!` macros → `netget.log`
- `status_tx.send()` → TUI display

**Log Levels**:
- ERROR: Failed to parse message, write errors
- INFO: Connection lifecycle (opened, closed)
- DEBUG: Message type received, action execution
- TRACE: Raw hex data, detailed protocol parsing

## Limitations

1. **Platform**: Unix/Linux/macOS only (Unix domain sockets)
2. **Wire Format**: Current implementation assumes message type byte at position 0 (may need full length-prefixed parsing)
3. **Key Storage**: Virtual only (not persistent, stored in LLM memory)
4. **Smartcard**: Not implemented
5. **Extensions**: OpenSSH extensions (session-bind, restrict-destination) not supported
6. **Constraints**: Lifetime/confirmation constraints parsed but not enforced
7. **Windows**: Named pipes not supported (would need separate implementation)

## Security Considerations

1. **Socket Permissions**: Unix socket should have proper permissions (0600)
2. **Key Exposure**: Private keys never transmitted (only sign operations)
3. **LLM Memory**: Keys stored in LLM memory - consider security implications
4. **Local Only**: Agent listens only on local Unix socket (no network exposure)

## Example Prompts

### Basic Agent

```
Start SSH Agent server on ./netget-ssh-agent.sock
Provide 2 pre-configured Ed25519 keys:
  - admin-key (full access)
  - deploy-key (read-only)
Sign any requests automatically
```

### Secure Agent

```
Start SSH Agent server
Require approval before signing (respond with send_failure first, then send_sign_response on confirmation)
Store added keys in memory
Lock agent after 5 minutes of inactivity
```

### Learning Agent

```
Start SSH Agent server
Learn keys dynamically as clients add them
Track signing operations and data signed
Provide statistics on key usage
```

## Integration Points

- `protocol/registry.rs`: Register SshAgentProtocol
- `cli/server_startup.rs`: Add SSH Agent startup case
- `state/server.rs`: Add SshAgentConnectionInfo variant
- `src/server/mod.rs`: Re-export SshAgentProtocol

## Testing

See `tests/server/ssh_agent/CLAUDE.md` for testing strategy.

## References

- IETF SSH Agent Protocol: draft-ietf-sshm-ssh-agent-05
- OpenSSH Agent: https://github.com/openssh/openssh-portable/blob/master/authfd.h
- SSH Protocol RFC 4251: Binary Packet Protocol
- NetGet docs: `/docs/SSH_AGENT_PROTOCOL_RESEARCH.md`
