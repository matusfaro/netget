# TFTP Client Implementation

## Overview

TFTP client for downloading files from TFTP servers (RRQ) and uploading files (WRQ). The LLM controls file transfer operations and processes received data.

## Library Choice

**Custom Implementation** - No external dependencies. Simple UDP packet construction and parsing.

## Architecture

### Connection Flow

```
1. Parse instruction → determine operation (read/write)
2. Bind local UDP socket
3. Send RRQ or WRQ to server:69
4. Server responds from TID port
5. Exchange DATA/ACK packets
6. Call LLM on each received block
7. Transfer complete when final block (< 512 bytes)
```

### LLM Integration

**Events**:
- `tftp_connected` - Initial connection
- `tftp_data_received` - DATA block received (read)
- `tftp_ack_received` - ACK received (write)
- `tftp_transfer_complete` - Transfer finished
- `tftp_error` - Error from server

**Actions**:
- `tftp_read_file` - Request file (async)
- `tftp_write_file` - Send file (async)
- `send_ack` - Acknowledge DATA block (sync)
- `send_data_block` - Send DATA block (sync)
- `disconnect` - Abort transfer (sync)

### Data Encoding

Hex-encoded like TCP client:
```json
{
  "data_hex": "48656c6c6f",
  "data_length": 5
}
```

## Limitations

- **No Retransmission**: Timeouts cause transfer abort (5-second timeout)
- **No Options**: RFC 1350 only, no blocksize/timeout negotiation
- **Simple Instruction Parsing**: Looks for "read"/"write" and filename in instruction
- **Single Transfer**: One operation per client instance

## Example Usage

**Read File**:
```
connect to 192.168.1.1:69 via tftp
Read file pxelinux.0 in octet mode
```

**Write File**:
```
connect to 192.168.1.1:69 via tftp
Write config.txt with data "Hello TFTP!"
```
