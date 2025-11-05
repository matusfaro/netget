# E2E Test Runner

A comprehensive script for running NetGet's end-to-end tests in isolation with proper feature gating.

## Quick Start

```bash
# List all available E2E test features
./e2e-test.sh

# Run all E2E tests (caution: may take a long time!)
./e2e-test.sh all

# Run specific protocol tests
./e2e-test.sh whois dns http

# Dry-run to see what would be executed
./e2e-test.sh --dry-run tor wireguard

# Verbose output for debugging
./e2e-test.sh --verbose whois
```

## Features

### Smart Feature Detection
- Automatically discovers E2E tests from `tests/server/*/e2e_test.rs`
- Extracts feature gates directly from test files
- Handles multi-feature tests (e.g., `torrent_integration` requires `torrent-tracker`, `torrent-dht`, and `torrent-peer`)

### Validation
- Validates all feature flags exist in `Cargo.toml` before running tests
- Fails fast if invalid features are specified
- Checks that E2E test files exist for requested features

### Build Isolation
- Uses `cargo-isolated.sh` for session-specific build directories
- Prevents conflicts when running multiple test sessions concurrently
- Automatic cleanup of old session directories

### Ollama Concurrency Safety
- Automatically sets `OLLAMA_LOCK_PATH` for serialized LLM API calls
- Safe to run multiple E2E test instances concurrently

## Options

- `--verbose`, `-v`: Show detailed test output (passes `--nocapture --test-threads=1` to cargo)
- `--dry-run`, `-n`: Show what would be executed without actually running tests
- `--help`, `-h`: Display help message

## Special Arguments

- *No arguments*: Lists all available E2E test features (default behavior)
- `all`: Runs all E2E tests with valid feature flags

## Examples

### List Available Features (Default)
```bash
./e2e-test.sh
```
Shows all features with checkmarks for available features and their associated test directories.

### Run All E2E Tests
```bash
./e2e-test.sh all
```
Runs all E2E tests with valid feature flags (excludes features without Cargo.toml entries).

### Test a Single Protocol
```bash
./e2e-test.sh whois
```

### Test Multiple Protocols
```bash
./e2e-test.sh whois dns tor
```

### Test with Verbose Output
```bash
./e2e-test.sh --verbose http
```

### Preview Test Execution
```bash
./e2e-test.sh --dry-run whois tor
```
Output:
```
Testing feature: whois
Test files: whois
Features enabled: whois
[DRY RUN] Would execute: ./cargo-isolated.sh test --no-default-features --features whois
```

### Preview All Tests
```bash
./e2e-test.sh --dry-run all
```
Shows what would be executed for all valid E2E tests without actually running them.

## Special Cases

### Multi-Feature Tests

Some tests require multiple features to be enabled:

- **`torrent_integration`**: Requires `torrent-tracker`, `torrent-dht`, and `torrent-peer`
- When you specify any one of these features, the script automatically enables all required features

Example:
```bash
./e2e-test.sh torrent-tracker
```
Actually runs:
```bash
./cargo-isolated.sh test --no-default-features --features torrent-dht,torrent-peer,torrent-tracker
```

### Shared Test Directories

Some features share test directories:

- **`tor`**: Used by `tor_integration` and `tor_relay` tests
- When you run `./e2e-test.sh tor`, both test files are executed

## Exit Codes

- `0`: All tests passed
- `1`: One or more tests failed or validation error occurred

## Integration with CI

The script is designed to work well in CI environments:

```bash
# Run specific protocol tests in CI
./e2e-test.sh whois dns http || exit 1

# Run all tests (may take significant time)
./e2e-test.sh all || exit 1
```

## Troubleshooting

### "Feature does not have feature gate in Cargo.toml"

The feature flag doesn't exist in `Cargo.toml`. Run without arguments to see available features:
```bash
./e2e-test.sh
```

### "No runnable tests found for protocol"

The test requires additional features that aren't available. Check the test file's `#[cfg(...)]` attributes.

### Tests Hanging

Make sure Ollama is running and accessible at `http://localhost:11434`:
```bash
curl http://localhost:11434/api/tags
```

### Build Conflicts

If you're running multiple test sessions concurrently, make sure to use separate terminal sessions (different shell PIDs). The script uses `$$` to isolate build directories.

## Implementation Details

### How It Works

1. **Discovery**: Scans `tests/server/*/e2e_test.rs` files
2. **Feature Extraction**: Parses `#[cfg(feature = "...")]` attributes from test files
3. **Validation**: Checks all features exist in `Cargo.toml`
4. **Feature Resolution**: For multi-feature tests, collects all required features
5. **Execution**: Runs `cargo-isolated.sh test --no-default-features --features <list>`
6. **Summary**: Reports passed/failed tests with exit code

### Directory Structure
```
tests/server/
├── whois/
│   └── e2e_test.rs        # #[cfg(feature = "whois")]
├── tor_integration/
│   └── e2e_test.rs        # #[cfg(feature = "tor")]
├── tor_relay/
│   └── e2e_test.rs        # #[cfg(feature = "tor")]
└── torrent_integration/
    └── e2e_test.rs        # #[cfg(all(feature = "torrent-tracker", feature = "torrent-dht", feature = "torrent-peer"))]
```

## Best Practices

1. **Always specify features**: Avoid running all tests unless necessary
2. **Use dry-run first**: Preview what will be executed
3. **Run related protocols together**: Group protocols by dependency (e.g., `dns dot doh`)
4. **Use verbose mode for debugging**: Helps identify LLM-related issues
5. **Keep LLM calls < 10**: Follow the efficiency guidelines in CLAUDE.md

## Related Files

- `cargo-isolated.sh`: Build isolation wrapper
- `cargo-isolated-kill.sh`: Kill isolated build processes
- `CLAUDE.md`: Project documentation with testing philosophy
- `TEST_STATUS_REPORT.md`: Current test status and known issues
