# Kafka Protocol Implementation

## Status: INCOMPLETE — hidden from the LLM

`DevelopmentState::Incomplete` (`src/server/kafka/actions.rs`), so `is_available_to_llm()`
returns false. It was previously `Experimental` with `llm_control("Message routing, topic
management, consumer offsets")` — a claim with no code behind it.

**No real Kafka client can use this broker, and the LLM controls none of it.**

## Why it is Incomplete

### 1. No client gets past the first request

`handle_api_versions` (`mod.rs`) returns `ApiVersionsResponse::default()`, whose
`api_keys` list is empty. ApiVersions is the first thing every Kafka client sends; an
empty list means "supports nothing", and the client aborts. `tests/server/kafka/e2e_test.rs`
documents the symptom in its header comment: rdkafka crashes against this server, so all
three tests assert only that `TcpStream::connect` succeeds — they would pass against `nc -l`.

### 2. The API version is ignored everywhere

Every `decode`/`encode` call passes a hardcoded `0`:

- `RequestHeader::decode(&mut cursor, 0)` — five call sites
- `MetadataRequest`/`ProduceRequest`/`FetchRequest`/`OffsetCommitRequest::decode(..., 0)`
- every response `encode(&mut buf, 0)`

`header.request_api_version` is never read. Request header **v0 does not carry
`client_id`** (`kafka-protocol` gates it on `version >= 1`), but every real client sends
v1 or v2. So the cursor is left sitting on the `client_id` length prefix and each request
body is parsed starting inside that string: topic names, partition indices and offsets are
all garbage. Responses are encoded at v0 no matter what the client negotiated, so a client
that asked for, say, Metadata v12 (flexible encoding, tagged fields, topic UUIDs) receives
a v0 body and desynchronises.

Knock-on effect: `MetadataResponse::encode` gates `cluster_id` on `version >= 2` and
`controller_id` on `version >= 1`, so the `cluster_id` startup parameter never reaches the
wire at all.

### 3. The LLM is never called

`handle_metadata`, `handle_produce`, `handle_fetch` and `handle_offset_commit` all take
`_llm_client`, `_app_state`, `_server_id` and `_protocol` and use none of them. There is no
`call_llm`, no `Event::new`, and no read of `ExecutionResult::protocol_results` anywhere in
the module. Therefore:

- the server `instruction` has no effect on any byte on the wire;
- **`event_handlers` are silently inert** — script and static dispatch happens inside
  `call_llm`, which is never reached. A user configures a handler, gets no error, and gets
  hardcoded behaviour. `get_startup_examples` still advertises exactly this;
- all nine declared actions (`produce_response`, `fetch_response`, `metadata_response`,
  `offset_commit_response`, `error_response`, `publish_message`, `create_topic`,
  `delete_topic`, `set_retention`) are dead: `execute_action` builds an
  `ActionResult::Custom` that nothing consumes, so **every documented field of every action
  is ignored**. The action `log_template` is still rendered on success, so the TUI prints
  `-> Kafka produce OK offset=42` while the client receives an offset computed elsewhere;
- all four declared event types (`kafka_produce_request`, `kafka_fetch_request`,
  `kafka_metadata_request`, `kafka_offset_commit_request`) are never constructed. Their
  documented parameters, including `first_value_preview` (`required: true`), cannot be
  produced.

### 4. It stores broker state in Rust

Against the project's no-storage rule:

```rust
topics: Arc<RwLock<HashMap<String, Vec<Vec<KafkaRecord>>>>>,
consumer_offsets: Arc<RwLock<HashMap<String, HashMap<String, HashMap<i32, i64>>>>>,
```

Topics are auto-created on produce, records are pushed and never evicted, and consumer
group offsets accumulate — all keyed by unauthenticated network input. `log_retention_hours`
and `auto_create_topics` are both parsed, stored as `_`-prefixed fields, and never read:
setting `auto_create_topics: false` does nothing, because topics are created
unconditionally.

This is left in place rather than half-removed: deleting it without the LLM path would
leave a broker that answers nothing at all. It is step 2 of the work below.

## Correlation identifiers

`correlation_id` is the one thing this module gets right. It is read from the request
header and echoed into every response header — metadata, produce, fetch, offset-commit and
the error path. It is not exposed to any handler, because no event is emitted.

`api_key` is used for dispatch and correctly not echoed (Kafka responses do not carry it).
`api_version` is the broken one — see above.

## Crash and DoS defects fixed while auditing

These were live regardless of the maturity label, since the port can still be opened
explicitly:

- **Remote panic from four bytes.** A size prefix of `00 00 00 00` made
  `buffer.resize(0, 0)` shrink the read buffer permanently; the next loop iteration
  indexed `buffer[..4]` and panicked with "range end index 4 out of range for slice of
  length 0". Sizes 1-3 did the same. The panic sat inside a detached `tokio::spawn`, so it
  was swallowed while the connection stayed `Active` and the server stayed `Running`.
  Fixed: the prefix is read into a fixed `[u8; 4]` with `read_exact`, and the buffer only
  ever grows.
- **Remote OOM / process abort.** `i32::from_be_bytes(...) as usize` sign-extends, so a
  prefix of `80 00 00 00` became ~1.8e19 and aborted on `Vec::resize`; `7F FF FF FF`
  zeroed 2 GiB per connection. Fixed: sizes are validated against `MIN_REQUEST_BYTES`
  (8, the header minimum) and `MAX_REQUEST_BYTES` (100 MiB, matching Kafka's
  `socket.request.max.bytes`) before any allocation.
- **Unbounded loop on a wire-supplied partition index.** `while partitions.len() <=
  partition_idx as usize` with `partition_idx = -1` (`usize::MAX` after the cast) pushed
  `Vec::new()` until the allocator aborted; `2_000_000_000` is a legal i32 asking for
  ~48 GB. Fixed: indices outside `0..=MAX_PARTITIONS` (1024) are rejected with error code
  3 (UNKNOWN_TOPIC_OR_PARTITION).
- **Panic from a startup parameter.** `default_partitions` comes from LLM or MCP JSON and
  reached `vec![Vec::new(); n as usize]`; `-1` aborted the process. Fixed: clamped to
  `1..=MAX_PARTITIONS`.
- **TRACE hex amplification.** `hex::encode` doubles the payload, so a maximum-size
  request built a 200 MiB `String` on an unbounded status channel. Fixed: capped at
  `MAX_TRACE_HEX_BYTES` (4 KiB).
- **Short read on the size prefix.** `read` (not `read_exact`) can return 1-3 bytes; all
  four were parsed anyway, mixing in stale bytes. Fixed.
- **Hot accept loop.** An accept error logged and continued with no backoff or break, so a
  persistent EMFILE saturated a core and flooded the unbounded status channel. Fixed: the
  loop breaks.
- **Connections never removed.** Entries were added to server state and never marked
  closed, so the TUI accumulated `Active` connections for dead sockets. Fixed in the accept
  loop's spawn wrapper.

## Known defects still open

- Unsupported APIs get `create_error_response`, which writes a bare `i16` error code as
  the entire response body. That is not a valid body for any Kafka API — most begin with
  `throttle_time_ms` (i32) — so the client misparses it. The `encode` error is also
  discarded with `let _ =`.
- A record batch that fails to parse is replaced by an **empty placeholder record** and the
  producer is told `error_code(0)` — success. This path is hit by *any* compressed batch
  (`decode_with_custom_compression` is passed `None`, so gzip/snappy/lz4/zstd all fail) and
  by any CRC mismatch. It should return `CORRUPT_MESSAGE` (2).
- Per-connection byte and packet counters are never updated, so connection stats read zero.

## Route to Experimental

1. Decode the request header at the version implied by `(api_key, api_version)`, pass
   `header.request_api_version` to every body decode and response encode, and populate
   `ApiVersionsResponse.api_keys` with the ranges actually supported. Verify with a real
   rdkafka client, not a TCP connect.
2. Delete `topics` and `consumer_offsets` and the hardcoded response logic. Emit the four
   already-declared events via `call_llm` and consume `ActionResult::Custom` in `mod.rs`
   for the nine already-declared actions. This one change fixes LLM integration, action
   liveness and the storage violation together.
3. Fix `create_error_response` to emit a valid body per API, and return a real error code
   for unparseable record batches.
4. Rewrite `tests/server/kafka/e2e_test.rs` to exercise Kafka bytes and to call
   `server.verify_mocks().await?`, which it currently never does.

## Startup Parameters

Parsed by `spawn_with_llm_actions`:

- `cluster_id` (string, default `"netget-kafka-1"`) — stored; never reaches the wire,
  because `MetadataResponse` v0 does not encode it
- `broker_id` (number, default 0) — used in metadata responses
- `auto_create_topics` (boolean, default true) — **parsed and never read**; topics are
  auto-created unconditionally
- `default_partitions` (number, default 1) — used, now clamped to `1..=1024`
- `log_retention_hours` (number, default 168) — **parsed and never read**; no retention
  logic exists

## References

- [Apache Kafka Protocol Guide](https://kafka.apache.org/protocol)
- [kafka-protocol Rust Crate](https://docs.rs/kafka-protocol/) — `Cargo.toml` pins 0.14
- [Kafka Error Codes](https://kafka.apache.org/protocol.html#protocol_error_codes)
- Testing notes: `tests/server/kafka/CLAUDE.md`
