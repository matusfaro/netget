# Kafka Client E2E Tests

## Status: all four tests are `#[ignore]`d, and none of them can run

**The Kafka client is not compiled into any build.** `src/client/mod.rs` gates it on
`#[cfg(all(feature = "kafka", feature = "rdkafka"))]`, and nothing enables `rdkafka`:
`Cargo.toml` declares it as an optional dependency that no feature depends on, annotated
`# rdkafka removed - causes malloc crashes`, and lists it among the features excluded from
`all-protocols`. `client_registry` therefore never registers it, and `open_client` with
`protocol: "Kafka"` fails at runtime with

> Client protocol 'Kafka' exists but is not compiled into this build (rebuild with --features kafka)

— a message that is itself misleading, since `--features kafka` is exactly what was used.

Measured before this pass: `cargo test --no-default-features --features kafka --test client --
kafka` gave **1 passed, 3 failed**. The one that passed, `test_kafka_client_protocol_detection`,
asserted only `client.protocol == "Kafka"` — a string the harness parses out of a log line
NetGet prints *before* the connect fails. It passed with the client entirely non-functional.

A second, independent blocker applies to the two consumer tests: NetGet's broker implements
ApiVersions, Metadata, Produce, Fetch and OffsetCommit only (`src/server/kafka/CLAUDE.md`).
Consumer groups need FindCoordinator, JoinGroup, SyncGroup and Heartbeat, none of which exist,
so an rdkafka `StreamConsumer` with a `group_id` cannot join. Those tests need manual partition
assignment in `src/client/kafka/` before they can pass.

Both fixes are outside this directory. Re-enabling the tests is then a matter of deleting the
`#[ignore]` attributes.

## Test shapes (written for the day the client returns)

| Test | Assertion that makes it able to fail | LLM calls |
|---|---|---|
| `test_kafka_producer_send_message` | broker's `kafka_produce_request` fires once with `topic` = `test-events` | 4 |
| `test_kafka_consumer_subscribe` | broker's `kafka_fetch_request` fires — a subscribed consumer polls | 4 |
| `test_kafka_producer_consumer_flow` | broker sees the produce; consumer's `kafka_message_received` carries `Test Flow` | 6 |
| `test_kafka_client_protocol_detection` | broker's `kafka_metadata_request` fires — the client actually reached it | 3 |

## Mock rot that was fixed along the way

- **`wait_for_more` is not a Kafka client action.** Every `kafka_connected` rule returned it.
  The client's vocabulary is `produce_message`, `subscribe_topics`, `commit_offset` and
  `disconnect` (`src/client/kafka/actions.rs`), and `call_llm_for_client` offers the model only
  `get_async_actions`, with no common actions mixed in. The mocks would have passed
  `mock_action_names` silently *only because the client is not compiled* — the catalog for an
  event no compiled protocol declares is empty and the check skips. They will panic the moment
  `rdkafka` is enabled.
- **Nothing was asserted on the broker side.** The server mock only answered the startup
  instruction, so no rule depended on a byte arriving. Every test's real assertion was
  `output_contains("Kafka")`.
- `let mut server` / `let mut client` on bindings that are never mutated (`verify_mocks` takes
  `&self`, `stop` takes `self`) produced six `unused_mut` warnings.

## Storage caveat for the flow test

The broker keeps no storage — the root CLAUDE.md forbids protocols from implementing it — so a
produced record is **not** replayed to a later fetch. The `fetch_response` in
`test_kafka_producer_consumer_flow` restates the payload; the producer-side assertion is the
`kafka_produce_request` arriving, not the broker relaying anything.

## Running

```bash
CARGO_TARGET_DIR=/tmp/clients-target cargo test --no-default-features --features kafka \
    --test client -- --test-threads=100 kafka          # 4 ignored
CARGO_TARGET_DIR=/tmp/clients-target cargo test --no-default-features --features kafka \
    --test client -- --test-threads=100 --ignored kafka # will fail until rdkafka is back
```
