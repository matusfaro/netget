# NFC (Near Field Communication) Client Implementation

## Library Choice

**pcsc v2.9** - PC/SC (Personal Computer/Smart Card) API bindings for Rust
**ndef-rs v0.1** - NDEF (NFC Data Exchange Format) message parsing

### PC/SC Cross-Platform Support

- ✅ **Windows**: Native WinSCard.dll (built-in)
- ✅ **macOS**: Native PCSC framework (built-in)
- ✅ **Linux**: PCSC lite library via pcscd daemon (system package)
- ✅ 726K+ downloads - Very mature library
- ✅ Industry standard for smart card/NFC reader access
- Documentation: https://docs.rs/pcsc/

### System Dependencies

**Linux only** (Windows/macOS have native support):
```bash
# Ubuntu/Debian
sudo apt install pcscd libpcsclite-dev

# Fedora/RHEL
sudo dnf install pcsc-lite pcsc-lite-devel

# Start daemon
sudo systemctl start pcscd
sudo systemctl enable pcscd
```

**Windows/macOS**: No system dependencies required

### Hardware Requirements

- **Recommended**: ACR122U USB NFC Reader (~$40)
  - PC/SC compliant on all platforms
  - Supports ISO14443A/B, MIFARE, FeliCa, NFC tags
  - Plug-and-play on Windows/macOS
  - Simple setup on Linux

- **Alternatives**: Any PC/SC compatible NFC reader
  - Most USB smart card readers work
  - Check for ISO14443 support for NFC tags

## Architecture

### PC/SC Connection Workflow

```
Context → List Readers → Connect to Card → Transmit APDU → Disconnect
```

1. **Context**: PC/SC context (User scope)
2. **List Readers**: Enumerate available readers
3. **Connect**: Wait for card/tag in reader (blocking or timeout)
4. **Transmit APDU**: Send Application Protocol Data Unit commands
5. **Disconnect**: Release card connection

### LLM Integration Points

The LLM has full control over NFC reader operations:

1. **Reader Selection** (Startup):
    - Parameter: `reader_index` (0-based) or `reader_name` (string)
    - Event: `nfc_readers_listed` with available readers
    - LLM can choose specific reader

2. **Card Detection** (Async Action):
    - Action: `connect_card` with optional timeout
    - Waits for card/tag to be presented to reader
    - Event: `nfc_card_detected` with ATR (Answer to Reset)
    - LLM learns card type from ATR

3. **APDU Commands** (Sync Action):
    - Action: `send_apdu` (structured) or `send_apdu_raw` (hex string)
    - Structured format for LLM-friendly construction:
      ```json
      {
        "type": "send_apdu",
        "cla": "00",
        "ins": "A4",  // SELECT
        "p1": "04",
        "p2": "00",
        "data": "D2760000850101",  // NDEF application
        "le": "00"
      }
      ```
    - Event: `nfc_apdu_response` with data, SW1, SW2 status bytes
    - LLM interprets response and decides next command

4. **NDEF Operations** (High-level):
    - Action: `read_ndef` - Read NDEF message from tag
    - Action: `write_ndef` - Write NDEF message to tag
    - Event: `nfc_ndef_read` with structured records
    - LLM sees NDEF records as JSON (text, URI, etc.)

5. **Disconnection**:
    - Action: `disconnect_card`
    - Event: `nfc_card_disconnected`

### State Management

- **ConnectionState**: Idle/Processing/Accumulating (same as TCP client)
- **ClientState**:
    - `ctx`: PC/SC context
    - `reader_name`: Selected reader
    - `card`: Optional card handle
    - `connection_state`: Current state

### APDU Structure

APDU (Application Protocol Data Unit) commands follow ISO 7816-4:

**Command APDU**:
```
CLA INS P1 P2 [Lc Data] [Le]
```
- CLA: Class byte (00 for ISO commands)
- INS: Instruction (A4=SELECT, B0=READ BINARY, etc.)
- P1, P2: Parameters
- Lc: Data length (optional)
- Data: Command data (optional)
- Le: Expected response length (optional)

**Response APDU**:
```
[Data] SW1 SW2
```
- Data: Response data (optional)
- SW1 SW2: Status bytes (90 00 = success)

## Data Format

All NFC data exchanged with the LLM is **structured JSON**, not raw bytes:

### Structured APDU (Preferred)

```json
{
  "type": "send_apdu",
  "cla": "00",
  "ins": "A4",
  "p1": "04",
  "p2": "00",
  "data": "D2760000850101",
  "le": "00"
}
```

LLMs understand APDU fields semantically (SELECT command, NDEF application).

### Raw APDU (Alternative)

```json
{
  "type": "send_apdu_raw",
  "apdu_hex": "00A4040007D276000085010100"
}
```

For advanced users or when copying from documentation.

### NDEF Records

```json
{
  "type": "write_ndef",
  "records": [
    {
      "type": "text",
      "language": "en",
      "text": "Hello NFC!"
    },
    {
      "type": "uri",
      "uri": "https://example.com"
    }
  ]
}
```

**Why structured data?**
- LLMs cannot effectively parse or construct raw bytes
- Standard APDU commands are well-known to LLMs
- NDEF record types are semantic (text, URI, smart poster)
- Implementation handles hex encoding/decoding

## Common APDU Commands

LLMs are familiar with standard ISO 7816 and NFC Forum commands:

### SELECT Application
```
00 A4 04 00 07 D2760000850101 00
```
- Select NDEF application (AID: D2760000850101)

### READ BINARY
```
00 B0 00 00 0F
```
- Read 15 bytes from current file at offset 0

### UPDATE BINARY
```
00 D6 00 00 0C <data>
```
- Write 12 bytes to current file at offset 0

### GET DATA
```
00 CA 00 6E 00
```
- Get application-specific data

Full commands: ISO 7816-4 specification, NFC Forum specifications

## Limitations

### PC/SC Only (No libnfc)

- Cannot access raw NFC commands (libnfc-specific)
- Only APDU-based communication
- Use case: ISO14443 cards, MIFARE, NFC tags (not peer-to-peer)

### Platform-Specific Behavior

- **Linux**: Requires pcscd daemon running
- **macOS**: May need permissions for first-time access
- **Windows**: Generally works without configuration

### Card Types Supported

- ISO14443-A (MIFARE, NFC Type 2/4)
- ISO14443-B
- MIFARE Classic/Ultralight/DESFire
- NFC Forum Type 2/4 tags
- **Not supported**: Bluetooth, proprietary protocols

### Reader Limitations

- Most PC/SC readers are **read-only** (cannot emulate cards)
- Card emulation requires special hardware (HCE devices, simulators)
- ACR122U: Reader mode only

### NDEF Implementation Status

- ✅ NDEF record structure defined
- ⚠️ NDEF reading: Not yet implemented (requires APDU sequence)
- ⚠️ NDEF writing: Not yet implemented (requires APDU sequence)
- Use `send_apdu` for manual NDEF operations currently

## Error Handling

### PC/SC Errors

- Context establishment fails → pcscd not running (Linux)
- No readers found → No NFC reader connected
- Card connection timeout → No card/tag present
- APDU transmission error → Card removed or incompatible

### APDU Status Bytes

- `90 00`: Success
- `6A 82`: File not found
- `6A 86`: Incorrect P1/P2
- `6C XX`: Wrong Le (XX = correct length)
- `69 82`: Security status not satisfied

All errors logged with `tracing` and reported to LLM as events.

## Testing Considerations

### Real Hardware Required

- E2E tests need ACR122U or similar PC/SC reader
- Test NFC tags: NTAG213, MIFARE Ultralight, etc. (~$1 each)
- Cannot test without physical hardware

### Test Strategies

1. **Unit tests**: Action parsing, EventType construction (no hardware)
2. **E2E tests**: Real NFC reader + tags (requires hardware)

### LLM Call Budget

- E2E tests should minimize LLM calls (< 10 per suite)
- Reuse reader connection across test cases
- Use scripting mode for predictable APDU sequences

### Test Tags

- NTAG213: Cheap (~$1), writable, NFC Forum Type 2
- MIFARE Ultralight: Common, read-only or writable
- Test cards: Blank ISO14443 cards

## Example Usage Scenarios

### Scenario 1: Read NFC Tag UID

```
User: "Scan for NFC tag and read its UID"
LLM:
  1. Event: nfc_readers_listed (1 reader found)
  2. Action: connect_card (timeout: 30000ms)
  3. Event: nfc_card_detected (ATR: 3B8F..., UID in ATR historical bytes)
  4. LLM extracts UID from ATR or sends GET DATA APDU
```

### Scenario 2: Read NDEF Message

```
User: "Read NDEF message from NFC tag"
LLM:
  1. Action: connect_card
  2. Event: nfc_card_detected
  3. Action: send_apdu (SELECT NDEF application: D2760000850101)
  4. Event: nfc_apdu_response (90 00 = success)
  5. Action: send_apdu (SELECT Capability Container)
  6. Action: send_apdu (READ BINARY)
  7. Action: send_apdu (SELECT NDEF file)
  8. Action: send_apdu (READ BINARY NDEF data)
  9. Parse NDEF message from response data
```

### Scenario 3: Write NDEF URL to Tag

```
User: "Write URL https://example.com to NFC tag"
LLM:
  1. Action: connect_card
  2. Action: send_apdu (SELECT NDEF application)
  3. Action: send_apdu (SELECT NDEF file)
  4. Construct NDEF message with URI record
  5. Action: send_apdu (UPDATE BINARY with NDEF data)
  6. Action: disconnect_card
```

## Future Enhancements

- **Auto NDEF**: Implement high-level `read_ndef`/`write_ndef` actions
- **MIFARE Classic**: Implement authentication (sector keys)
- **NFC-DEP**: Peer-to-peer mode (if hardware supports)
- **Multiple readers**: Support switching between readers
- **Card monitoring**: Detect card insertion/removal events

## Known Issues

- NDEF high-level actions not yet implemented (use `send_apdu` directly)
- Linux requires pcscd daemon (standard package)
- Some readers may not support all card types
- Card removal during APDU transmission causes error (expected behavior)

## References

- PC/SC specification: https://pcscworkgroup.com/
- ISO 7816-4: Smart card APDU commands
- NFC Forum: https://nfc-forum.org/our-work/specifications-and-application-documents/
- NDEF specification: NFC Data Exchange Format
- ACR122U documentation: https://www.acs.com.hk/en/products/3/acr122u-usb-nfc-reader/
