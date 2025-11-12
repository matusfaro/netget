# NFC Server E2E Testing

## Test Strategy

**Virtual Server Only**: Tests simulate NFC tag behavior without physical hardware.

### No Hardware Required

Since the NFC server is a virtual/simulation server (most PC/SC readers cannot emulate cards), these tests focus on:
- Virtual tag initialization
- LLM interaction for tag configuration
- Action parsing and execution
- NDEF message construction

### LLM Call Budget

- **Target**: < 5 LLM calls per test suite
- **Strategy**:
  - Simple initialization test
  - Tag configuration test
  - No complex interactions needed (virtual only)

### Test Coverage

1. **Server Initialization**: Virtual tag starts successfully
2. **ATR Configuration**: Set Answer to Reset
3. **NDEF Configuration**: Set NDEF message content
4. **Action Execution**: Test action parsing

### Runtime

- **Expected**: 10-20 seconds
- **No hardware needed**: Can run in CI

### Known Issues

- Server is virtual only - no actual RF communication
- Cannot test with real NFC readers
- Primarily for code coverage and LLM integration testing

### Test Execution

```bash
# Run NFC server tests
./cargo-isolated.sh test --no-default-features --features nfc --test server::nfc::e2e_test
```

### Future Improvements

- Integration with vsmartcard for real emulation testing
- Android HCE bridge testing
- More comprehensive APDU response simulation
