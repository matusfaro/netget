# FTP Server Implementation

## Overview

FTP (File Transfer Protocol) server implementation following RFC 959. This implementation provides LLM-controlled responses to FTP commands over the control channel.

## Architecture

### Control Channel Only

This implementation handles only the FTP control channel (command/response). Data channel operations (PORT/PASV mode) are simulated through inline responses. This simplifies the implementation while still allowing useful FTP interactions.

### Connection Flow

1. Client connects to control port (typically 21)
2. Server sends 220 greeting (via LLM)
3. Client sends commands (USER, PASS, LIST, etc.)
4. Server responds via LLM-controlled actions
5. Session ends with QUIT command

## Library Choices

- **tokio**: Async TCP handling
- **No external FTP library**: Manual line-based parsing for full LLM control

## LLM Integration

### Events

| Event | Description | Parameters |
|-------|-------------|------------|
| `ftp_command` | FTP command received | `command` - full command string |

### Actions

| Action | Description | Parameters |
|--------|-------------|------------|
| `send_ftp_response` | Single-line response | `code`, `message` |
| `send_ftp_multiline` | Multi-line response | `code`, `lines[]` |
| `send_ftp_data` | Raw data | `data` |
| `send_ftp_list` | Directory listing | `entries[]` |
| `wait_for_more` | Wait for more input | - |
| `close_connection` | Close connection | - |

### Common FTP Response Codes

- 220: Service ready (greeting)
- 230: User logged in
- 250: Requested file action okay
- 257: "PATHNAME" created
- 331: User name okay, need password
- 350: Requested file action pending further information
- 421: Service not available
- 425: Can't open data connection
- 450: Requested file action not taken
- 500: Syntax error, command unrecognized
- 530: Not logged in
- 550: Requested action not taken (file unavailable)

## Limitations

1. **No Data Channel**: True data transfers (PORT/PASV) not implemented
2. **No TLS**: FTPS/implicit TLS not supported
3. **Control Channel Only**: All data must be sent inline or simulated
4. **No Binary Mode**: Only ASCII text transfers conceptually supported

## Example Usage

```
User: Start an FTP server on port 2121 that allows anonymous login

LLM Response:
- On CONNECTION_ESTABLISHED: send_ftp_response(220, "FTP Server Ready")
- On USER anonymous: send_ftp_response(331, "Anonymous login okay, send email as password")
- On PASS *: send_ftp_response(230, "Anonymous user logged in")
- On SYST: send_ftp_response(215, "UNIX Type: L8")
- On PWD: send_ftp_response(257, "\"/\" is current directory")
- On LIST: send_ftp_response(150, "Opening data connection") then send_ftp_list([entries])
- On QUIT: send_ftp_response(221, "Goodbye") then close_connection
```

## Testing

Test with standard FTP clients:
- `nc localhost 2121` (raw commands)
- `lftp localhost:2121`
- `ftp localhost 2121`
