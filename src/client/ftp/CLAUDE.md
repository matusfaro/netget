# FTP Client Implementation

## Overview

FTP client implementation that allows the LLM to connect to remote FTP servers, send commands, and interpret responses. The client handles the FTP control channel protocol.

## Architecture

### Control Channel Client

This implementation handles FTP control channel communication:
- Line-based request/response protocol
- Response code parsing (3-digit codes)
- Multiline response support (with `-` continuation)

### Connection Flow

1. LLM initiates connection via `open_client` action
2. Client connects to FTP server (typically port 21)
3. Server sends 220 greeting
4. LLM receives `ftp_response` event with greeting
5. LLM sends commands via `send_ftp_command` action
6. Server responses trigger `ftp_response` events
7. Session continues until QUIT or disconnect

## Library Choices

- **tokio**: Async TCP handling with `BufReader` for line-based reading
- **No external FTP library**: Manual protocol for full LLM control

## LLM Integration

### Events

| Event | Description | Parameters |
|-------|-------------|------------|
| `ftp_connected` | Connected to server | `remote_addr` |
| `ftp_response` | Response from server | `response`, `response_code` |

### Actions

| Action | Description | Parameters |
|--------|-------------|------------|
| `send_ftp_command` | Send FTP command | `command` |
| `wait_for_more` | Wait for more data | - |
| `disconnect` | Close connection | - |

### Common FTP Commands

- `USER <username>` - Set username
- `PASS <password>` - Set password
- `SYST` - Get system type
- `PWD` - Print working directory
- `CWD <path>` - Change directory
- `LIST` - List directory contents
- `RETR <file>` - Retrieve file
- `STOR <file>` - Store file
- `QUIT` - End session

## State Machine

```
                ┌─────────────────────────────────────────┐
                │                                         │
                ▼                                         │
            ┌───────┐  Response received   ┌────────────┐ │
            │ Idle  │───────────────────>│ Processing │──┘
            └───────┘                      └────────────┘
                ▲                                │
                │  LLM response complete         │ More data
                │                                ▼
                │                          ┌──────────────┐
                └──────────────────────────│ Accumulating │
                                           └──────────────┘
```

## Limitations

1. **Control Channel Only**: No passive/active mode data channel
2. **No TLS**: FTPS not supported
3. **No Binary Transfers**: Text-only interaction
4. **Synchronous Commands**: One command at a time

## Example Usage

```
User: Connect to ftp.example.com and log in as anonymous to list files

LLM Response on ftp_connected:
- wait_for_more (wait for 220 greeting)

LLM Response on ftp_response (220 greeting):
- send_ftp_command("USER anonymous")

LLM Response on ftp_response (331):
- send_ftp_command("PASS guest@example.com")

LLM Response on ftp_response (230 logged in):
- send_ftp_command("LIST")

LLM Response on ftp_response (150 opening data connection):
- wait_for_more

LLM Response on ftp_response (226 transfer complete):
- send_ftp_command("QUIT")
- disconnect
```

## Testing

Test with FTP servers:
- Local netget FTP server
- vsftpd or proftpd
- Public anonymous FTP servers
