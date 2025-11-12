# NFC Client E2E Testing

## Test Strategy

**Hardware-Dependent**: These tests require physical NFC hardware.

### Required Hardware

- **NFC Reader**: ACR122U or compatible PC/SC reader
- **Test Tags**: NTAG213, MIFARE Ultralight, or ISO14443 test cards
- **System**: Linux (pcscd), Windows, or macOS

### LLM Call Budget

- **Target**: < 10 LLM calls per test suite
- **Strategy**:
  - Reuse reader connection across tests
  - Use scripting mode for predictable APDU sequences
  - Minimal LLM calls per test case

### Test Coverage

1. **Reader Enumeration**: List available PC/SC readers
2. **Card Detection**: Detect and connect to NFC tag
3. **APDU Commands**: Send SELECT and READ BINARY
4. **Error Handling**: Handle missing readers, no card, etc.

### Runtime

- **Expected**: 30-60 seconds (with hardware)
- **Without Hardware**: Tests marked `#[ignore]` - skip in CI

### Known Issues

- Tests require physical hardware - cannot run in CI
- Reader availability varies by platform
- Test tags must be present for full coverage

### Test Execution

```bash
# With NFC hardware
./cargo-isolated.sh test --no-default-features --features nfc-client --test client::nfc::e2e_test

# Without hardware (will fail/skip)
# Mark tests as #[ignore] and run with --ignored flag when hardware available
```

### Future Improvements

- Mock PC/SC context for unit testing
- Simulated readers for CI
- Test with multiple tag types
