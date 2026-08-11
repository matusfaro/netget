//! BitTorrent DHT client: the advertised action names must reach the wire encoder.
//!
//! `get_async_actions()` advertises `dht_ping`, `dht_find_node`, `dht_get_peers` and
//! `dht_announce_peer`. Until this was fixed, `execute_action` had no arm for any of them — it
//! accepted only `dht_query`, a name declared nowhere — so a model calling the tool it was
//! shown got `Unknown DHT client action`, and only a model that copied the action's `example`
//! verbatim (which said `{"type": "dht_query", "query_type": "ping"}`, contradicting the
//! action's own name) happened to work.
//!
//! `mod.rs` turns the resulting `Custom { name: "dht_query", .. }` into the BEP 5 bencode
//! `{"t": …, "y": "q", "q": <query_type>, "a": {"id": …}}`, so `query_type` is the field these
//! assertions are about: get it wrong and the DHT node sees the wrong query.

use netget::client::torrent_dht::actions::TorrentDhtClientProtocol;
use netget::llm::actions::client_trait::{Client, ClientActionResult};
use serde_json::json;

fn query(action: serde_json::Value) -> serde_json::Value {
    let protocol = TorrentDhtClientProtocol::new();
    match protocol
        .execute_action(action)
        .expect("the executor must accept the action name it advertises")
    {
        ClientActionResult::Custom { name, data } => {
            assert_eq!(
                name, "dht_query",
                "mod.rs dispatches DHT queries on the `dht_query` custom result"
            );
            data
        }
        other => panic!("expected a dht_query, got {:?}", other),
    }
}

#[test]
fn each_advertised_query_action_sets_its_own_bep5_query_type() {
    for (action_type, expected) in [
        ("dht_ping", "ping"),
        ("dht_find_node", "find_node"),
        ("dht_get_peers", "get_peers"),
        ("dht_announce_peer", "announce_peer"),
    ] {
        let data = query(json!({
            "type": action_type,
            "node_id": "abcdefghij0123456789",
            "target": "mnopqrstuv0123456789",
            "info_hash": "0123456789abcdefghij"
        }));

        assert_eq!(
            data["query_type"], expected,
            "{} must become BEP 5 query type {}",
            action_type, expected
        );
        assert_eq!(
            data["node_id"], "abcdefghij0123456789",
            "the querying node id must survive into the query"
        );
    }
}

/// The raw shape stays accepted, and an explicit `query_type` still wins — so a caller that
/// learned `dht_query` before the named actions existed is unaffected.
#[test]
fn an_explicit_query_type_is_not_overwritten() {
    let data = query(json!({
        "type": "dht_ping",
        "query_type": "find_node",
        "node_id": "abcdefghij0123456789"
    }));

    assert_eq!(
        data["query_type"], "find_node",
        "an explicitly supplied query_type must be honoured rather than replaced"
    );

    let raw = query(json!({
        "type": "dht_query",
        "query_type": "get_peers",
        "node_id": "abcdefghij0123456789"
    }));
    assert_eq!(raw["query_type"], "get_peers");
}

/// Anything else is still an error: widening the executor must not make it fail open.
#[test]
fn an_unknown_dht_action_is_still_refused() {
    let protocol = TorrentDhtClientProtocol::new();
    let err = protocol
        .execute_action(json!({"type": "dht_teleport"}))
        .expect_err("an unknown action must not be accepted");
    assert!(
        err.to_string().contains("dht_teleport"),
        "the error must name the action, got: {}",
        err
    );
}
