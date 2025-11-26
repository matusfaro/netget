# FTP Server E2E Tests

## Test Strategy

Black-box testing using raw TCP connections to verify FTP protocol responses.

## LLM Call Budget

- **Target**: < 5 LLM calls per test
- **Current**: 1 LLM call per test (server setup only)

## Test Cases

| Test | Description | LLM Calls | Expected Runtime |
|------|-------------|-----------|------------------|
| `test_ftp_greeting` | Verify 220 greeting on connect | 1 | ~2s |
| `test_ftp_user_pass` | Verify USER/PASS authentication flow | 1 | ~3s |
| `test_ftp_pwd_quit` | Verify PWD and QUIT commands | 1 | ~3s |

## Mock Configuration

All tests use mock LLM responses via `.with_mock()` builder:
- No actual Ollama required for CI
- Deterministic responses for predictable testing
- Mock expectations verified with `.verify_mocks().await?`

## Running Tests

```bash
# Run with mocks (default, no Ollama needed)
./test-e2e.sh ftp

# Run with real Ollama
./test-e2e.sh --use-ollama ftp

# Run with cargo
./cargo-isolated.sh test --no-default-features --features ftp --test server::ftp::test
```

## Known Issues

1. **Control Channel Only**: Tests only verify FTP control channel responses
2. **No Data Transfer Tests**: LIST/RETR/STOR data transfer not tested (no data channel)

## FTP Response Codes Tested

- 220: Service ready (greeting)
- 221: Service closing (QUIT)
- 230: User logged in (after PASS)
- 257: Pathname created/current directory (PWD)
- 331: User name okay, need password (USER)
