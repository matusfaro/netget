//! Live-LLM real-time-media suite (event-level): RTP, TURN, WebRTC, WebRTC
//! Signaling.
//!
//! None of these can be driven end-to-end here — RTP and TURN need a real
//! media peer, WebRTC needs a full ICE/DTLS stack on the other side — but the
//! decision the model makes is identical either way, and it is the part worth
//! grading.
//!
//! Protocol facts these cases encode:
//! - **RTP**: the model must never emit raw samples. `send_rtp_audio`
//!   describes media (`content` = tone/dtmf/silence/raw) and only `raw`
//!   carries bytes, which must be `encoding: "hex"` — the server hex-decodes
//!   and never sniffs. DTMF digits belong in `digits`, not in `content`.
//! - **RTCP**: a sender report carries this endpoint's *own* cumulative
//!   counters, not the peer's.
//! - **TURN**: `transaction_id` is the entire correlation mechanism — a
//!   client discards any reply whose transaction ID differs from the request
//!   it sent (RFC 8656 / RFC 5389). And the relay address is chosen by
//!   NetGet, not the model: `relay_address` must be copied verbatim from the
//!   event or the server refuses the action with 508.
//! - **WebRTC**: this server carries **no media**, only data channels. An
//!   offer asking for audio/video cannot be served, and the offer event
//!   documents that answering with anything other than `accept_offer`
//!   refuses the peer — so a refusal must be explicit.
//! - **WebRTC Signaling**: the disconnect and message events are declared
//!   `with_no_actions()` — the socket is already gone / the message is
//!   already forwarded — so the only correct answer is an observation, and
//!   inventing a protocol action there would be wrong.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

// ---------------------------------------------------------------------------
// RTP
// ---------------------------------------------------------------------------

/// A media reply is *described*, never sampled: the model asks for a tone by
/// frequency and the server synthesizes it. Emitting raw samples here would
/// be the "no raw bytes in action parameters" rule broken.
#[tokio::test]
async fn rtp_tone_is_described_not_sampled() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RTP",
        "You are a VoIP answering machine. As soon as audio arrives from the \
         caller, play a 440 Hz ringback tone back to them for one second.",
        "rtp_packet_received",
        json!({
            "peer_addr": "192.168.1.50:40000",
            "local_addr": "192.168.1.100:5004",
            "connection_id": "conn-1",
            "payload_type": 0,
            "sequence": 1042,
            "timestamp": 160320,
            "ssrc": 305419896u32,
            "marker": false,
            "payload_len": 160
        }),
    )
    .expect_action("send_rtp_audio")
    .check(ParamCheck::equals("content", json!("tone")))
    .check(ParamCheck::custom(
        "tone_hz",
        "440 Hz as instructed, within the 20-3800 Hz the server accepts",
        |v| {
            let hz = v.as_f64().or_else(|| v.as_str()?.parse().ok());
            match hz {
                Some(hz) if (430.0..=450.0).contains(&hz) => Ok(()),
                Some(hz) => Err(format!("instructed 440 Hz, got {}", hz)),
                None => Err(format!("tone_hz is not a number: {}", v)),
            }
        },
    ))
    .check(ParamCheck::custom(
        "duration_ms",
        "one second, in milliseconds (the parameter's unit)",
        |v| {
            let ms = v.as_f64().or_else(|| v.as_str()?.parse().ok());
            match ms {
                // 1 s expressed in ms. A model that answered `1` took the
                // unit from the instruction rather than the parameter.
                Some(ms) if (900.0..=1100.0).contains(&ms) => Ok(()),
                Some(ms) => Err(format!(
                    "duration_ms is milliseconds; one second is 1000, got {}",
                    ms
                )),
                None => Err(format!("duration_ms is not a number: {}", v)),
            }
        },
    ))
    // "raw" is the only content that may carry bytes, and this is not it.
    .check_action(|a| match a.get("samples") {
        Some(v) if !v.is_null() => Err(format!(
            "a tone is synthesized by the server; samples must not be sent (got {})",
            v
        )),
        _ => Ok(()),
    })
    .run()
    .await
}

/// DTMF is its own `content` kind with its own parameter. Putting the digits
/// in the wrong place produces silence on the wire.
#[tokio::test]
async fn rtp_dtmf_digits_use_the_digits_parameter() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RTP",
        "You are an IVR test endpoint. Whenever the caller sends audio, dial \
         back the DTMF digits 1234 to them.",
        "rtp_packet_received",
        json!({
            "peer_addr": "192.168.1.50:40000",
            "local_addr": "192.168.1.100:5004",
            "connection_id": "conn-1",
            "payload_type": 0,
            "sequence": 88,
            "timestamp": 14080,
            "ssrc": 2882343476u32,
            "marker": false,
            "payload_len": 160
        }),
    )
    .expect_action("send_rtp_audio")
    .check(ParamCheck::equals("content", json!("dtmf")))
    .check(ParamCheck::custom(
        "digits",
        "the four digits 1234, in the digits parameter",
        |v| {
            let s = v.as_str().unwrap_or("").replace([' ', ',', '-'], "");
            if s == "1234" {
                Ok(())
            } else {
                Err(format!("expected digits \"1234\", got {:?}", v))
            }
        },
    ))
    .run()
    .await
}

/// A sender report reports *our* transmission counters. The event carries the
/// peer's numbers; copying them back would misreport the stream.
#[tokio::test]
async fn rtcp_sender_report_carries_our_own_counters() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RTP",
        "You are an RTP endpoint that has itself sent 500 packets totalling \
         80000 payload octets so far. When an RTCP report arrives, answer \
         with your own sender report stating those totals.",
        "rtcp_packet_received",
        json!({
            "peer_addr": "192.168.1.50:40001",
            "local_addr": "192.168.1.100:5005",
            "connection_id": "conn-1",
            "packet_type": 201,
            "length": 32
        }),
    )
    .expect_action("send_rtcp_sender_report")
    .check(ParamCheck::equals("packet_count", json!(500)))
    .check(ParamCheck::equals("octet_count", json!(80000)))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// TURN
// ---------------------------------------------------------------------------

/// The two things a TURN Allocate response cannot get wrong: the transaction
/// ID (or the client drops the reply) and the relay address (which NetGet has
/// already bound — any other value is refused with 508).
#[tokio::test]
async fn turn_allocate_echoes_transaction_and_relay_address() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TURN",
        "You are a TURN relay for a WebRTC deployment. Grant every allocation \
         request, with a lifetime of 600 seconds.",
        "turn_allocate_request",
        json!({
            "transaction_id": "a1b2c3d4e5f60718293a4b5c",
            "peer_addr": "203.0.113.9:54321",
            "local_addr": "127.0.0.1:3478",
            "message_type": "AllocateRequest",
            "bytes_received": 44,
            "existing_allocations": [],
            "relay_address": "127.0.0.1:49312",
            "requested_lifetime_seconds": 600,
            "requested_transport": "udp"
        }),
    )
    .expect_action("send_turn_allocate_response")
    .check(ParamCheck::equals(
        "transaction_id",
        json!("a1b2c3d4e5f60718293a4b5c"),
    ))
    // NetGet bound this socket already. Any other address is refused with 508.
    .check(ParamCheck::equals(
        "relay_address",
        json!("127.0.0.1:49312"),
    ))
    .check(ParamCheck::equals("lifetime_seconds", json!(600)))
    .run()
    .await
}

/// A refresh only extends an existing allocation: transaction ID plus the
/// granted lifetime.
#[tokio::test]
async fn turn_refresh_grants_the_requested_lifetime() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TURN",
        "You are a TURN relay. Grant every refresh request, extending the \
         allocation by the lifetime the client asks for.",
        "turn_refresh_request",
        json!({
            "transaction_id": "0f1e2d3c4b5a69788796a5b4",
            "peer_addr": "203.0.113.9:54321",
            "local_addr": "127.0.0.1:3478",
            "message_type": "RefreshRequest",
            "bytes_received": 32,
            "existing_allocations": [{
                "allocation_id": "alloc-1",
                "relay_address": "127.0.0.1:49312",
                "lifetime_seconds": 600,
                "expires_in_seconds": 42,
                "permitted_peers": ["198.51.100.7"],
                "channels": []
            }],
            "requested_lifetime_seconds": 900
        }),
    )
    .expect_action("send_turn_refresh_response")
    .check(ParamCheck::equals(
        "transaction_id",
        json!("0f1e2d3c4b5a69788796a5b4"),
    ))
    .check(ParamCheck::equals("lifetime_seconds", json!(900)))
    .run()
    .await
}

/// CreatePermission names the peers allowed to exchange relayed traffic.
/// Permitting the wrong address silently blackholes the call.
#[tokio::test]
async fn turn_create_permission_names_the_requested_peer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TURN",
        "You are a TURN relay. Permit every peer a client asks to talk to.",
        "turn_create_permission_request",
        json!({
            "transaction_id": "112233445566778899aabbcc",
            "peer_addr": "203.0.113.9:54321",
            "local_addr": "127.0.0.1:3478",
            "message_type": "CreatePermissionRequest",
            "bytes_received": 40,
            "existing_allocations": [{
                "allocation_id": "alloc-1",
                "relay_address": "127.0.0.1:49312",
                "lifetime_seconds": 600,
                "expires_in_seconds": 580,
                "permitted_peers": [],
                "channels": []
            }],
            "peer_addresses": ["198.51.100.7"]
        }),
    )
    .expect_action("send_turn_create_permission_response")
    .check(ParamCheck::equals(
        "transaction_id",
        json!("112233445566778899aabbcc"),
    ))
    .check(ParamCheck::custom(
        "peer_addresses",
        "permits the peer the client asked about, or omits the field to permit all of them",
        |v| {
            // Omitting `peer_addresses` is the documented way to permit exactly the peers the
            // request named (see src/server/turn/CLAUDE.md), and the executor implements it —
            // peers the request did not name are ignored either way, so a hallucinated address
            // cannot open a hole. Requiring the field therefore failed the model for choosing
            // the simpler correct answer. What must never pass is naming a *different* peer:
            // `peer_addr` (the client, 203.0.113.9) sits right beside `peer_addresses` in the
            // event and is the address a model grabs when the two are confused.
            if v.is_null() {
                return Ok(());
            }
            let flat = v.to_string();
            if !flat.contains("198.51.100.7") {
                return Err(format!(
                    "must permit the requested peer 198.51.100.7, got {}",
                    v
                ));
            }
            if flat.contains("203.0.113.9") {
                return Err(format!(
                    "permitted 203.0.113.9, which is the client's own address (peer_addr), \
                     not a peer it asked to reach: {}",
                    v
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// ChannelBind is answered with the transaction ID alone — the channel number
/// and peer are already fixed by the request, and the server refuses a
/// response that tries to change them.
#[tokio::test]
async fn turn_channel_bind_echoes_the_transaction() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TURN",
        "You are a TURN relay. Accept every channel bind request so the \
         client can use the compact ChannelData framing.",
        "turn_channel_bind_request",
        json!({
            "transaction_id": "ccbbaa998877665544332211",
            "peer_addr": "203.0.113.9:54321",
            "local_addr": "127.0.0.1:3478",
            "message_type": "ChannelBindRequest",
            "bytes_received": 48,
            "existing_allocations": [{
                "allocation_id": "alloc-1",
                "relay_address": "127.0.0.1:49312",
                "lifetime_seconds": 600,
                "expires_in_seconds": 560,
                "permitted_peers": ["198.51.100.7"],
                "channels": []
            }],
            "channel_number": 16384,
            "peer_address": "198.51.100.7:9000"
        }),
    )
    .expect_action("send_turn_channel_bind_response")
    .check(ParamCheck::equals(
        "transaction_id",
        json!("ccbbaa998877665544332211"),
    ))
    .run()
    .await
}

/// A denial must be an explicit TURN error carrying the transaction ID, not
/// silence: a client that gets nothing back retries until it times out.
#[tokio::test]
async fn turn_denied_allocation_is_an_explicit_error() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TURN",
        "You are a TURN relay restricted to the 198.51.100.0/24 network. \
         Refuse allocation requests from any other network with a 403 \
         Forbidden error response.",
        "turn_allocate_request",
        json!({
            "transaction_id": "deadbeefcafef00dba5eba11",
            "peer_addr": "203.0.113.9:54321",
            "local_addr": "127.0.0.1:3478",
            "message_type": "AllocateRequest",
            "bytes_received": 44,
            "existing_allocations": [],
            "relay_address": "127.0.0.1:49500",
            "requested_lifetime_seconds": 600,
            "requested_transport": "udp"
        }),
    )
    .expect_action("send_turn_error_response")
    .check(ParamCheck::equals(
        "transaction_id",
        json!("deadbeefcafef00dba5eba11"),
    ))
    .check(ParamCheck::equals("error_code", json!(403)))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// WebRTC
// ---------------------------------------------------------------------------

/// A data-channel offer this server can actually serve must be admitted.
#[tokio::test]
async fn webrtc_data_channel_offer_is_accepted() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebRTC",
        "You are a public WebRTC data-channel echo service. Admit any peer \
         that offers a data channel.",
        "webrtc_offer_received",
        json!({
            "peer_id": "peer-7431",
            "remote_addr": "203.0.113.44:51000",
            "requests_data_channel": true,
            "media_kinds": []
        }),
    )
    .expect_action("accept_offer")
    .run()
    .await
}

/// This server carries no media. An audio/video-only offer cannot be served,
/// and the event documents that a refusal must be explicit — so the model has
/// to notice that `media_kinds` is non-empty and `requests_data_channel` is
/// false.
#[tokio::test]
async fn webrtc_media_only_offer_is_refused() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebRTC",
        "You are a WebRTC data-channel service. This server carries no audio \
         or video at all, so refuse any peer whose offer you cannot serve, \
         saying why.",
        "webrtc_offer_received",
        json!({
            "peer_id": "peer-9002",
            "remote_addr": "203.0.113.44:51044",
            "requests_data_channel": false,
            "media_kinds": ["audio", "video"]
        }),
    )
    .expect_action("reject_offer")
    .check(ParamCheck::non_empty("reason"))
    .run()
    .await
}

/// The data channel is open: the greeting goes out over it.
#[tokio::test]
async fn webrtc_peer_connected_greets_over_the_channel() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebRTC",
        "You are a WebRTC data-channel service. Greet every peer whose data \
         channel opens with exactly: WELCOME-NETGET",
        "webrtc_peer_connected",
        json!({
            "peer_id": "peer-7431",
            "channel_label": "chat"
        }),
    )
    .expect_action("send_message")
    .check(ParamCheck::contains("message", "WELCOME-NETGET"))
    .run()
    .await
}

/// Echo over the data channel: the reply is the peer's own text.
#[tokio::test]
async fn webrtc_message_is_echoed() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebRTC",
        "You are a WebRTC echo service. Send every peer's message straight \
         back to them, unchanged.",
        "webrtc_message_received",
        json!({
            "peer_id": "peer-7431",
            "message": "netget-live-echo-7431",
            "is_binary": false
        }),
    )
    .expect_action("send_message")
    .check(ParamCheck::contains("message", "netget-live-echo-7431"))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// WebRTC Signaling
// ---------------------------------------------------------------------------

/// A signaling message is an object carrying its own `type` field, addressed
/// to a registered peer ID. A bare string would not route.
#[tokio::test]
async fn signaling_welcome_is_addressed_to_the_new_peer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebRTC Signaling",
        "You are a WebRTC signaling server. When a peer registers, send it a \
         relay message welcoming it to the room.",
        "webrtc_signaling_peer_connected",
        json!({
            "peer_id": "alice",
            "remote_addr": "203.0.113.44:51000",
            "peer_count": 1
        }),
    )
    .expect_action("send_signaling_message")
    .check(ParamCheck::custom(
        "message",
        "an object carrying its own `type`, addressed to the peer that registered",
        |v| {
            let obj = v
                .as_object()
                .ok_or_else(|| format!("message must be an object, got {}", v))?;
            if !obj.contains_key("type") {
                return Err(format!(
                    "signaling messages carry their own `type` field; got keys {:?}",
                    obj.keys().collect::<Vec<_>>()
                ));
            }
            let addressed = obj
                .get("to")
                .and_then(|t| t.as_str())
                .map(|t| t == "alice")
                .unwrap_or(false);
            if addressed {
                Ok(())
            } else {
                Err(format!(
                    "must be addressed to the peer that registered (to: \"alice\"), got {}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// The socket is already gone by the time this fires — the event is declared
/// `with_no_actions()`. Recording the departure is the only correct answer;
/// trying to message the peer would be a protocol error.
#[tokio::test]
async fn signaling_disconnect_is_recorded_not_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebRTC Signaling",
        "You are a WebRTC signaling server. Keep a record of peers coming and \
         going so the room roster stays accurate.",
        "webrtc_signaling_peer_disconnected",
        json!({
            "peer_id": "alice",
            "peer_count": 0
        }),
    )
    .expect_action("append_to_log")
    .or_action("append_memory")
    .check_action(|a| {
        let flat = a.to_string();
        if flat.contains("alice") {
            Ok(())
        } else {
            Err(format!(
                "the record should name the peer that left (alice), got {}",
                a
            ))
        }
    })
    .run()
    .await
}

/// The message has already been forwarded — this event exists for
/// observation, and is declared `with_no_actions()` for that reason.
#[tokio::test]
async fn signaling_forwarded_message_is_only_observed() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebRTC Signaling",
        "You are a WebRTC signaling server. Remember who is negotiating with \
         whom so you can report on the room later.",
        "webrtc_signaling_message_received",
        json!({
            "peer_id": "alice",
            "message_type": "offer",
            "target_peer": "bob",
            "delivered": true
        }),
    )
    .expect_action("append_memory")
    .or_action("append_to_log")
    .check_action(|a| {
        let flat = a.to_string().to_lowercase();
        if flat.contains("alice") && flat.contains("bob") {
            Ok(())
        } else {
            Err(format!(
                "the note should name both ends of the negotiation, got {}",
                a
            ))
        }
    })
    .run()
    .await
}
