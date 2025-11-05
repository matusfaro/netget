# BitTorrent Peer Wire Protocol - Testing

## Testing Strategy

**Test Type**: Black-box E2E testing with real BitTorrent clients
**Test File**: `tests/server/torrent_peer/e2e_test.rs`
**Feature Gate**: `#[cfg(all(test, feature = "torrent-peer"))]`

## Test Execution

### Prerequisites
```bash
# Build release binary
./cargo-isolated.sh build --release --no-default-features --features torrent-peer

# Install test dependencies (Ubuntu/Debian)
sudo apt-get install transmission-cli aria2 python3-bencodepy

# Or macOS
brew install transmission aria2
pip3 install bencode.py
```

### Running Tests
```bash
# Run peer E2E tests only
./cargo-isolated.sh test --no-default-features --features torrent-peer --test torrent_peer_e2e

# Expected output:
# - test test_peer_handshake ... ok (5-10s)
# - test test_peer_bitfield_exchange ... ok (5-10s)
# - test test_peer_piece_request ... ok (10-15s)
```

## LLM Call Budget

**Target**: < 10 LLM calls per test suite
**Actual Breakdown**:
- **Server startup**: 1 call (parse prompt)
- **First handshake**: 1 call (new event type)
- **Subsequent handshakes**: 0 calls (pattern reuse)
- **First message type**: 1 call per type (choke, request, bitfield)
- **Subsequent messages**: 0 calls (pattern reuse)

**Total Estimated**: 5-7 LLM calls per full suite (well under 10 target)

**Optimization**: Single server instance for all test cases. Group related scenarios.

## Runtime Expectations

**Per Test**:
- Server startup: 5-10s (LLM parses instruction)
- Handshake exchange: 0.5-2s (first slower, subsequent faster)
- Per piece request: 0.5-2s
- TCP connection: < 100ms

**Full Suite**: 20-40s with Ollama `qwen3-coder:30b` model

## Test Scenarios

### Test 1: Basic Handshake Exchange

**Objective**: Verify peer handshake protocol

**Setup**:
1. Start NetGet peer on port {AVAILABLE_PORT}
2. Prompt: "Start a BitTorrent peer. Respond to handshakes with peer ID '-NT0001-xxxxxxxxxxxx'. Echo back the received info_hash."

**Test Steps**:
1. Create test info_hash (20 bytes): `0123456789abcdef0123456789abcdef01234567`
2. Connect via TCP to NetGet peer
3. Send handshake:
   ```rust
   let mut handshake = Vec::new();
   handshake.push(19);
   handshake.extend_from_slice(b"BitTorrent protocol");
   handshake.extend_from_slice(&[0u8; 8]);  // reserved
   handshake.extend_from_slice(&hex::decode(info_hash)?);  // 20 bytes
   handshake.extend_from_slice(b"TEST_PEER_ID12345678");   // 20 bytes
   stream.write_all(&handshake).await?;
   ```
4. Read response handshake (68 bytes)
5. Verify response:
   - pstrlen == 19
   - pstr == "BitTorrent protocol"
   - info_hash matches (bytes 28-48)
   - peer_id present (bytes 48-68)

**Validation**:
```rust
let mut response = vec![0u8; 68];
stream.read_exact(&mut response).await?;

assert_eq!(response[0], 19, "Invalid pstrlen");
assert_eq!(&response[1..20], b"BitTorrent protocol", "Invalid pstr");

let info_hash_resp = hex::encode(&response[28..48]);
assert_eq!(info_hash_resp, info_hash, "Info hash mismatch");

let peer_id_resp = String::from_utf8_lossy(&response[48..68]);
println!("Peer ID: {}", peer_id_resp);
```

### Test 2: Bitfield Exchange

**Objective**: Test bitfield message (seeder announces pieces)

**Setup**:
1. Prompt: "Start a BitTorrent seeder. After handshake, send bitfield 'ff' (all 8 pieces available)."

**Test Steps**:
1. Connect and complete handshake
2. Wait for bitfield message:
   ```
   <length> <id=5> <bitfield bytes>
   ```
3. Parse message:
   ```rust
   let mut len_buf = [0u8; 4];
   stream.read_exact(&mut len_buf).await?;
   let length = u32::from_be_bytes(len_buf) as usize;

   let mut message = vec![0u8; length];
   stream.read_exact(&mut message).await?;

   assert_eq!(message[0], 5, "Expected bitfield message");
   let bitfield = &message[1..];
   assert_eq!(hex::encode(bitfield), "ff", "Expected all pieces");
   ```

**Bitfield Verification**:
```rust
// Parse bitfield bits
fn parse_bitfield(bitfield: &[u8]) -> Vec<bool> {
    let mut pieces = Vec::new();
    for byte in bitfield {
        for bit in (0..8).rev() {
            pieces.push((byte >> bit) & 1 == 1);
        }
    }
    pieces
}

let pieces = parse_bitfield(bitfield);
println!("Pieces: {:?}", pieces);  // [true, true, true, ...]
```

### Test 3: Choke/Unchoke Messages

**Objective**: Test connection state management

**Setup**:
1. Prompt: "Start a BitTorrent peer. After handshake, send unchoke message. If peer sends request, choke them."

**Test Steps**:
1. Complete handshake
2. Wait for unchoke message:
   ```
   00 00 00 01 01  (length=1, id=1)
   ```
3. Send interested message:
   ```rust
   stream.write_all(&[0, 0, 0, 1, 2]).await?;  // interested
   ```
4. Send piece request:
   ```rust
   let request = vec![
       0, 0, 0, 13,  // length = 13
       6,             // id = request
       0, 0, 0, 0,    // index = 0
       0, 0, 0, 0,    // begin = 0
       0, 0, 64, 0,   // length = 16384
   ];
   stream.write_all(&request).await?;
   ```
5. Wait for choke message:
   ```
   00 00 00 01 00  (length=1, id=0)
   ```

**Validation**:
```rust
let mut msg_buf = vec![0u8; 5];
stream.read_exact(&mut msg_buf).await?;

assert_eq!(&msg_buf[..4], &[0, 0, 0, 1], "Invalid length");
assert_eq!(msg_buf[4], 0, "Expected choke message");
```

### Test 4: Piece Request and Transfer

**Objective**: Test actual piece data transfer

**Setup**:
1. Prompt: "Start a BitTorrent seeder. You have all pieces. For piece requests, send fake data '48656c6c6f' (hex for 'Hello'). Keep peers unchoked."

**Test Steps**:
1. Complete handshake
2. Wait for unchoke (if seeder sends it)
3. Send interested:
   ```rust
   stream.write_all(&[0, 0, 0, 1, 2]).await?;
   ```
4. Send request (piece 0, begin 0, length 5):
   ```rust
   let request = vec![
       0, 0, 0, 13,  // length = 13
       6,             // id = request
       0, 0, 0, 0,    // index = 0
       0, 0, 0, 0,    // begin = 0
       0, 0, 0, 5,    // length = 5
   ];
   stream.write_all(&request).await?;
   ```
5. Read piece message:
   ```
   <length=14> <id=7> <index=0> <begin=0> <block="Hello">
   ```
6. Verify block data

**Validation**:
```rust
// Read length
let mut len_buf = [0u8; 4];
stream.read_exact(&mut len_buf).await?;
let length = u32::from_be_bytes(len_buf) as usize;

// Read message
let mut message = vec![0u8; length];
stream.read_exact(&mut message).await?;

assert_eq!(message[0], 7, "Expected piece message");

let index = u32::from_be_bytes([message[1], message[2], message[3], message[4]]);
let begin = u32::from_be_bytes([message[5], message[6], message[7], message[8]]);
let block = &message[9..];

assert_eq!(index, 0);
assert_eq!(begin, 0);
assert_eq!(hex::encode(block), "48656c6c6f");
println!("Received: {}", String::from_utf8_lossy(block));  // "Hello"
```

### Test 5: Have Message

**Objective**: Test piece availability announcements

**Setup**:
1. Prompt: "Start a BitTorrent leecher. After handshake, send have messages for pieces 0, 1, 2."

**Test Steps**:
1. Complete handshake
2. Read have messages:
   ```
   00 00 00 05 04 00 00 00 00  (have piece 0)
   00 00 00 05 04 00 00 00 01  (have piece 1)
   00 00 00 05 04 00 00 00 02  (have piece 2)
   ```

**Validation**:
```rust
for expected_piece in 0..3 {
    let mut msg = vec![0u8; 9];
    stream.read_exact(&mut msg).await?;

    assert_eq!(&msg[..5], &[0, 0, 0, 5, 4], "Invalid have message");

    let piece_index = u32::from_be_bytes([msg[5], msg[6], msg[7], msg[8]]);
    assert_eq!(piece_index, expected_piece);
}
```

### Test 6: Multiple Peer Connections

**Objective**: Test concurrent peer connections

**Setup**:
1. Prompt: "Start a BitTorrent seeder. Handle multiple peer connections simultaneously. Track each peer's state independently."

**Test Steps**:
1. Open 3 TCP connections simultaneously
2. Each sends handshake with different peer_id
3. Verify each receives independent responses
4. Send requests on connection 1, verify only connection 1 gets pieces

**Validation**:
```rust
let mut handles = Vec::new();

for i in 0..3 {
    let handle = tokio::spawn(async move {
        let mut stream = TcpStream::connect(peer_addr).await?;

        let peer_id = format!("PEER{:016}", i);
        // Send handshake with unique peer_id
        // Verify independent response
        Ok(())
    });
    handles.push(handle);
}

for handle in handles {
    handle.await??;
}
```

### Test 7: Keepalive Messages

**Objective**: Test connection maintenance

**Setup**:
1. Prompt: "Start a BitTorrent peer. Send keepalive messages every 5 seconds."

**Test Steps**:
1. Complete handshake
2. Wait up to 10 seconds
3. Verify keepalive received:
   ```
   00 00 00 00  (length=0, no message_id)
   ```

**Validation**:
```rust
tokio::select! {
    result = stream.read_exact(&mut buf) => {
        result?;
        assert_eq!(&buf, &[0, 0, 0, 0], "Expected keepalive");
    }
    _ = tokio::time::sleep(Duration::from_secs(10)) => {
        // No keepalive received (acceptable if connection active)
    }
}
```

## Real Client Testing

### Using transmission-cli

**Create Test Torrent and Seeder**:
```bash
# Create dummy file
dd if=/dev/zero of=test.dat bs=1M count=10

# Create torrent
transmission-create -o test.torrent test.dat

# Start NetGet seeder (with extracted info_hash)
# Prompt: "Start a BitTorrent seeder on port 51413 for info_hash <hash>"

# Start transmission downloader (point to NetGet)
transmission-cli test.torrent --peer 127.0.0.1:51413
```

**Verify**:
- transmission connects to NetGet
- Handshake exchanged
- Bitfield/pieces transferred
- Download progresses

### Using aria2

**Download from NetGet Seeder**:
```bash
aria2c --bt-enable-lpd=false --enable-dht=false \
       --bt-external-ip=127.0.0.1 \
       --listen-port=6881 \
       --seed-time=0 \
       test.torrent
```

**Add NetGet as peer** (manual connection):
```bash
# aria2 doesn't support manual peer addition easily
# Use transmission or custom Python client instead
```

### Custom Python Client

**Simple Peer Client**:
```python
#!/usr/bin/env python3
import socket
import struct

def send_handshake(sock, info_hash, peer_id):
    handshake = (
        b'\x13' +                      # pstrlen
        b'BitTorrent protocol' +
        b'\x00' * 8 +                  # reserved
        bytes.fromhex(info_hash) +     # 20 bytes
        peer_id.encode()[:20]          # 20 bytes
    )
    sock.send(handshake)
    return sock.recv(68)

def send_request(sock, index, begin, length):
    message = struct.pack('>IBIII', 13, 6, index, begin, length)
    sock.send(message)

# Connect to NetGet peer
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect(('127.0.0.1', 51413))

# Handshake
resp = send_handshake(sock, '0123456789abcdef0123456789abcdef01234567', 'TESTPEER12345678901')
print(f"Handshake response: {resp.hex()}")

# Send interested
sock.send(struct.pack('>IB', 1, 2))

# Request piece 0
send_request(sock, 0, 0, 16384)

# Read piece response
length = struct.unpack('>I', sock.recv(4))[0]
message = sock.recv(length)
print(f"Piece data: {message[:100]}...")  # First 100 bytes

sock.close()
```

## Test Helpers

**Custom Helpers Needed**:
```rust
/// Build handshake message
fn build_handshake(info_hash: &str, peer_id: &str) -> Vec<u8> {
    let mut handshake = Vec::new();
    handshake.push(19);
    handshake.extend_from_slice(b"BitTorrent protocol");
    handshake.extend_from_slice(&[0u8; 8]);
    handshake.extend_from_slice(&hex::decode(info_hash).unwrap());
    handshake.extend_from_slice(peer_id.as_bytes());
    handshake
}

/// Read length-prefixed message
async fn read_message(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let length = u32::from_be_bytes(len_buf) as usize;

    if length == 0 {
        return Ok(Vec::new());  // Keepalive
    }

    let mut message = vec![0u8; length];
    stream.read_exact(&mut message).await?;
    Ok(message)
}

/// Parse message type
fn get_message_type(message: &[u8]) -> &'static str {
    if message.is_empty() { return "keepalive"; }
    match message[0] {
        0 => "choke",
        1 => "unchoke",
        2 => "interested",
        3 => "not_interested",
        4 => "have",
        5 => "bitfield",
        6 => "request",
        7 => "piece",
        8 => "cancel",
        _ => "unknown",
    }
}

/// Build piece request message
fn build_request(index: u32, begin: u32, length: u32) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(&13u32.to_be_bytes());  // length
    message.push(6);                                   // id = request
    message.extend_from_slice(&index.to_be_bytes());
    message.extend_from_slice(&begin.to_be_bytes());
    message.extend_from_slice(&length.to_be_bytes());
    message
}
```

## Known Issues

1. **Fake Piece Data**: LLM doesn't have real torrent files, so piece data is random/fake. This is expected for testing protocol compliance, not actual file transfer.

2. **No Piece Verification**: SHA-1 hashes not checked. Real clients may reject pieces.

3. **State Tracking**: LLM may not perfectly track choke state, piece availability, etc. across requests.

4. **Concurrent Requests**: LLM processes one message at a time per connection. High request rates may queue.

5. **Connection Persistence**: Long-lived connections may timeout if no keepalives sent.

## Debugging

**Enable TRACE logging**:
```bash
RUST_LOG=trace ./target/release/netget
```

**Wireshark Capture**:
```bash
sudo tcpdump -i lo -n -X 'tcp port 51413' -w peer_traffic.pcap

# View in Wireshark (BitTorrent dissector available)
wireshark peer_traffic.pcap
```

**Hex Dump Messages**:
```bash
# In tests, log all messages
println!("Sent: {}", hex::encode(&message));
println!("Received: {}", hex::encode(&response));
```

## Performance Benchmarks

**Single Connection**:
- Handshake: ~1-3s (first, with LLM)
- Piece request: ~1-2s (first, then faster)
- 10 sequential piece requests: ~5-10s (pattern reuse)

**Multiple Connections** (3 simultaneous):
- Handshakes: ~3-5s (processed concurrently)
- Requests: ~10-15s (serialized per connection)

## Success Criteria

✅ **Pass Criteria**:
- Handshake exchange successful with real clients
- Bitfield/Have messages parsed correctly
- Piece requests receive valid responses
- Multiple concurrent connections supported
- < 10 LLM calls total

❌ **Failure Indicators**:
- Handshake parse errors
- Invalid message formats (wrong length, bad encoding)
- Connection drops immediately after handshake
- Timeouts on piece requests (> 30s)

## References

- [BEP 3: Peer Wire Protocol](http://www.bittorrent.org/beps/bep_0003.html)
- [BitTorrent Protocol Specification](https://wiki.theory.org/BitTorrentSpecification#Peer_wire_protocol_.28TCP.29)
- [transmission Protocol Documentation](https://github.com/transmission/transmission/blob/main/docs/Protocol.md)
- [Wireshark BitTorrent Dissector](https://wiki.wireshark.org/BitTorrent)
