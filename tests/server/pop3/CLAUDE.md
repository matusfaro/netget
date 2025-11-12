# POP3 Protocol E2E Tests

## Test Overview

Tests POP3 server with raw TCP clients validating RFC 1939 command/response sequences.

## Test Strategy

- **Consolidated tests** - Each test focuses on a specific POP3 workflow
- **Multiple server instances** - 4 separate servers (one per test)
- **Real TCP clients** - Manual socket I/O with line-based protocol
- **No POP3 library** - Tests use `tokio::net::TcpStream` directly

## LLM Call Budget

- `test_pop3_greeting()`: 1 startup call (greeting on connect)
- `test_pop3_authentication()`: 1 startup call + 2 commands (USER, PASS)
- `test_pop3_stat()`: 1 startup call + 3 commands (USER, PASS, STAT)
- `test_pop3_quit()`: 1 startup call + 1 QUIT command
- **Total: 12 LLM calls** (4 startups + 8 command calls)

## Scripting Usage

**Scripting Disabled** - POP3 tests use action-based responses only

- POP3 protocol is conversational (each command requires context)
- Script generation not beneficial for command/response patterns
- LLM interprets each command dynamically
- State machine (Authorization → Transaction → Update) managed by LLM

## Client Library

**Manual TCP Client** - No POP3 library used

- `tokio::net::TcpStream` for connections
- `BufReader::read_line()` for reading responses
- `AsyncWriteExt::write_all()` for sending commands
- Line-based parsing with `\r\n` terminators

## Expected Runtime

- Model: qwen3-coder:30b
- Runtime: ~45-60 seconds for full test suite
- Moderate speed due to 12 LLM calls

## Failure Rate

- **Low-Medium** (5-10%) - Occasional LLM non-compliance
- LLM may not format POP3 responses correctly (missing \r\n, wrong prefix)
- Most common issue: LLM returns prose instead of protocol responses
- LLM may forget +OK/-ERR prefix

## Test Cases

1. **test_pop3_greeting** - Validates +OK greeting on connect
2. **test_pop3_authentication** - Tests USER and PASS commands with +OK responses
3. **test_pop3_stat** - Tests STAT command for mailbox status (message count, total size)
4. **test_pop3_quit** - Tests QUIT command and +OK response

## Known Issues

- **Lenient assertions** - Tests check for response codes anywhere in output (not just at start)
- Some tests may pass even if responses are malformed
- LLM occasionally forgets to send greeting on connection
- Timeouts set to 10 seconds to accommodate slow LLM responses
- Multiline responses (LIST, RETR) not tested yet (future enhancement)

## Example Test Pattern

```rust
// Start server with prompt
let server = start_netget_server(ServerConfig::new(prompt)).await?;

// Connect via TCP
let stream = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
let (read_half, mut write_half) = stream.into_split();
let mut reader = BufReader::new(read_half);

// Read greeting
let mut line = String::new();
reader.read_line(&mut line).await?;

// Send POP3 command
write_half.write_all(b"USER alice\r\n").await?;

// Read response
line.clear();
reader.read_line(&mut line).await?;
assert!(line.contains("+OK"));
```

## Future Enhancements

1. **Multiline response testing** - Test LIST, RETR, UIDL, TOP commands
2. **Error handling** - Test -ERR responses for invalid commands
3. **Message retrieval** - Test RETR command with full email content
4. **Deletion** - Test DELE command for message deletion
5. **TLS support** - Test POP3S (implicit TLS on port 995)
