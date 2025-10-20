# NetGet Integration Tests

## Overview

This directory contains **integration tests** for NetGet that test the full system including:
- TCP/IP stack
- LLM integration with Ollama
- Protocol implementations (FTP, HTTP, etc.)
- Real network clients

## Rust Integration Test Structure

In Rust, any test file in the `tests/` directory is automatically an **integration test**:

```
netget/
├── src/           # Library code
├── tests/         # Integration tests (separate binary)
│   └── ftp_integration_test.rs
└── Cargo.toml
```

**Key differences from unit tests:**

| Unit Tests | Integration Tests |
|------------|-------------------|
| In `src/` with `#[cfg(test)]` | In `tests/` directory |
| Test internal functions | Test public API only |
| Part of library binary | Separate test binary |
| Fast, isolated | Slower, full system |

Our integration tests use the **public API** (`use netget::*`) just like external users would.

## Test Files

- `ftp_integration_test.rs` - FTP protocol integration tests using real FTP client

## Prerequisites

### Required for Integration Tests

1. **Ollama must be running**:
   ```bash
   ollama serve
   ```

2. **A model must be installed**:
   ```bash
   ollama pull llama3.2:latest
   ```

3. **Available ports**: Tests use ports 21212, 21213, etc. (high ports to avoid conflicts)

## Running Tests

### Unit Tests Only (no Ollama required)

```bash
# Run only unit tests (in src/)
cargo test --lib
```

This runs simple parsing tests that don't require the server or LLM.

### Integration Tests (requires Ollama)

**IMPORTANT**: Integration tests require Ollama to be running!

**Run all tests (unit + integration):**
```bash
# Make sure Ollama is running first!
ollama serve

# In another terminal:
cargo test
```

**Run only integration tests:**
```bash
cargo test --test ftp_integration_test
```

**Run specific integration test:**
```bash
cargo test test_ftp_server_basic_commands
cargo test test_ftp_server_file_retrieval
```

### Without Ollama

If Ollama is not running, integration tests will **fail** with connection errors. This is expected! To run only unit tests:

```bash
cargo test --lib
```

### Parallel Execution

**Note**: Use `--test-threads=1` to avoid port conflicts:

```bash
cargo test -- --test-threads=1
```

## Test Descriptions

### `test_user_command_parsing` (Unit Test)
- **Location**: `tests/ftp_integration_test.rs` (but a unit test)
- **Requires Ollama**: ❌ No
- **Description**: Tests command parsing logic
- **Verifies**:
  - "listen on port 21 via ftp" → `Listen { port: 21, protocol: Ftp }`
  - "listen on port 80 via http" → `Listen { port: 80, protocol: Http }`
  - "close" → `Close`
  - "status" → `Status`
  - "model deepseek-coder:latest" → `ChangeModel { model: "..." }`

### `test_ftp_server_basic_commands` (Integration Test)
- **Location**: `tests/ftp_integration_test.rs`
- **Requires Ollama**: ✅ Yes
- **Port**: 21212
- **Description**: Tests basic FTP protocol with real FTP client
- **Verifies**:
  - Server starts and binds to port
  - TCP listener accepts connections
  - FTP client can connect
  - LLM generates FTP welcome message
  - LOGIN command works (USER/PASS)
  - PWD command works
  - TYPE command works
  - Clean connection teardown

### `test_ftp_server_file_retrieval` (Integration Test)
- **Location**: `tests/ftp_integration_test.rs`
- **Requires Ollama**: ✅ Yes
- **Port**: 21213
- **Description**: Tests file serving via FTP protocol
- **Verifies**:
  - Server serves configured file (data.txt with content "hello")
  - LIST command returns file listing
  - LLM generates proper file listing format
  - File appears in directory listing with correct name

## How Tests Work

### 1. Server Setup

```rust
let state = AppState::new();
state.set_mode(Mode::Server).await;
state.set_protocol_type(ProtocolType::Ftp).await;
state.add_instruction("Serve file data.txt with content: hello").await;
```

The test configures NetGet to:
- Run as FTP server
- Serve a file named `data.txt`
- File content is "hello"

### 2. Server Start

```rust
let mut tcp_server = TcpServer::new(network_tx);
tcp_server.listen("127.0.0.1:21212").await;
```

TCP server binds to localhost on test port.

### 3. LLM Event Processing

When the FTP client sends commands:
1. NetGet receives the data
2. Sends it to Ollama LLM
3. LLM generates appropriate FTP response (e.g., "220 FTP Server Ready")
4. NetGet sends response back to client

### 4. FTP Client Verification

```rust
let mut ftp_stream = suppaftp::FtpStream::connect("127.0.0.1:21212")?;
ftp_stream.login("anonymous", "test@example.com")?;
let path = ftp_stream.pwd()?;
```

Uses `suppaftp` crate to connect as a real FTP client and verify responses.

## Troubleshooting

### "Failed to connect to Ollama"

```
Error: Failed to connect to Ollama at http://localhost:11434
```

**Solution**: Start Ollama:
```bash
ollama serve
```

### "Model not found"

```
Error: Model llama3.2:latest not found
```

**Solution**: Pull the model:
```bash
ollama pull llama3.2:latest
```

### "Address already in use"

```
Error: Address already in use (os error 48)
```

**Solution**:
1. Another test is still running (use `--test-threads=1`)
2. Port conflict with another process:
   ```bash
   lsof -i :21212
   kill -9 <PID>
   ```

### Tests timeout or hang

**Possible causes**:
- Ollama is slow (first request can take 10+ seconds)
- Model is too large
- Network latency

**Solution**:
- Use a smaller/faster model
- Increase test timeout in code
- Check Ollama logs: `journalctl -u ollama -f`

### LLM generates incorrect responses

**This is expected behavior!** The LLM might not perfectly implement FTP protocol.

Tests are designed to be tolerant of LLM variations:
- They test basic connectivity, not perfect protocol compliance
- Real-world use would require prompt engineering for production use

## Adding New Tests

### Example: HTTP Integration Test

```rust
#[tokio::test]
#[ignore]
async fn test_http_server() {
    let state = AppState::new();
    state.set_mode(Mode::Server).await;
    state.set_protocol_type(ProtocolType::Http).await;
    state.add_instruction("Serve HTML with 'Hello World'").await;

    // Start server on port 8080...

    // Use HTTP client
    let response = reqwest::get("http://127.0.0.1:8080").await?;
    assert!(response.status().is_success());
    let body = response.text().await?;
    assert!(body.contains("Hello World"));
}
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2

      - name: Install Ollama
        run: |
          curl https://ollama.ai/install.sh | sh
          ollama serve &
          sleep 5

      - name: Pull Model
        run: ollama pull llama3.2:latest

      - name: Run Unit Tests
        run: cargo test

      - name: Run Integration Tests
        run: cargo test -- --ignored --test-threads=1
```

## Performance Notes

- **Integration tests are slow**: LLM calls take 1-10 seconds each
- **First test is slowest**: Ollama loads model on first request
- **Use smaller models for tests**: `ollama pull llama3.2:1b` for faster tests
- **Mock LLM for unit tests**: Consider mocking LLM for faster unit testing

## Future Improvements

- [ ] Mock LLM client for fast unit tests
- [ ] Test helpers for common setup
- [ ] Snapshot testing for LLM responses
- [ ] Performance benchmarks
- [ ] Parallel test execution (with port management)
- [ ] Docker container for consistent test environment
