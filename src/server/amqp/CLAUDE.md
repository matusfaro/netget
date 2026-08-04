# AMQP Server Implementation

## Status: INCOMPLETE — hidden from the LLM

`DevelopmentState::Incomplete` (`src/server/amqp/actions.rs`), so `is_available_to_llm()`
returns false and the model is never offered this protocol. It was previously
`Experimental`, which put a broker in the model's menu that could not answer a single
AMQP method.

**This is not a broker.** It accepts TCP, fails the handshake, and discards everything
after that.

## What the code actually does

`src/server/amqp/mod.rs`:

1. `spawn_with_llm_actions` binds the port (bind failure propagates with `?`) and
   registers the accept-loop `JoinHandle` exactly once via `register_server_task`.
2. Per connection: reads the 8-byte `AMQP\0\0\x09\x01` protocol header and rejects
   anything not starting with `AMQP`.
3. Writes `create_connection_start_frame()` — **malformed**. The frame header declares
   a 20-byte payload; 31 payload bytes follow. A conforming client reads 20 bytes and
   then requires the `0xCE` frame-end marker, but finds `0x49` (`'I'`, from the middle
   of `"PLAIN"`), and aborts with a framing error. The payload is not valid method
   encoding either: AMQP 0.9.1 §4.2.5 gives version-major and version-minor one octet
   each (the code writes five bytes), and server-properties must be a field table.
4. Reads subsequent frames in a spawned task, logs `type`/`channel`/`size`, and drops
   the payload. Method frames (type 1) are logged as "Received AMQP method frame" and
   nothing else happens.
5. Heartbeat frames (type 8) are echoed back. That is the only correct behaviour in
   the module.

## What does not exist

- No method decoder or encoder. Connection.StartOk, Connection.Tune(Ok),
  Connection.Open(Ok), Channel.Open(Ok), Exchange.Declare, Queue.Declare, Queue.Bind,
  Basic.Publish, Basic.Consume, Basic.Get, Basic.Ack/Nack/Reject: none are parsed or
  produced.
- No field-table codec, which every one of the above depends on.
- No content header (type 2) or content body (type 3) handling — the two frame types
  that carry an actual message are logged and dropped.
- No queues, exchanges, bindings, consumers or deliveries. There is nothing to route.
- No TLS, no SASL beyond advertising the string `PLAIN`, no transactions, no publisher
  confirms, no QoS/prefetch, no clustering.

## No LLM integration

`spawn_with_llm_actions` takes the `OllamaClient` as `_llm_client` and drops it. No
`Event` is ever constructed and `call_llm` is never called, so:

- the server `instruction` has no effect on any byte on the wire;
- `event_handlers` — script or static — never fire, because handler dispatch happens
  inside `call_llm`;
- `get_sync_actions()`, `get_async_actions()` and `get_event_types()` all return empty
  vectors, and `execute_action` returns an error for every input.

Correlation identifiers (channel number, delivery tag, consumer tag) are therefore not
propagated anywhere: no reply is ever generated that could carry them. The frame
reader does parse the channel number out of the header, but only to log it.

## Storage

None — no queue, exchange or message is held in Rust, which satisfies the project rule
by accident rather than by design, since no message ever gets far enough to be stored.

## Fixed while auditing

Frame payloads were allocated straight from the wire-supplied 32-bit size
(`vec![0u8; size as usize]`), letting one peer request a 4 GiB allocation per frame.
Frames are now capped at `MAX_FRAME_SIZE` (1 MiB, versus RabbitMQ's 128 KiB default)
and the connection is closed when a larger one is announced.

## Route to Experimental

1. Write an AMQP 0.9.1 method codec: class/method ids, short/long strings, and the
   field-table encoding. This is the whole job; everything else is small.
2. Complete the connection handshake: Connection.Start/StartOk → Tune/TuneOk →
   Open/OpenOk → Channel.Open/OpenOk. Verify with `lapin` — the client used by
   `tests/server/amqp/e2e_test.rs`, which today only checks that a TCP connection can
   be opened.
3. Surface the decision points as events (`amqp_queue_declare`, `amqp_basic_publish`,
   `amqp_basic_consume`) with matching actions, echoing channel number, delivery tag
   and consumer tag so a `{{event.*}}` static handler can reply without a model call.
4. Keep the "no storage" rule: the model decides what a queue contains and what a
   consumer receives.

## References

- [AMQP 0.9.1 Specification](https://www.rabbitmq.com/resources/specs/amqp0-9-1.pdf)
- [AMQP 0.9.1 protocol reference](https://www.rabbitmq.com/amqp-0-9-1-reference.html)
- Testing notes: `tests/server/amqp/CLAUDE.md`
