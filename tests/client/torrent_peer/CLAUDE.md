# tests/client/torrent_peer

## Strategy

Codec-level, no LLM, no sockets. Every assertion is on the bytes BEP 3 specifies, fed through
the protocol's own `execute_action`, so the test fails if the framing changes rather than if the
implementation is merely refactored.

## What it pins

`get_async_actions()` advertises `peer_interested`, `peer_not_interested`, `peer_request_piece`
and `peer_send_piece`. Before August 2026 `execute_action` had an arm for none of them: it
accepted only `peer_message`, a name declared nowhere, so a model that called the tool it was
shown got `Unknown Peer client action`, and one that copied the action's `example` — which said
`{"type": "peer_message", ...}`, contradicting the action's own name — worked by accident. The
guard against the general shape is
`tests/event_action_declarations_test.rs::every_advertised_client_action_is_accepted_by_its_own_executor`;
this suite pins that the *bytes* are right, not just that the name is accepted.

- `interested` / `not interested` are message ids 2 and 3 with an empty payload.
- `request` (6) payload is `<index><begin><length>`, three big-endian u32.
- `piece` (7) payload is `<index><begin>` then the raw block.
- The raw `peer_message` shape still works, for the message ids that have no named action
  (have, bitfield, cancel).
- A `peer_request_piece` with no parameters is refused *by name*, not silently truncated.

## LLM call budget

Zero. No mock Ollama, no `verify_mocks()` — nothing here goes near the LLM path.
