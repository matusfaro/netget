# SSH Client Implementation

## Overview

SSH client implementation using the `russh` library for connecting to SSH servers and executing commands under LLM
control.

## Library Choices

### Primary Library: russh

**Crate:** `russh` v0.44+
**Why:** Pure Rust SSH implementation with async support and good channel management.

**Key Features:**

- Full SSH protocol implementation (SSH-2)
- Password and public key authentication
- Channel multiplexing
- Command execution via channels
- Active maintenance and good documentation

**Alternatives Considered:**

- `ssh2` - Bindings to libssh2 (C library), less idiomatic Rust
- `thrussh` - Older name for russh (renamed)

### Key Library: russh-keys

**Crate:** `russh-keys` v0.44+
**Why:** Key management and cryptography for russh.

**Features:**

- Public key parsing and validation
- Server key verification
- Key fingerprinting

## Architecture

### Connection Model

**Connection Lifecycle:**

1. Resolve hostname and connect to SSH server
2. Perform SSH handshake and version exchange
3. Authenticate (currently password-only)
4. Trigger `ssh_connected` event to LLM
5. Wait for LLM to issue commands
6. Execute commands via SSH channels
7. Return output to LLM via `ssh_output_received` event
8. LLM can chain commands or disconnect

**Channel Management:**

- Each command execution opens a new SSH channel
- Channels are closed after command completion
- No persistent shell session (command mode only)

### State Machine

**Client State:** Idle → Processing → Accumulating (standard pattern)

**Note:** For SSH, the state machine is simplified because:

- Commands are discrete (one command = one channel = one LLM call)
- No streaming data accumulation like TCP
- Each command waits for completion before next

### Authentication

**Current Implementation:**

- Password authentication only
- Server key verification disabled (accepts all keys for testing)

**Future Enhancements:**

- Public key authentication
- SSH agent support
- Known hosts verification
- Interactive keyboard authentication

## LLM Integration

### Event Flow

1. **Connection Event:** `ssh_connected`
    - Triggered after successful authentication
    - Provides remote_addr and username
    - LLM can issue initial commands

2. **Output Event:** `ssh_output_received`
    - Triggered after command execution completes
    - Provides command output (stdout) and exit code
    - LLM analyzes output and decides next action

### Actions

**Async Actions (User-Triggered):**

- `execute_command` - Run a shell command
- `disconnect` - Close SSH connection

**Sync Actions (Response to Output):**

- `execute_command` - Run follow-up command
- `wait_for_more` - (no-op for SSH, included for consistency)

### Action Examples

```json
{
  "type": "execute_command",
  "command": "ls -la /home"
}

{
  "type": "execute_command",
  "command": "cat /etc/hostname"
}

{
  "type": "disconnect"
}
```

## Data Flow

```
User Instruction → Connect to SSH server → Authenticate
                                               ↓
                                    ssh_connected event
                                               ↓
                                    LLM receives event
                                               ↓
                                    LLM returns action: execute_command
                                               ↓
                                    Open channel, execute command
                                               ↓
                                    Read output, close channel
                                               ↓
                                    ssh_output_received event
                                               ↓
                                    LLM analyzes output
                                               ↓
                            LLM decides: execute_command (next) or disconnect
```

## Logging

**Dual Logging Pattern:**

- All logs via tracing macros (`info!`, `debug!`, `trace!`, `error!`)
- Important events via `status_tx.send()` for TUI

**Log Levels:**

- `INFO` - Connection, authentication, command execution start
- `DEBUG` - Channel operations, exit codes
- `TRACE` - Command output, data transfer details
- `ERROR` - Authentication failures, connection errors

## Limitations

### Current Limitations

1. **Authentication:**
    - Password only (no pubkey yet)
    - No SSH agent support
    - Server key verification disabled

2. **Command Execution:**
    - Command mode only (no interactive shell)
    - No pseudo-terminal (PTY) allocation
    - No stdin streaming to running commands

3. **File Transfer:**
    - No SFTP support yet
    - No SCP support yet

4. **Advanced Features:**
    - No port forwarding
    - No X11 forwarding
    - No SSH tunneling

### Security Considerations

**⚠️ IMPORTANT:** This implementation is for testing and development only.

**Security Issues:**

- Accepts all server keys (MITM risk)
- Password authentication (less secure than pubkey)
- No host key verification

**For Production Use:**

- Implement proper host key verification (`~/.ssh/known_hosts`)
- Prefer public key authentication
- Add certificate validation
- Enable SSH agent support

## Performance

**Resource Usage:**

- Lightweight (russh is pure Rust)
- One channel per command (channels are cheap)
- No persistent processes

**Latency:**

- SSH handshake: ~100-500ms
- Authentication: ~50-200ms
- Command execution: depends on command
- LLM call: ~500-2000ms (dominant factor)

## Error Handling

**Connection Errors:**

- DNS resolution failures
- Network timeouts
- SSH handshake failures

**Authentication Errors:**

- Invalid credentials
- Unsupported auth methods
- Server rejection

**Command Errors:**

- Channel open failure
- Command execution timeout
- Non-zero exit codes (reported to LLM)

## Future Enhancements

### Phase 1 (Current)

- ✅ Password authentication
- ✅ Command execution
- ✅ Output capture

### Phase 2 (Next)

- [ ] Public key authentication
- [ ] Host key verification
- [ ] PTY allocation for interactive commands

### Phase 3 (Advanced)

- [ ] SFTP file transfer
- [ ] SCP file transfer
- [ ] Port forwarding
- [ ] Interactive shell session

### Phase 4 (Expert)

- [ ] SSH agent integration
- [ ] Certificate authentication
- [ ] Jump host (ProxyJump) support

## Testing Strategy

See `tests/client/ssh/CLAUDE.md` for detailed testing approach.

**Test Server Options:**

- OpenSSH server (most common)
- Dropbear (lightweight)
- Local SSH server on localhost:22

## Example Prompts

```
"Connect to SSH at localhost:22 with user 'admin' and password 'test', then execute 'uname -a'"

"SSH to 192.168.1.100:22 as root, check disk usage with 'df -h', then list processes"

"Connect via SSH to server.example.com as deploy user, execute 'git pull' in /var/www/app"
```

## References

- [russh documentation](https://docs.rs/russh/)
- [russh-keys documentation](https://docs.rs/russh-keys/)
- [SSH Protocol RFC 4253](https://datatracker.ietf.org/doc/html/rfc4253)
- [OpenSSH manual](https://www.openssh.com/manual.html)
