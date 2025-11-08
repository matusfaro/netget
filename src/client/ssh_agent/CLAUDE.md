# SSH Agent Client Implementation

## Overview

SSH Agent client connects to an existing SSH Agent (OpenSSH agent, NetGet agent, or other compatible agents) and allows the LLM to interact with it - requesting identities, signing data, adding/removing keys.

## Protocol Specification

- **Standard**: IETF draft-ietf-sshm-ssh-agent-05
- **Transport**: Unix domain socket (Unix/Linux/macOS)
- **Wire Format**: SSH protocol binary format (uint32 length prefix + message type + data)
- **Stack**: Application layer

## Library Choices

### Custom Implementation

Uses a custom SSH Agent protocol implementation for:
- Full control over message construction
- Direct integration with NetGet's LLM action system
- Simple, lightweight client without complex dependencies

### Dependencies

- `tokio::net::UnixStream` - Unix domain socket client
- `bytes` - Efficient byte buffer manipulation
- `hex` - Encoding/decoding for LLM-friendly hex strings

## Architecture

### Connection Model

```
Connect to Unix Socket ($SSH_AUTH_SOCK or specified path)
    ↓
Split Stream (read/write halves)
    ↓
Send Initial Event (connected) → LLM → Execute Actions → Send Requests
    ↓
Reader Task ──→ Parse Response ──→ Call LLM ──→ Execute Actions → Send More Requests
```

### State Machine

Same as server: Idle → Processing → Accumulating

Prevents concurrent LLM calls while processing responses.

### Supported Operations

| Operation | Code | Direction | LLM Action | Response |
|-----------|------|-----------|------------|----------|
| REQUEST_IDENTITIES | 11 | Client→Agent | `request_identities` | IDENTITIES_ANSWER (12) |
| SIGN_REQUEST | 13 | Client→Agent | `sign_request` | SIGN_RESPONSE (14) |
| ADD_IDENTITY | 17 | Client→Agent | `add_identity` | SUCCESS (6) / FAILURE (5) |
| REMOVE_IDENTITY | 18 | Client→Agent | `remove_identity` | SUCCESS / FAILURE |
| REMOVE_ALL | 19 | Client→Agent | `remove_all_identities` | SUCCESS / FAILURE |

## LLM Integration

### Control Points

1. **Connected** (`ssh_agent_client_connected`)
   - Triggered when client connects to agent
   - LLM can immediately request identities or perform operations
   - Parameters: `socket_path`

2. **Response Received** (`ssh_agent_client_response_received`)
   - Triggered when agent responds to a request
   - LLM interprets response and decides next action
   - Parameters:
     - `response_type`: "identities", "signature", "success", "failure"
     - `response_data`: Structured response data

### Action Design (Structured Data)

**Request Identities**:
```json
{
  "type": "request_identities"
}
```

**Sign Request**:
```json
{
  "type": "sign_request",
  "public_key_blob_hex": "0000000b7373682d656432353531390000002104...",
  "data_hex": "48656c6c6f",
  "flags": 0
}
```

**Add Identity**:
```json
{
  "type": "add_identity",
  "key_type": "ssh-ed25519",
  "public_key_blob_hex": "...",
  "private_key_blob_hex": "...",
  "comment": "my-key-2025"
}
```

**Response Events**:

Identities Response:
```json
{
  "response_type": "identities",
  "response_data": {
    "count": 2,
    "identities": [
      {
        "public_key_blob_hex": "...",
        "comment": "key1"
      }
    ]
  }
}
```

Signature Response:
```json
{
  "response_type": "signature",
  "response_data": {
    "signature_hex": "0000000b7373682d65643235353139000000400a1b2c..."
  }
}
```

### Memory Usage

The LLM can use memory to:
- Track requested identities
- Store signature results
- Maintain state across multiple operations
- Implement multi-step workflows

## Logging Strategy

**Dual Logging**:
- `trace!`, `info!`, `error!` macros → `netget.log`
- `status_tx.send()` → TUI display

**Log Levels**:
- ERROR: Connection errors, parse failures
- INFO: Connection lifecycle, operations
- TRACE: Request/response details, hex data

## Limitations

1. **Platform**: Unix/Linux/macOS only (Unix domain sockets)
2. **Socket Path**: Requires valid socket path or $SSH_AUTH_SOCK
3. **Response Parsing**: Basic parsing, may not handle all edge cases
4. **Queued Data**: Simplified queuing (discards queued responses)
5. **Windows**: Not supported (would need named pipe implementation)
6. **Extensions**: OpenSSH-specific extensions not implemented

## Example Prompts

### List Keys from Agent

```
Connect to SSH Agent at $SSH_AUTH_SOCK
Request list of available identities
Display the identities with their comments
```

### Sign Data with Agent

```
Connect to SSH Agent
Request identities
Use the first Ed25519 key to sign "Hello, World!"
Display the signature
```

### Add Key to Agent

```
Connect to SSH Agent
Generate an Ed25519 key pair
Add it to the agent with comment "netget-test-2025"
Verify it was added by listing identities
```

### Automated Key Management

```
Connect to SSH Agent
List all identities
Remove any keys older than 24 hours (based on comment timestamps)
Add a new temporary key
```

## Integration Points

- `protocol/client_registry.rs`: Register SshAgentClientProtocol
- `cli/client_startup.rs`: Add SSH Agent client startup case
- `src/client/mod.rs`: Re-export SshAgentClientProtocol

## Testing

See `tests/client/ssh_agent/CLAUDE.md` for testing strategy.

## References

- IETF SSH Agent Protocol: draft-ietf-sshm-ssh-agent-05
- OpenSSH Agent: https://github.com/openssh/openssh-portable/blob/master/ssh-agent.c
- NetGet docs: `/docs/SSH_AGENT_PROTOCOL_RESEARCH.md`
