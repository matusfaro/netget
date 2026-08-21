# HTTP Client E2E Tests

## Test Strategy

Unit tests for HTTP client state management. Full integration tests would use httpbin.org or local server.

## LLM Call Budget

**Target:** < 10 calls
**Actual:** 0 calls (unit tests only)

## Tests

1. **test_http_client_initialization** (0 LLM calls)
    - Create HTTP client instance
    - Verify fields

2. **test_http_client_status** (0 LLM calls)
    - Test status transitions
    - Verify state management

## Runtime

**Expected:** < 5 seconds

## Future Tests

- Integration test with httpbin.org
- Test actual HTTP requests with LLM
- Test response parsing

## `command_channel_test.rs`

Covers `AppState::send_to_client` injecting an action into a running http client (the
dashboard's `[ send ]`). **Zero LLM calls**: the client's LLM points at
`http://127.0.0.1:1`, so its connected-event call fails and the loop must tolerate that —
part of what the test verifies. It always `wait_for_client_handle`s before sending, which is
the regression guard for "register the command channel *before* the connected-event LLM
call"; register it after and a client whose connect event parks on a manual rule reads "no
command channel" for the whole park.

Asserts the exact `ClientSendOutcome` variant. A successful request is
`Executed { detail }`, **not** `Sent` — reqwest/h3 own the socket and report no wire byte
count, so a byte count would be invented; the detail carries what actually came back
instead. An unknown action must be `Rejected` (not silently swallowed), and `disconnect`
must be `Disconnected` and leave the client with no command handle.

The peer is a NetGet HTTP server of our own with a `*` static handler, so the assertion that
the injected `GET /dashboard-marker` came back `200` is a real round trip, and the server's
access log is checked for the path.
