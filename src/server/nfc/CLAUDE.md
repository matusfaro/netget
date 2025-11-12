# NFC (Near Field Communication) Virtual Server Implementation

## Important Note

**This is a VIRTUAL/SIMULATION server only.**

Most PC/SC readers are **read-only** and cannot emulate NFC tags or cards. This server simulates what an NFC tag would do for testing purposes without requiring special hardware.

## Why Virtual?

### Card Emulation Challenges

1. **Hardware Limitations**: Most PC/SC readers (including ACR122U) can only **read** cards, not emulate them
2. **Special Hardware Needed**:
   - Smart card simulators (~$100+)
   - Android devices with HCE (Host Card Emulation)
   - iOS devices with CoreNFC (iOS apps only)
   - Specialized NFC emulation hardware

3. **PC/SC API**: Standard PC/SC does not support card emulation mode

### Virtual Server Use Cases

- **Testing NFC client implementations**
- **Understanding NFC protocols** (APDU commands, NDEF)
- **Simulating tag responses** without physical tags
- **Educational purposes**
- **Protocol development**

## Architecture

### Virtual NFC Tag State

```rust
struct VirtualNfcTag {
    atr: String,                         // Answer to Reset
    uid: String,                         // Tag UID (7 bytes hex)
    tag_type: String,                    // Tag type (type2, type4, generic)
    ndef_records: Vec<Value>,            // NDEF message content
    selected_application: Option<String>, // Current selected AID
}
```

### LLM Integration Points

The LLM controls virtual tag behavior:

1. **Server Initialization** (Event):
    - Event: `nfc_server_started`
    - LLM configures tag properties (ATR, NDEF message)
    - Virtual tag ready to "respond" to readers

2. **Tag Configuration** (Async Actions):
    - Action: `set_atr` - Set Answer to Reset bytes
    - Action: `set_ndef_message` - Set NDEF content
    - LLM designs tag characteristics

3. **APDU Response** (Sync Action - Simulated):
    - Event: `nfc_apdu_received` - Virtual reader sent command
    - Action: `respond_to_apdu` - LLM provides response data + status
    - Simulates tag responding to SELECT, READ, WRITE commands

### Startup Parameters

- `tag_type`: "type2" (MIFARE), "type4" (ISO14443-4), "generic" (default)
- `uid`: Tag UID (hex string, auto-generated if not provided)

## Data Format

All NFC data is **structured JSON**:

### ATR (Answer to Reset)

```json
{
  "type": "set_atr",
  "atr_hex": "3B8F8001804F0CA0000003060300030000000068"
}
```

Default ATR is for NFC Type 4 tag (ISO14443-4).

### NDEF Message

```json
{
  "type": "set_ndef_message",
  "records": [
    {
      "type": "text",
      "language": "en",
      "text": "Hello from virtual NFC tag!"
    },
    {
      "type": "uri",
      "uri": "https://example.com"
    }
  ]
}
```

### APDU Response

```json
{
  "type": "respond_to_apdu",
  "data_hex": "D2760000850101",  // Response data
  "sw1": "90",                    // Status byte 1
  "sw2": "00"                     // Status byte 2 (90 00 = success)
}
```

## Virtual vs. Real Implementation

| Aspect | Virtual Server | Real Card Emulation |
|--------|----------------|---------------------|
| **Hardware** | None (simulation) | Smart card simulator, HCE device |
| **Use Case** | Testing, education | Production card emulation |
| **Network** | N/A (no socket) | N/A (RF communication) |
| **APDU** | Logged only | Actual RF transmission |
| **Cost** | Free | $100+ hardware or Android/iOS dev |

## Limitations

### Not a Real Server

- **No RF communication**: This is not actual NFC/RF
- **No reader interaction**: Cannot be detected by real NFC readers
- **Simulation only**: For testing client code logic

### Missing Features

- No actual card emulation API
- No anti-collision (UID handling)
- No cryptographic operations (MIFARE keys, DESFire auth)
- No RF layer simulation

### What This Is Useful For

- ✅ Understanding NFC protocols
- ✅ Testing NFC client APDU logic
- ✅ Simulating tag responses
- ✅ NDEF message construction

### What This Is NOT Useful For

- ❌ Actual card emulation
- ❌ Interacting with real NFC readers
- ❌ Production use
- ❌ Physical NFC testing

## Alternative: Real Card Emulation

If you need real card emulation, consider:

### Option 1: Android HCE (Host Card Emulation)

```java
// Android HCE Service
public class MyHceService extends HostApduService {
    @Override
    public byte[] processCommandApdu(byte[] commandApdu, Bundle extras) {
        // LLM could control responses via IPC
        return responseApdu;
    }
}
```

- Requires Android 4.4+
- Free (use any Android device)
- LLM could control via Android app + IPC

### Option 2: iOS CoreNFC

```swift
// iOS NFC reader/writer
let session = NFCNDEFReaderSession(...)
// iOS doesn't support card emulation mode
```

- iOS only supports reader mode, not emulation (as of iOS 16)

### Option 3: Smart Card Simulator Hardware

- Devices like "SimplyTapp" or "CardSimu"
- Cost: $100-500
- PC/SC compatible
- Real card emulation

### Option 4: OpenVPN/PC/SC Relay

Use `vsmartcard` project to relay PC/SC over network:
```
Real Reader → PC/SC Relay → Virtual Smart Card → LLM Control
```

This allows LLM to control a "smart card" that appears to readers.

## Logging

All virtual operations logged with `tracing`:

- **INFO**: Server started, tag configured
- **DEBUG**: APDU commands received (simulated)
- **TRACE**: Full APDU hex data

Example:
```
[INFO] Virtual NFC tag started: type=type4, UID=04A1B2C3D4E5F6
[DEBUG] Virtual APDU received (simulated): 00A4040007D276000085010100
[TRACE] Virtual APDU response: 9000
```

## Future Enhancements

- **PC/SC Card Emulation API**: If hardware supports it
- **Android HCE Integration**: Bridge to Android HCE service
- **vsmartcard Integration**: Use virtual smart card infrastructure
- **APDU Simulation**: Automatic responses for common commands
- **NDEF Auto-Response**: Automatically respond to NDEF SELECT/READ

## Known Issues

- This is a simulation only - no actual RF communication
- Cannot be detected by real NFC readers
- APDU events are generated manually, not from readers
- Primarily for educational/testing purposes

## References

- PC/SC specification: https://pcscworkgroup.com/
- Android HCE: https://developer.android.com/guide/topics/connectivity/nfc/hce
- vsmartcard: https://github.com/frankmorgner/vsmartcard
- NFC Forum: https://nfc-forum.org/
- ISO 7816-4: Smart card APDU commands
