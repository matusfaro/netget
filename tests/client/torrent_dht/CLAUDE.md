# tests/client/torrent_dht

## Strategy

Codec-level, no LLM, no sockets. Assertions are on the `dht_query` payload `mod.rs` bencodes
into a BEP 5 query, so a wrong `query_type` fails here rather than on a live DHT node.

## What it pins

`get_async_actions()` advertises `dht_ping`, `dht_find_node`, `dht_get_peers` and
`dht_announce_peer`. Before August 2026 `execute_action` had an arm for none of them: it
accepted only `dht_query`, a name declared nowhere, so a model that called the tool it was shown
got `Unknown DHT client action`, and one that copied the action's `example` — which said
`{"type": "dht_query", "query_type": "ping"}`, contradicting the action's own name — worked by
accident. The guard against the general shape is
`tests/event_action_declarations_test.rs::every_advertised_client_action_is_accepted_by_its_own_executor`.

- Each advertised action sets its own BEP 5 `query_type`, and carries `node_id` through.
- An explicitly supplied `query_type` is honoured rather than replaced, and the bare
  `dht_query` shape still works, so callers that learned the old form are unaffected.
- An unknown action is still an error: widening the executor must not make it fail open.

## LLM call budget

Zero. No mock Ollama, no `verify_mocks()` — nothing here goes near the LLM path.
