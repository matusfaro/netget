# BGP Client E2E Testing

## Strategy

A mocked NetGet **server** speaks BGP to a mocked NetGet **client** over a real socket. Every
assertion is a mock rule that fires only if bytes crossed the wire and parsed — never an
`output_contains` check, which is satisfied by a log line saying the connection failed.

| Test | Asserts | LLM calls |
|---|---|---|
| `test_bgp_client_establishes_session_and_receives_routes` | server's `bgp_open` and `bgp_established` each fire once (client's OPEN and KEEPALIVE arrived); client's `bgp_connected` fires once (server's OPEN and KEEPALIVE arrived); client's `bgp_update_received` fires with `nlri` containing `10.0.0.0/24` | 6 |
| `test_bgp_client_sends_four_octet_asn` | server's `bgp_open` fires with `peer_as` = 4200000000, and the session still reaches Established with `peer_supports_four_octet_as` true | 5 |

**Total: 11 LLM calls, ~6s.** Run:

```bash
CARGO_TARGET_DIR=/tmp/clients-target cargo test --no-default-features --features bgp \
    --test client -- --test-threads=100 bgp
```

## Why the assertions are where they are

`and_event_data_contains("nlri", "10.0.0.0/24")` on the client's `bgp_update_received` is the
regression test for the hex-blob event data. The UPDATE used to reach the handler as
`update_data_hex`, so a rule matching on `nlri` would never have fired.

`and_event_data_contains("peer_as", "4200000000")` on the server's `bgp_open` is the regression
test for the 16-bit truncation. `4200000000 & 0xFFFF` is 60416, so a client that puts
`local_as as u16` on the wire makes this rule miss.

**Mutation-checked**: changing the client's OPEN to `wire::build_open(local_as & 0xFFFF, …)`
made `test_bgp_client_sends_four_octet_asn` fail with
`Rule #1 (event=bgp_open, event_data [("peer_as", "4200000000")]): Expected 1 calls, got 0`.

## What these replaced

Three tests that could not pass and could not fail informatively:

1. Every rule mocked `bgp_open_received` and `bgp_keepalive_received`. Neither event exists on
   the BGP server, whose events are `bgp_open`, `bgp_established`, `bgp_update` and
   `bgp_notification`. No rule ever matched.
2. Client rules answered `bgp_connected` with `send_bgp_open`, a *server* action. The BGP
   client cannot execute it, so `tests/helpers/mock_action_names.rs` panics while the test is
   being configured.
3. Every assertion was `output_contains("BGP") || output_contains("connected")`, satisfied by
   a failure message.

## Notes

- The server binds port 0 (`{AVAILABLE_PORT}` in the prompt, `"port": 0` in the action), so
  the `PrivilegedPort(179)` requirement never fires and the tests need no privilege.
- Client startup parameters go in the `open_client` action's `startup_params`, not in the
  prompt: the mock decides them, the model's wording does not.
- Sleeps are 500ms after each server start and 3s after the client, covering
  OPEN → OPEN → KEEPALIVE → KEEPALIVE → UPDATE with an LLM round trip at three steps.

## Not covered

Hold-timer expiry (the client does not enforce one), NOTIFICATION handling, the client's
`disconnect` and `send_notification` actions reaching a peer, multiple simultaneous peers, and
interoperability with a real BGP daemon (none is installed).
