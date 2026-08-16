//! Live-LLM peer-to-peer suite (event-level): BitTorrent DHT, BitTorrent
//! peer wire, Bitcoin P2P.
//!
//! All three are peer protocols: driving them end-to-end needs a real swarm
//! or a real node on the other side, but every one of them turns on a
//! correlation value the model must copy exactly, which is precisely what an
//! event-level case grades.
//!
//! Protocol facts these cases encode:
//! - **DHT (BEP 5 / KRPC)**: the transaction id `t` must be echoed
//!   byte-for-byte or the querying node discards the reply as unsolicited;
//!   `id`/`target`/`info_hash` are 20 raw bytes and always travel as 40 hex
//!   chars; `get_peers` hands out a token the querier must present in a later
//!   `announce_peer`, and a bad token is refused with KRPC error **203**
//!   (protocol error) — not silence.
//! - **BitTorrent peer wire (BEP 3)**: the handshake reply must echo the same
//!   `info_hash` or the peer disconnects immediately; a `request` is answered
//!   with a `piece` echoing the same `index` and `begin`, or the block is
//!   discarded as unrelated; the peer's own `bitfield` is an announcement, not
//!   a request, so the answer is a state message.
//! - **Bitcoin P2P**: a `ping` is answered with a `pong` carrying **the same
//!   nonce** — bitcoind uses it to match its own outstanding ping and will
//!   eventually drop a peer that never answers correctly. The handshake is
//!   `version` → `verack`, in that order.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// A KRPC transaction id is short and opaque; the check is byte equality
/// after stripping the quoting a model might add.
fn echoes_transaction(expected: &'static str) -> ParamCheck {
    ParamCheck::custom(
        "transaction_id",
        format!("echoes the query's transaction id {:?} verbatim", expected),
        move |v| {
            let s = v.as_str().unwrap_or("").trim().to_lowercase();
            if s == expected {
                Ok(())
            } else {
                Err(format!(
                    "KRPC replies are matched by transaction id alone; expected {:?}, got {:?}",
                    expected, v
                ))
            }
        },
    )
}

/// A node id is 20 bytes — exactly 40 hex characters. Anything else is
/// dropped by the receiving node.
fn is_a_node_id(name: &'static str) -> ParamCheck {
    ParamCheck::custom(name, "20 bytes as 40 hex chars", move |v| {
        let s = v.as_str().unwrap_or("").trim().to_lowercase();
        let s = s.strip_prefix("0x").unwrap_or(&s);
        if s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(())
        } else {
            Err(format!(
                "a DHT node id is 20 raw bytes = 40 hex chars; got {:?} ({} chars)",
                v,
                s.len()
            ))
        }
    })
}

// ---------------------------------------------------------------------------
// BitTorrent DHT
// ---------------------------------------------------------------------------

/// The simplest KRPC exchange, and the one that proves the correlation rule:
/// a ping reply is nothing but the transaction id and our node id.
#[tokio::test]
async fn dht_ping_reply_echoes_the_transaction() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-DHT",
        "You are a BitTorrent DHT node with node ID \
         0123456789abcdef0123456789abcdef01234567. Answer every query \
         addressed to you.",
        "dht_ping_query",
        json!({
            "query_type": "ping",
            "transaction_id": "aa",
            "id": "f0e1d2c3b4a5968778695a4b3c2d1e0ff0e1d2c3",
            "client_ip": "203.0.113.20:6881"
        }),
    )
    .expect_action("send_ping_response")
    .check(echoes_transaction("aa"))
    .check(is_a_node_id("node_id"))
    .run()
    .await
}

/// find_node asks for the nodes closest to a target. An empty list is a
/// legitimate answer for a node that knows nobody — what is *not* legitimate
/// is inventing a malformed entry, so the check grades the shape of whatever
/// is returned.
#[tokio::test]
async fn dht_find_node_returns_well_formed_nodes() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-DHT",
        "You are a BitTorrent DHT node with node ID \
         0123456789abcdef0123456789abcdef01234567. You know of one other \
         node: ID abcdefabcdefabcdefabcdefabcdefabcdefabcd at 198.51.100.30 \
         port 6881. Answer find_node queries with the nodes you know.",
        "dht_find_node_query",
        json!({
            "query_type": "find_node",
            "transaction_id": "bb",
            "id": "f0e1d2c3b4a5968778695a4b3c2d1e0ff0e1d2c3",
            "target": "1111111111111111111111111111111111111111",
            "client_ip": "203.0.113.20:6881"
        }),
    )
    .expect_action("send_find_node_response")
    .check(echoes_transaction("bb"))
    .check(ParamCheck::custom(
        "nodes",
        "each entry is {id: 40 hex, ip: IPv4 dotted quad, port}",
        |v| {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("nodes must be an array, got {}", v))?;
            for n in arr {
                let id = n.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let ip = n.get("ip").and_then(|x| x.as_str()).unwrap_or("");
                if id.len() != 40 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(format!(
                        "node id must be 40 hex chars (entries with any other length are \
                         silently dropped by the server); got {:?}",
                        id
                    ));
                }
                if ip.split('.').count() != 4 {
                    return Err(format!("node ip must be an IPv4 dotted quad, got {:?}", ip));
                }
                if n.get("port").is_none() {
                    return Err(format!("node entry is missing its port: {}", n));
                }
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// get_peers hands out the token the querier must present in a later
/// announce_peer. Omitting it makes announcing impossible.
#[tokio::test]
async fn dht_get_peers_returns_a_token() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-DHT",
        "You are a BitTorrent DHT node with node ID \
         0123456789abcdef0123456789abcdef01234567. One peer is downloading \
         every torrent you are asked about: 198.51.100.44 port 51413. Hand \
         out the token netget-tok-1 so nodes can announce to you afterwards.",
        "dht_get_peers_query",
        json!({
            "query_type": "get_peers",
            "transaction_id": "cc",
            "id": "f0e1d2c3b4a5968778695a4b3c2d1e0ff0e1d2c3",
            "info_hash": "2222222222222222222222222222222222222222",
            "client_ip": "203.0.113.20:6881"
        }),
    )
    .expect_action("send_get_peers_response")
    .check(echoes_transaction("cc"))
    // Plain text, not hex — the server sends the token through unencoded.
    .check(ParamCheck::contains("token", "netget-tok-1"))
    .check(ParamCheck::custom(
        "peers",
        "the known peer, as {ip: IPv4, port}",
        |v| {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("peers must be an array, got {}", v))?;
            let found = arr.iter().any(|p| {
                p.get("ip").and_then(|x| x.as_str()) == Some("198.51.100.44")
                    && p.get("port")
                        .map(|x| x.to_string().trim_matches('"').to_string())
                        == Some("51413".to_string())
            });
            if found {
                Ok(())
            } else {
                Err(format!(
                    "expected the known peer 198.51.100.44:51413 as {{ip, port}}, got {}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// Nothing validates the announce token on the wire, so refusing a wrong one
/// is a decision the model owns — and BEP 5 says the refusal is error 203,
/// not silence.
#[tokio::test]
async fn dht_announce_with_a_bad_token_is_refused_with_203() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-DHT",
        "You are a BitTorrent DHT node with node ID \
         0123456789abcdef0123456789abcdef01234567. The only token you have \
         ever handed out is netget-tok-1. Refuse any announce_peer that \
         presents a different token.",
        "dht_announce_peer_query",
        json!({
            "query_type": "announce_peer",
            "transaction_id": "dd",
            "id": "f0e1d2c3b4a5968778695a4b3c2d1e0ff0e1d2c3",
            "info_hash": "2222222222222222222222222222222222222222",
            "port": 51413,
            "token": "not-the-token-you-issued",
            "client_ip": "203.0.113.20:6881"
        }),
    )
    .expect_action("send_dht_error_response")
    .check(echoes_transaction("dd"))
    .check(ParamCheck::equals("code", json!(203)))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// BitTorrent peer wire
// ---------------------------------------------------------------------------

/// The handshake reply must carry the *same* info_hash. A peer that receives
/// any other value drops the connection before anything else happens.
#[tokio::test]
async fn peer_handshake_echoes_the_info_hash() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-Peer",
        "You are a BitTorrent seeder. Complete the handshake with any peer \
         that connects for a torrent you are serving.",
        "peer_handshake",
        json!({
            "info_hash": "3a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d",
            "peer_id": "-qB4550-abcdefghijkl",
            "peer_id_hex": "2d7142343535302d6162636465666768696a6b6c",
            "peer_addr": "203.0.113.30:51413",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("send_handshake")
    .check(ParamCheck::custom(
        "info_hash",
        "echoes the peer's info_hash exactly (40 hex chars)",
        |v| {
            let s = v.as_str().unwrap_or("").trim().to_lowercase();
            if s == "3a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d" {
                Ok(())
            } else {
                Err(format!(
                    "the handshake reply must echo the peer's info_hash or it disconnects; \
                     expected 3a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d, got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// A block is matched to its request by index and offset. Getting either
/// wrong makes the peer discard the data and re-request it forever.
#[tokio::test]
async fn peer_request_is_answered_with_the_matching_piece() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-Peer",
        "You are a BitTorrent seeder that has every piece of the torrent. \
         Serve every block a peer requests.",
        "peer_request_message",
        json!({
            "message_type": "request",
            "index": 7,
            "begin": 32768,
            "length": 16384,
            "peer_addr": "203.0.113.30:51413",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("send_piece")
    .check(ParamCheck::equals("index", json!(7)))
    .check(ParamCheck::equals("begin", json!(32768)))
    .check(ParamCheck::custom(
        "block_hex",
        "hex-encoded block data (an even number of hex digits)",
        |v| {
            let s = v.as_str().unwrap_or("").trim().to_lowercase();
            let s = s.strip_prefix("0x").unwrap_or(&s);
            if s.is_empty() {
                Err("block_hex must carry the block data".to_string())
            } else if !s.chars().all(|c| c.is_ascii_hexdigit()) {
                Err(format!(
                    "block_hex is hex-encoded, never text or base64; got {:?}",
                    v
                ))
            } else if s.len() % 2 != 0 {
                Err(format!("hex must be whole bytes; got {} digits", s.len()))
            } else {
                Ok(())
            }
        },
    ))
    .run()
    .await
}

/// A peer announcing which pieces it holds is not asking for anything. The
/// correct answer is a state message about what *we* want from it.
#[tokio::test]
async fn peer_bitfield_is_answered_with_interest() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-Peer",
        "You are a BitTorrent client that has none of this torrent yet and \
         wants to download it. React to what peers tell you they hold.",
        "peer_bitfield_message",
        json!({
            "message_type": "bitfield",
            "bitfield": "ff",
            "peer_addr": "203.0.113.30:51413",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("send_interested")
    .run()
    .await
}

/// choke/unchoke/interested/not_interested all arrive on one event and are
/// told apart by `message_type` — the model has to read it to answer.
#[tokio::test]
async fn peer_interested_is_answered_with_unchoke() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-Peer",
        "You are a generous BitTorrent seeder: unchoke any peer that tells \
         you it is interested, so it can start requesting blocks.",
        "peer_choke_message",
        json!({
            "message_type": "interested",
            "peer_addr": "203.0.113.30:51413",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("send_unchoke")
    .run()
    .await
}

/// A keep-alive carries no payload and asks for nothing but proof of life.
#[tokio::test]
async fn peer_keepalive_is_answered_in_kind() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Torrent-Peer",
        "You are a BitTorrent seeder. Keep connections alive: when a peer \
         sends a keep-alive, send one back so the connection is not dropped.",
        "peer_message",
        json!({
            "message_type": "keepalive",
            "peer_addr": "203.0.113.30:51413",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("send_keepalive")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Bitcoin P2P
// ---------------------------------------------------------------------------

/// The Bitcoin handshake is initiated with `version`, and the network magic
/// has to match the network the peer is on or every message is rejected as
/// malformed.
#[tokio::test]
async fn bitcoin_connection_opens_with_version() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Bitcoin P2P",
        "You are a Bitcoin mainnet node. Start the handshake as soon as a \
         peer connects, advertising the user agent /netget-live:1.0/.",
        "bitcoin_connection_opened",
        json!({
            "peer_addr": "203.0.113.60:8333",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("send_version")
    .check(ParamCheck::equals("network", json!("mainnet")))
    .check(ParamCheck::contains("user_agent", "netget-live"))
    .run()
    .await
}

/// The handshake completes with `verack` in answer to the peer's `version`.
#[tokio::test]
async fn bitcoin_version_is_acknowledged_with_verack() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Bitcoin P2P",
        "You are a Bitcoin mainnet node. Complete the handshake with every \
         peer that greets you.",
        "bitcoin_message_received",
        json!({
            "message_type": "version",
            "message": {
                "version": 70016,
                "services": 1037,
                "timestamp": 1735689600,
                "receiver": "127.0.0.1:8333",
                "sender": "203.0.113.60:8333",
                "nonce": 8123456789012345678u64,
                "user_agent": "/Satoshi:27.0.0/",
                "start_height": 880000,
                "relay": true
            }
        }),
    )
    .expect_action("send_verack")
    .run()
    .await
}

/// The one value in Bitcoin's keep-alive that matters: the pong nonce must be
/// the ping's nonce. bitcoind matches its outstanding ping on it, and a peer
/// that never answers correctly is eventually disconnected.
#[tokio::test]
async fn bitcoin_pong_echoes_the_ping_nonce() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Bitcoin P2P",
        "You are a Bitcoin mainnet node. Answer every ping so peers keep you \
         in their address book.",
        "bitcoin_message_received",
        json!({
            "message_type": "ping",
            "message": { "nonce": 7431912345678901u64 }
        }),
    )
    .expect_action("send_pong")
    .check(ParamCheck::custom(
        "nonce",
        "echoes the ping's nonce exactly (bitcoind matches on it)",
        |v| {
            let got = v
                .as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(|s| s.trim().to_string()));
            match got.as_deref() {
                Some("7431912345678901") => Ok(()),
                Some(other) => Err(format!(
                    "pong must carry the ping's nonce 7431912345678901, got {}",
                    other
                )),
                None => Err(format!("nonce is missing or not a number: {}", v)),
            }
        },
    ))
    .run()
    .await
}
