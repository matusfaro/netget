# Modbus TCP E2E Tests

## Strategy

Three layers, deliberately, because each one catches something the others cannot.

1. **A real, independent client.** `tokio-modbus` 0.17 (MIT OR Apache-2.0) is a dev-dependency
   and drives the server in-process. It is a *different* implementation from
   `src/server/modbus/codec.rs`, which is hand-rolled — that asymmetry is what makes the test
   evidence rather than a tautology. If our byte counts, bit packing or exception framing were
   wrong, `tokio-modbus` would reject the response or decode it into the wrong values.

2. **Raw sockets with hand-built, spec-derived frames.** For behaviour that never reaches the
   model — unknown function codes, illegal quantities — and for MBAP fields that a client
   library hides. This is where the transaction-id and unit-id echo are asserted, and where two
   requests are sent in a **single TCP write** to prove the ADU framing loop really frames.

3. **Codec assertions against literal bytes.** `codec::encode_bits_response`,
   `encode_registers_response`, `encode_write_ack`, `parse_request` and `try_parse_adu` are
   checked against byte sequences taken from the Modbus specification's own examples
   (`0x01 0x02 0xCD 0x01` for nine coils, `0x03 0x04 0x02 0x2B 0x00 0x00` for two registers).

**Nothing here asserts that a connection opened or that bytes arrived.** Every assertion is on a
decoded PDU: register values, coil bits, function codes, exception codes, MBAP fields.

## LLM call budget

| Test | Startup | Events | Total |
|---|---|---|---|
| `test_modbus_reads_writes_and_exceptions_against_tokio_modbus` | 1 | 5 | **6** |
| `test_modbus_spec_exceptions_and_mbap_framing` | 1 | 0 | **1** |
| `test_codec_*` (three tests) | 0 | 0 | **0** |
| `peer_inject_test::injected_close_connection_sends_eof_and_counters_move` | 0 | 0 | **0** |

`peer_inject_test.rs` is the dashboard-injection test: a `*` static handler answers one FC 3
read over a raw socket (asserting both byte/packet counters), then `send_to_peer` proves an
injected `send_modbus_write_ack` is `Executed` without writing (request-bound Custom result)
and an injected `close_connection` yields `Disconnected`, EOF on the socket, and the peer
handle being released. It uses no mock LLM at all (`AppState` pointed at a dead port).

**Total: 7**, under the ~10 target. The second test costs one call because starting a server
costs one; the three exception exchanges it performs cost nothing, which is itself the assertion
— those paths are answered from the specification, not the model.

## Mock expectations

Six rules in the first test. Two things to know about them:

- **Order matters.** `and_event_data_contains` is a substring match and rules are tried in
  order, so the *narrower* rule is declared first: the `register_type: input` rule precedes the
  `register_type: holding` one, and the `write_multiple_registers` rule precedes the
  `write_single_register` one.

- **Two rules use `respond_with_actions_from_event`** and derive their answer from the request's
  own `quantity` and `start_address` — the holding-register read returns `1800 + start + i*10`
  per register, the coil read returns a pattern of the requested width. A hardcoded array would
  pass just as well against a correct server, but would keep passing if the server started
  ignoring the requested quantity. Deriving from the event means the assertion
  `vec![1800, 1810]` can only hold if `start_address` and `quantity` really reached the model.

Every test ends with `server.verify_mocks().await?`. Without it the test asserts nothing about
LLM interaction at all.

## Why there is no `.respond_with_actions_from_event()` requirement for the ids

The CLAUDE.md rule about echoing transaction ids dynamically exists because DNS makes the model
supply `query_id`, so a static mock with a hardcoded id causes client timeouts. Modbus here does
**not** do that: `mod.rs` reconstructs the MBAP header from the request it parsed, and no action
parameter carries a transaction id. A static mock is therefore safe, and
`test_modbus_spec_exceptions_and_mbap_framing` asserts the echo directly against literal
transaction ids (`0xBEEF`, `0x0102`) that the mock never sees.

## Client library

`tokio-modbus` 0.17, `default-features = false, features = ["tcp"]` — no serial, no sync
wrapper, so the dependency is small (bytes, futures-core/util, tokio-util, log, thiserror,
byteorder, async-trait, crc, smallvec, most of which are already in the tree).

Its read/write methods return `Result<Result<T, ExceptionCode>, Error>`: the outer error is
transport, the inner is a decoded Modbus exception. Both layers are asserted — a refusal must
arrive as `Ok(Err(ExceptionCode::IllegalDataAddress))`, never as transport failure and never as
data.

## What is covered

- FC 3 read holding registers, values decoded by an independent client
- FC 4 read input registers, refused with exception 0x02
- FC 1 read coils, including the LSB-first bit packing
- FC 6 write single register, accepted (the echo path)
- FC 16 write multiple registers, refused with exception 0x03 supplied by *name*
  (`"illegal_data_value"`), proving the name→code mapping in the executor
- FC 0x08, unimplemented → exception 0x01, with no LLM call
- Quantity 0 and quantity 2001 → exception 0x03, with no LLM call
- MBAP transaction id and unit id echoed verbatim, protocol id 0, length field consistent
- Two ADUs in one TCP segment → two correctly framed responses
- Codec: spec example frames, incomplete frames reported as incomplete (not as an error),
  non-zero protocol id reported as not-Modbus, bit packing, register packing, both write-echo
  shapes, exception encoding, malformed byte counts, out-of-range addresses

## Coverage gaps

- **No real PLC, no `mbpoll`, no `pymodbus`.** The peer is a Rust crate. It is an independent
  implementation, which is the important property, but it is not a field device and it is not a
  second language's stack.
- FC 2 (discrete inputs) and FC 5/15 (coil writes) are exercised through the codec tests and the
  shared event/action paths, but not through the `tokio-modbus` client.
- No test for the `unit_id` startup parameter's 0x0B gateway exception.
- No concurrency test: pipelined requests that are *not* both spec-rejected would each cost an
  LLM call, and the budget does not allow it. The framing loop is covered; the
  `Processing`/`Accumulating` interleaving is not.
- No test that a partial ADU split across two TCP segments is reassembled. The code path is the
  `Ok(None)` branch of `try_parse_adu`, which the codec test covers in isolation.

## Running

```bash
./cargo-isolated.sh test --no-default-features --features modbus \
    --test server -- --test-threads=100 modbus
```

Runtime is about 1 second: every LLM call is mocked in-process, and Modbus round trips on
loopback are sub-millisecond.

## Failure modes seen so far

None. The suite has been stable across repeated runs. The most likely future flake is the
`wait_for_log("Modbus accept loop started", 15)` guard on a heavily loaded machine; the timeout
is generous precisely because the alternative — sleeping a fixed interval — is what makes E2E
suites flaky.
