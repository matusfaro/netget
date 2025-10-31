# SSH Protocol Implementation

## Overview
SSH server implementing RFC 4253 (SSH Transport Layer Protocol) using the russh library. Provides secure remote access via shell sessions and secure file transfer via SFTP subsystem. The LLM controls authentication decisions, shell command responses, and SFTP filesystem operations.

**Status**: Beta (Application Protocol)
**RFC**: RFC 4253, RFC 4254, RFC 4256 (SSH Protocol Suite)
**Port**: 22 (default)

## Library Choices

### Core SSH Implementation
- **russh v0.40** - Pure Rust SSH server implementation
  - Handles SSH protocol state machine (version exchange, key exchange, encryption)
  - Provides async/await API for connection handling
  - Supports multiple authentication methods (password, publickey)
  - Channel management for shell and subsystem requests

- **russh-keys** - SSH key generation and management
  - Ed25519 host key generation (used for server identity)
  - Automatic key pair creation on server startup

- **russh-sftp v2.0** - SFTP subsystem implementation
  - Parses SFTP protocol packets (SSH_FXP_* messages)
  - Provides trait-based API for filesystem operations
  - Integrates with russh channel API

**Rationale**: russh is the most mature pure-Rust SSH library with full async support. It handles all SSH protocol complexity (encryption, key exchange, packet framing) while exposing high-level APIs for authentication and shell/SFTP handling. This allows the LLM to focus on business logic (auth decisions, shell responses, file contents) rather than protocol details.

## Architecture Decisions

### 1. LLM Control Points
The LLM has three integration points:

**Authentication**:
- LLM receives `ssh_auth` event with username and auth type (password/publickey)
- Returns `ssh_auth_decision` action with `allowed: true/false`
- Supports both password and public key authentication

**Shell Sessions**:
- LLM receives `ssh_banner` event when shell is opened
- LLM receives `ssh_shell_command` event for each command line
- Returns `ssh_shell_response` action with output text and optional `close_connection`

**SFTP Subsystem**:
- LLM receives SFTP operation events (read, write, readdir, stat, etc.)
- Returns filesystem responses (file contents, directory listings, errors)
- Uses custom `LlmSftpHandler` that translates SFTP packets to LLM events

### 2. Shell Input Handling
Complex terminal emulation for proper SSH client experience:

**Character Echo**:
- Every printable character is echoed back to client (0x20-0x7E)
- Backspace/delete (0x7F, 0x08): Echo `\x08 \x08` to erase character visually
- Control characters: Echo as `^C\r\n` format for visibility

**Line Buffering**:
- Input accumulated in per-channel buffer until Enter or control character
- Prevents partial command processing
- Buffer cleared after LLM processes command

**Line Ending Normalization**:
- SSH requires `\r\n` (CRLF) line endings for proper terminal display
- Helper function `normalize_line_endings()` converts `\n` to `\r\n`
- Applied to all LLM output before sending to client

**Prompt Management**:
- After command output, server sends "$ " prompt automatically
- Keeps shell interactive without LLM needing to include prompt in every response
- User knows where to type next command

### 3. Connection and Channel Tracking
**Connection Lifecycle**:
1. `new_client()` - Create `SshHandler` for connection, register in `ServerInstance`
2. Authentication - LLM decides to accept/reject user
3. Channel open - Client requests session (shell) or subsystem (SFTP)
4. Shell/SFTP operations - LLM handles commands/file operations
5. Channel close - Remove from tracking maps

**Multiple Channels Per Connection**:
- SSH supports multiple channels (e.g., shell + port forwarding)
- Each channel tracked separately in `channel_types` HashMap
- Shell channels have input buffers and initialization state
- SFTP channels hand off to `russh_sftp::server::run()`

### 4. SFTP Integration
SFTP runs as a subsystem over SSH:
- Client sends "subsystem" request with name="sftp"
- Server creates `LlmSftpHandler` and passes channel to `russh_sftp::server::run()`
- russh_sftp parses all SFTP packets and calls trait methods
- `LlmSftpHandler` converts SFTP operations to LLM events and back

**Supported SFTP Operations**:
- `SSH_FXP_REALPATH` - Canonicalize path
- `SSH_FXP_STAT` / `SSH_FXP_LSTAT` - Get file attributes
- `SSH_FXP_OPENDIR` - Open directory for reading
- `SSH_FXP_READDIR` - Read directory entries
- `SSH_FXP_OPEN` - Open file for reading/writing
- `SSH_FXP_READ` - Read file contents
- `SSH_FXP_WRITE` - Write file contents
- `SSH_FXP_CLOSE` - Close file/directory handle
- `SSH_FXP_REMOVE` - Delete file
- `SSH_FXP_MKDIR` - Create directory
- `SSH_FXP_RMDIR` - Remove directory
- `SSH_FXP_RENAME` - Rename file/directory

### 5. Dual Logging
All operations use **dual logging**:
- **DEBUG**: Request/response summaries (e.g., "SSH shell command: ls -la")
- **TRACE**: Full payloads (command text, file contents, packet hex)
- **INFO**: Lifecycle events (connection opened, SFTP subsystem started)
- **ERROR**: Failures (authentication denied, SFTP errors)
- All logs go to both `netget.log` (via tracing) and TUI Status panel (via status_tx)

## LLM Integration

### Action-Based Response Model
The LLM responds to SSH events with actions:

**Events**:
- `ssh_auth` - Authentication request (username, auth_type)
- `ssh_banner` - Shell opened, send welcome banner
- `ssh_shell_command` - Shell command received
- `sftp_*` - SFTP operations (read, write, readdir, etc.)

**Available Actions**:
- `ssh_auth_decision` - Allow/deny authentication
- `ssh_send_banner` - Send shell banner
- `ssh_shell_response` - Send shell output
- `close_connection` - Close SSH connection
- `sftp_response` - Return SFTP data (file contents, directory listings)
- Common actions: `show_message`, `update_instruction`, etc.

### Example LLM Responses

**Authentication**:
```json
{
  "actions": [
    {
      "type": "ssh_auth_decision",
      "allowed": true,
      "message": "User alice authenticated successfully"
    }
  ]
}
```

**Shell Banner**:
```json
{
  "actions": [
    {
      "type": "ssh_shell_response",
      "output": "Welcome to NetGet SSH Server\r\nLast login: Mon Jan 1 12:00:00 2024\r\n$ "
    }
  ]
}
```

**Shell Command**:
```json
{
  "actions": [
    {
      "type": "ssh_shell_response",
      "output": "file1.txt\r\nfile2.txt\r\n",
      "close_connection": false
    }
  ]
}
```

**SFTP Directory Listing**:
```json
{
  "entries": [
    {"name": "readme.txt", "size": 100, "is_dir": false},
    {"name": "data", "size": 4096, "is_dir": true}
  ]
}
```

## Connection Management

### Host Key Generation
- Ed25519 key pair generated on server startup (`generate_host_key()`)
- Key is ephemeral (not persisted to disk)
- Each server instance has unique host key
- Clients will see "host key changed" warning if server restarts

### Authentication Flow
1. Client sends authentication request (password or publickey)
2. Handler calls `llm_auth_decision()` with username and auth type
3. LLM returns `ssh_auth_decision` action with `allowed: true/false`
4. Handler returns `Auth::Accept` or `Auth::Reject` to russh
5. If accepted, connection marked as authenticated in `ServerInstance`

### Shell Session Flow
1. Client opens session channel
2. Handler accepts channel, stores in `channels` HashMap
3. Client sends `shell_request` or `exec_request`
4. Handler calls `llm_shell_banner()` to get initial banner (for shell_request)
5. Client sends input data
6. Handler buffers input until newline or control character
7. Handler calls `llm_shell_command()` with buffered line
8. LLM returns output, handler sends to client
9. Handler automatically sends "$ " prompt
10. Loop continues until client closes channel or LLM sends `close_connection`

### SFTP Session Flow
1. Client sends `subsystem_request` with name="sftp"
2. Handler creates `LlmSftpHandler` and channels
3. Handler passes channel to `russh_sftp::server::run()`
4. russh_sftp parses SFTP packets and calls trait methods on `LlmSftpHandler`
5. `LlmSftpHandler` translates to LLM events and actions
6. SFTP responses sent back through russh_sftp to client
7. Session ends when client sends `SSH_FXP_CLOSE` or closes channel

## Known Limitations

### 1. Ephemeral Host Keys
- Host key not persisted across restarts
- Clients will see "WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED" on restart
- No option to load host key from file

**Workaround**: Not critical for testing/honeypot scenarios where server identity isn't important.

### 2. No SFTP Persistence
- SFTP filesystem is purely virtual (defined by LLM responses)
- No actual files stored on disk
- File writes are processed but not persisted
- Each SFTP session starts with clean slate (unless LLM uses memory)

**Workaround**: For testing and honeypot scenarios, virtual filesystem is sufficient.

### 3. Limited Authentication Methods
- Only password and publickey supported
- No keyboard-interactive authentication
- No certificate-based authentication
- No multi-factor authentication

**Rationale**: Password and publickey cover 95% of SSH use cases.

### 4. No Port Forwarding
- SSH port forwarding (local/remote/dynamic) not implemented
- No X11 forwarding
- Only shell and SFTP channels supported

**Rationale**: Port forwarding requires complex network plumbing beyond protocol handling.

### 5. No Session Multiplexing
- Each SSH connection requires separate TCP connection
- No support for SSH connection sharing/multiplexing
- No ControlMaster equivalent

**Rationale**: Not critical for server-side implementation.

### 6. Input Echo Limitations
- Tab completion not implemented (tab echoed but no completion logic)
- No command history (up/down arrows not handled)
- No line editing shortcuts (Ctrl-A, Ctrl-E work but Ctrl-K doesn't delete to end)

**Rationale**: Full readline emulation would be extremely complex. Basic echo is sufficient for testing.

## Example Prompts

### Basic SSH Server
```
listen on port 2222 via ssh
Allow user 'admin' with any password
When authenticated, send banner "Welcome to NetGet SSH!"
For 'ls' command, list: file1.txt, file2.txt, directory1/
For 'pwd' command, return "/home/admin"
For 'exit' command, send "Goodbye!" and close connection
```

### SFTP Server with Files
```
listen on port 22 via ssh
Accept all users with password 'test'
Enable SFTP subsystem
Virtual filesystem:
- /readme.txt: "Hello from NetGet SFTP!\nThis is a virtual file.\n"
- /data.json: {"status": "ok", "files": 42}
- /logs/ (directory with access.log and error.log)
When client lists /, show: readme.txt (100 bytes), data.json (50 bytes), logs (dir)
```

### SSH Honeypot
```
listen on port 22 via ssh
Log all authentication attempts
Deny all users except 'root'
For 'root', accept password 'toor'
After login, simulate Linux shell:
- uname -a: "Linux honeypot 5.10.0 x86_64 GNU/Linux"
- whoami: "root"
- ls: "flag.txt database.db"
- cat flag.txt: "You found the honeypot!"
```

### Multi-User SSH
```
listen on port 22 via ssh
Allow users: alice, bob, charlie
Alice gets admin prompt: "alice@server# "
Bob and charlie get user prompt: "user@server$ "
Each user sees different files in 'ls':
- alice: system.conf, users.db
- bob: documents.txt
- charlie: projects/
```

## Performance Characteristics

### Latency
- Authentication: 1 LLM call (~2-5s)
- Shell banner: 1 LLM call (~2-5s)
- Each shell command: 1 LLM call (~2-5s)
- SFTP operations: 1 LLM call per operation (~2-5s)

**Scripting Mode Improvement**:
- Authentication: 0 LLM calls after setup (script handles)
- Shell commands: 0 LLM calls (script handles)
- SFTP operations: Can be scripted for common paths

### Throughput
- Limited by LLM response time
- Concurrent connections handled in parallel
- russh handles encryption/decryption efficiently (minimal overhead)

### Concurrency
- Unlimited concurrent connections (bounded by system resources)
- Each connection has independent `SshHandler` instance
- Ollama lock serializes LLM API calls but not SSH protocol handling

## Security Considerations

### Cryptography
- russh handles all encryption (AES, ChaCha20, etc.)
- Key exchange uses modern algorithms (Curve25519)
- Host key uses Ed25519 (modern, secure elliptic curve)

### Authentication
- LLM controls all authentication decisions
- No passwords stored (LLM decides based on prompt/instructions)
- Public key authentication supported (key validated by russh, LLM approves user)

### Honeypot Usage
SSH is commonly used for honeypots:
- Log authentication attempts (usernames, passwords)
- Simulate vulnerable systems
- Capture attacker commands
- LLM can adapt responses based on attacker behavior

## References
- [RFC 4253: SSH Transport Layer Protocol](https://datatracker.ietf.org/doc/html/rfc4253)
- [RFC 4254: SSH Connection Protocol](https://datatracker.ietf.org/doc/html/rfc4254)
- [RFC 4256: SSH Keyboard-Interactive Authentication](https://datatracker.ietf.org/doc/html/rfc4256)
- [russh Documentation](https://docs.rs/russh/latest/russh/)
- [russh-sftp Documentation](https://docs.rs/russh-sftp/latest/russh_sftp/)
- [SSH Protocol Overview](https://www.ssh.com/academy/ssh/protocol)
