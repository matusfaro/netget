//! BitTorrent Peer Wire client: the advertised action names must produce the BEP 3 bytes.
//!
//! `get_async_actions()` advertises `peer_interested`, `peer_not_interested`,
//! `peer_request_piece` and `peer_send_piece`. Until this was fixed, `execute_action` had no
//! arm for any of them — it accepted only `peer_message`, a name declared nowhere — so a model
//! that called the tool it was shown got `Unknown Peer client action` and a model could only
//! succeed by copying the action's `example`, which contradicted the action's own name.
//!
//! These are protocol-level assertions on decoded bytes, not on the size of the tool list: the
//! peer wire payload for `request` is `<index><begin><length>` as three big-endian u32 and for
//! `piece` is `<index><begin><block>`, so the exact hex is checked against BEP 3 rather than
//! against whatever the implementation happens to emit.

use netget::client::torrent_peer::actions::TorrentPeerClientProtocol;
use netget::llm::actions::client_trait::{Client, ClientActionResult};
use serde_json::json;

/// Run one action through the protocol's executor and return the framed `peer_message` it
/// produced: `(message_type, payload_hex)`.
fn framed(action: serde_json::Value) -> (u64, String) {
    let protocol = TorrentPeerClientProtocol::new();
    match protocol
        .execute_action(action)
        .expect("the executor must accept the action name it advertises")
    {
        ClientActionResult::Custom { name, data } => {
            assert_eq!(
                name, "peer_message",
                "peer wire messages are framed by mod.rs's `peer_message` handler"
            );
            (
                data["message_type"].as_u64().expect("message_type"),
                data["payload"].as_str().unwrap_or("").to_string(),
            )
        }
        other => panic!("expected a framed peer_message, got {:?}", other),
    }
}

#[test]
fn interested_and_not_interested_are_bare_message_ids() {
    // BEP 3: interested = id 2, not interested = id 3, both with an empty payload.
    assert_eq!(
        framed(json!({"type": "peer_interested"})),
        (2, String::new())
    );
    assert_eq!(
        framed(json!({"type": "peer_not_interested"})),
        (3, String::new())
    );
}

#[test]
fn request_piece_frames_index_begin_length_as_big_endian_u32() {
    let (message_type, payload) = framed(json!({
        "type": "peer_request_piece",
        "index": 1,
        "begin": 16384,
        "length": 16384
    }));

    assert_eq!(message_type, 6, "BEP 3 request is message id 6");
    // 1 -> 00000001, 16384 -> 00004000, 16384 -> 00004000
    assert_eq!(
        payload, "000000010000400000004000",
        "request payload is <index><begin><length>, three big-endian u32"
    );
}

#[test]
fn send_piece_frames_index_begin_then_the_raw_block() {
    let (message_type, payload) = framed(json!({
        "type": "peer_send_piece",
        "index": 2,
        "begin": 0,
        "block": "deadbeef"
    }));

    assert_eq!(message_type, 7, "BEP 3 piece is message id 7");
    assert_eq!(
        payload, "0000000200000000deadbeef",
        "piece payload is <index><begin> as big-endian u32 followed by the block itself"
    );
}

/// A missing required parameter must be a named error, not a silently truncated frame — the
/// fail-open shape this codebase treats as the dangerous default.
#[test]
fn a_request_without_its_parameters_is_refused_by_name() {
    let protocol = TorrentPeerClientProtocol::new();
    let err = protocol
        .execute_action(json!({"type": "peer_request_piece"}))
        .expect_err("a request with no index/begin/length must fail");

    let msg = err.to_string();
    assert!(
        msg.contains("index"),
        "the error must name the missing parameter, got: {}",
        msg
    );
}

/// The raw escape hatch stays: `peer_message` is what `mod.rs` dispatches on, and a caller that
/// already knows a message id this protocol has no named action for (have, bitfield, cancel)
/// must still be able to send it.
#[test]
fn the_raw_peer_message_shape_still_works() {
    let (message_type, payload) = framed(json!({
        "type": "peer_message",
        "message_type": 4,
        "payload": "00000005"
    }));

    assert_eq!(message_type, 4, "have");
    assert_eq!(payload, "00000005");
}
