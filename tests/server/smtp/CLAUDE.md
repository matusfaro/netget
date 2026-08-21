# SMTP Protocol E2E Tests

## Test Overview

Tests SMTP server with raw TCP clients validating RFC 5321 command/response sequences.

## Test Strategy

- **Consolidated tests** - Each test focuses on a specific SMTP workflow
- **Multiple server instances** - 5 separate servers (one per test)
- **Real TCP clients** - Manual socket I/O with line-based protocol
- **No SMTP library** - Tests use `tokio::net::TcpStream` directly

## LLM Call Budget

- `test_smtp_greeting()`: 1 startup call (greeting on connect)
- `test_smtp_ehlo()`: 1 startup call + 1 EHLO command
- `test_smtp_mail_transaction()`: 1 startup call + 5 mocked `smtp_command` events (greeting on
  connect, EHLO, MAIL FROM, RCPT TO, DATA). Every step **must** have an event mock: an
  unmatched event returns HTTP 500 from the mock, the greeting then fails closed with a 421
  that closes the session, and every later write dies on a broken pipe.
- `test_smtp_quit()`: 1 startup call + 1 QUIT command
- `test_smtp_error_handling()`: 1 startup call + 1 invalid command
- **Total: 15 LLM calls** (5 startups + 10 command calls)

## Scripting Usage

**Scripting Disabled** - SMTP tests use action-based responses only

- SMTP protocol is conversational (each command requires context)
- Script generation not beneficial for command/response patterns
- LLM interprets each command dynamically

## Client Library

**Manual TCP Client** - No SMTP library used

- `tokio::net::TcpStream` for connections
- `BufReader::read_line()` for reading responses
- `AsyncWriteExt::write_all()` for sending commands
- Line-based parsing with `\r\n` terminators

## Expected Runtime

- Model: qwen3-coder:30b
- Runtime: ~60-90 seconds for full test suite
- Moderate speed due to 15 LLM calls

## Failure Rate

- **Low-Medium** (5-10%) - Occasional LLM non-compliance
- LLM may not format SMTP responses correctly (missing \r\n, wrong code)
- Most common issue: LLM returns prose instead of protocol responses

## Test Cases

1. **test_smtp_greeting** - Validates 220 greeting on connect
2. **test_smtp_ehlo** - Tests EHLO command and 250 response with extensions
3. **test_smtp_mail_transaction** - Full mail transaction (EHLO → MAIL FROM → RCPT TO → DATA)
4. **test_smtp_quit** - Tests QUIT command and 221 response
5. **test_smtp_error_handling** - Validates 5xx error for invalid commands

## Known Issues

- **Lenient assertions** - Some tests still check for response codes anywhere in output;
  `test_smtp_greeting` and `test_smtp_mail_transaction` now assert `starts_with` on each
  reply code and fail on a missing/timed-out reply instead of printing a note
- Some of the remaining tolerant tests may pass even if responses are malformed
- LLM occasionally forgets to send greeting on connection
- Timeouts set to 10 seconds to accommodate slow LLM responses

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

// Send SMTP command
write_half.write_all(b"EHLO client.test\r\n").await?;

// Read response
line.clear();
reader.read_line(&mut line).await?;
assert!(line.contains("250"));
```
