# OpenVPN E2E tests

## Strategy

Two suites, both of which must be able to fail for the reason they claim to test.

### `wire.rs` — an independently written codec

Byte layout decoded and encoded with explicit offsets, never calling
`netget::server::openvpn::packet`. Both suites build their requests and decode the server's replies through it.

This exists because a test that checks NetGet's parser against NetGet's serializer proves nothing — both can be wrong in
the same way, and that is exactly how this protocol shipped a reset reply with the message packet id written before the
ACK array, which no real client could parse.

It also holds the captured frames. Every literal was taken off the wire from **OpenVPN 2.7.4**
(`aarch64-apple-darwin`, OpenSSL 3.6.2) against a reference responder written outside this repository:

| Constant                      | What it is                                                        |
|-------------------------------|-------------------------------------------------------------------|
| `CAPTURED_CLIENT_RESET_V2`    | The 14-byte `P_CONTROL_HARD_RESET_CLIENT_V2` a real client sends   |
| `CAPTURED_SERVER_RESET_V2`    | The 26-byte reply that client **accepted**                        |
| `CAPTURED_SERVER_ACK_V1`      | The 22-byte `P_ACK_V1` that client **accepted**                   |
| `CAPTURED_CLIENT_CONTROL_V1`  | Head of the `P_CONTROL_V1` carrying its TLS ClientHello           |

Acceptance was observed in the client's own log: `TLS: Initial packet from [AF_INET]127.0.0.1:PORT, sid=...` for the
reset reply, and `UDPv4 READ [22] ... P_ACK_V1 kid=0 [ 1 ] DATA len=0` followed by retransmissions stopping, for the
ACK.

### `codec_test.rs` — wire format, 10 tests, 0 LLM calls

Parses the captured client frames and asserts the decoded fields; emits our reset reply and ACK and asserts they are
**byte-identical** to the frames the real client accepted; round-trips; and feeds hostile input — empty, truncated,
`ACK length 255` with nothing behind it, unknown opcodes, wrong-category opcodes, tls-crypt-v2 — asserting `Err` and no
panic. `no_byte_string_can_panic_either_parser` fuzzes both parsers with 20,000 pseudorandom strings, half of them
starting from a plausible opcode byte.

### `e2e_test.rs` — 4 tests, 4 LLM calls

1. **`test_real_openvpn_client_accepts_our_reset_reply`** — drives the system `openvpn` binary against the server and
   asserts its log contains `TLS: Initial packet from [AF_INET]127.0.0.1:<port>`, a line it emits only after parsing and
   accepting a `P_CONTROL_HARD_RESET_SERVER_V2`. It also asserts the client never logs
   `Initialization Sequence Completed` — the server cannot build a tunnel, and if that ever changes the metadata and
   docs are wrong and this test says so.

   The client runs with `--dev null` (no TUN, no root), `--auth-user-pass` pointing at a throwaway file, and
   `--peer-fingerprint` with a bogus fingerprint. Both are needed only to get past `openvpn`'s config validation;
   neither is ever exercised, because the handshake cannot reach a certificate. The test asserts the client did **not**
   print `Options error`, so a future `openvpn` release that rejects this command line fails the test loudly instead of
   silently testing nothing.

2. **`test_reset_reply_is_spec_correct_and_control_packets_are_acked`** — raw UDP. Asserts the reply is 26 bytes,
   opcode 8, acknowledges the packet id actually sent, echoes the session id actually sent (not a constant), numbers its
   own packet 0, and carries no trailing bytes. Then: a retransmitted reset gets the identical answer; a `P_CONTROL_V1`
   carrying a TLS record gets a 22-byte `P_ACK_V1` with no message packet id; a `P_DATA_V2` gets no reply at all; and
   after being fed five hostile datagrams the server still answers a brand-new peer correctly.

3. **`test_rejected_peer_receives_nothing`** — `reject_peer` is enforced: no bytes at all, including for a
   retransmission.

4. **`test_absent_decision_fails_closed`** — the handler runs and logs but produces neither decision. The peer must
   still receive nothing, and the log must distinguish "no decision" from an explicit refusal. This is the regression
   test for the fail-open pattern that OAuth2 shipped.

## Privileges

**No test requires root.** The server has no TUN device, so there is nothing to elevate for. The old suite asserted
`geteuid() == 0` and failed outright on any normal machine.

## `openvpn` must be installed

The real-client test **fails** rather than skips when the binary is missing. A capability check that returns success
when the capability is absent is worse than no test: it reports coverage that does not exist. Install with
`brew install openvpn` or `apt-get install openvpn`.

## LLM call budget

4 calls total — one server startup per E2E test. Peer decisions use static `event_handlers`, so no model call happens
per peer. The codec suite makes none.

## Running

```bash
# Use your own target dir: target/debug/netget is shared and another agent's
# build will silently break a run in progress.
CARGO_TARGET_DIR=/tmp/ovpn-target cargo test --no-default-features --features openvpn \
    --test server -- --test-threads=100 openvpn
```

Expected: `14 passed; 0 failed`, about 9 seconds.

The first run after a source edit rebuilds and can time out; run twice and use the second result.

## Verifying these tests can fail

The suite was checked against a deliberate regression: `ControlFrame::serialize` was reverted to the old field order
(message packet id before the ACK array). All four E2E tests and the three frame-emitting codec tests failed, including
the real-client test. Re-run that check if you change the codec — a wire-format test that cannot fail is the failure
mode this protocol already had once.

## References

- [OpenVPN protocol overview](https://openvpn.net/community-resources/openvpn-protocol/)
- [NetGet OpenVPN implementation](../../../src/server/openvpn/CLAUDE.md)
