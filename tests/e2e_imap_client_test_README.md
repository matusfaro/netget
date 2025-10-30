# IMAP E2E Tests with async-imap Client

This file contains end-to-end tests for the NetGet IMAP implementation using the `async-imap` Rust client library - the same library used by real email clients.

## Overview

These tests complement the raw TCP tests in `tests/e2e/server/imap/test.rs` by using an actual IMAP client implementation (`async-imap`), providing:

- **More realistic testing**: Uses the same library that email clients use
- **Protocol validation**: The client enforces correct IMAP protocol usage
- **Higher-level operations**: Tests common email client workflows
- **Better coverage**: Validates complex multi-command sequences

## Test Coverage

### 10 Comprehensive Tests

1. **`test_imap_login_success`** - Successful authentication
2. **`test_imap_login_failure`** - Authentication rejection
3. **`test_imap_list_mailboxes`** - Mailbox listing
4. **`test_imap_select_mailbox`** - Mailbox selection with EXISTS/RECENT/UNSEEN
5. **`test_imap_fetch_messages`** - Message retrieval (RFC822)
6. **`test_imap_search_messages`** - Search by FROM criteria
7. **`test_imap_capability`** - Capability negotiation
8. **`test_imap_examine_readonly`** - Read-only mailbox access
9. **`test_imap_status_command`** - Mailbox status without selection
10. **`test_imap_concurrent_connections`** - Multiple simultaneous clients
11. **`test_imap_noop_and_logout`** - Connection keep-alive and graceful shutdown

## Prerequisites

### 1. Build NetGet with IMAP Feature

```bash
cargo build --release --all-features
```

This creates the binary at `target/release/netget` that the tests will spawn.

### 2. Install Ollama and Model

```bash
# Install Ollama (if not already installed)
curl -fsSL https://ollama.com/install.sh | sh

# Pull the required model
ollama pull qwen3-coder:30b
```

### 3. Install Dependencies

The test dependencies are automatically installed when running tests:
- `async-imap` - IMAP client library
- `async-native-tls` - TLS support for IMAP connections

## Running the Tests

### Run All IMAP async-imap Tests

```bash
cargo test --features e2e-tests,imap --test e2e_imap_client_test
```

### Run with Parallelization (Faster)

```bash
cargo test --features e2e-tests,imap --test e2e_imap_client_test -- --test-threads=3
```

**Expected runtime**: ~40-60 seconds with 3 threads (vs ~2+ minutes serial)

### Run a Specific Test

```bash
cargo test --features e2e-tests,imap --test e2e_imap_client_test test_imap_login_success
```

### Debug Output

```bash
cargo test --features e2e-tests,imap --test e2e_imap_client_test -- --nocapture
```

## Test Structure

Each test follows this pattern:

```rust
#[tokio::test]
async fn test_imap_example() -> E2EResult<()> {
    // 1. Define the prompt (instructs NetGet's LLM)
    let prompt = "listen on port 0 via imap. ...";

    // 2. Start NetGet server
    let server = start_netget_server(ServerConfig::new(prompt)).await?;

    // 3. Connect with async-imap client
    let client = connect_imap_client(server.port).await?;
    let mut session = client.login("user", "pass").await?;

    // 4. Perform IMAP operations
    let mailbox = session.select("INBOX").await?;

    // 5. Assert expected behavior
    assert!(mailbox.exists.is_some(), "Should have messages");

    // 6. Cleanup
    session.logout().await?;
    server.stop().await?;

    Ok(())
}
```

## Key Features Tested

### Authentication
- ✅ Successful LOGIN with correct credentials
- ✅ Failed LOGIN with wrong credentials
- ✅ CAPABILITY negotiation before and after login

### Mailbox Operations
- ✅ LIST - Enumerate available mailboxes
- ✅ SELECT - Open mailbox for read/write
- ✅ EXAMINE - Open mailbox read-only
- ✅ STATUS - Get mailbox info without selecting

### Message Operations
- ✅ FETCH - Retrieve message content (RFC822)
- ✅ SEARCH - Find messages by criteria (FROM, etc.)

### Connection Management
- ✅ NOOP - Keep connection alive
- ✅ LOGOUT - Graceful disconnect
- ✅ Concurrent connections - Multiple simultaneous clients

## Comparison with Raw TCP Tests

### `tests/e2e/server/imap/test.rs` (Raw TCP)
- **Pros**: Full protocol control, tests low-level details
- **Cons**: Manual protocol implementation, error-prone
- **Use case**: Protocol compliance, edge cases

### `tests/e2e_imap_client_test.rs` (async-imap)
- **Pros**: Real client, validates actual usage, easier to write
- **Cons**: Limited control over protocol details
- **Use case**: End-user workflows, integration validation

Both test suites are complementary and important for comprehensive coverage.

## Troubleshooting

### Test Timeout

If tests timeout after 30 seconds:
- Check Ollama is running: `ollama list`
- Verify model is available: `ollama run qwen3-coder:30b`
- Increase timeout in the test code if needed

### Connection Refused

If tests fail with "connection refused":
- Ensure the release binary was built: `cargo build --release --all-features`
- Check no other process is using the dynamic port
- Look for server startup errors in test output

### LLM Response Issues

If the LLM doesn't generate correct responses:
- Try a different model: `ServerConfig::new(prompt).with_model("llama3.1:70b")`
- Make prompts more explicit about expected behavior
- Check server output with `--nocapture` flag

## Contributing

When adding new IMAP tests:

1. **Follow the existing pattern**: Use `connect_imap_client()` helper
2. **Use realistic prompts**: Describe what an IMAP server should do
3. **Test both success and failure paths**: Verify correct rejections
4. **Document expected behavior**: Add comments explaining the test
5. **Clean up resources**: Always call `server.stop().await?`

## References

- [RFC 3501 - IMAP4rev1](https://tools.ietf.org/html/rfc3501)
- [async-imap crate](https://docs.rs/async-imap/)
- [NetGet IMAP Implementation](../src/server/imap/)
