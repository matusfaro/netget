//! Live-LLM VPN suite (event-level): WireGuard, OpenVPN, IPsec.
//!
//! All three need root, a TUN device or a privileged port; the access decision
//! they ask the model for does not.
//!
//! Protocol facts these cases encode:
//! - WireGuard: the peer is identified **only** by its Curve25519 public key,
//!   so an authorization that does not quote that exact key configures the
//!   wrong peer; `allowed_ips` is what the peer may route and must not be
//!   empty (an empty list can route nothing);
//! - OpenVPN: a refusal at reset time is *silence* on the wire, but it must
//!   still be an explicit `reject_peer` — returning neither decision leaves
//!   the peer unanswered for the wrong reason and is indistinguishable from an
//!   outage;
//! - IPsec: the server is receive-only, so every action is a classification.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

const PEER_KEY: &str = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";
const UNKNOWN_KEY: &str = "mF7pXk2QvLd9RtYbNc4WjH8sAe1ZuPo6IyGx3TrKwVE=";

/// An allowed peer: authorize it, quoting its public key and pinning the VPN
/// address it may use.
#[tokio::test]
async fn wireguard_authorizes_known_peer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WireGuard",
        format!(
            "You run a WireGuard VPN on 10.20.30.0/24. The peer whose public \
             key is {} is an approved client and must be given the single VPN \
             address 10.20.30.2/32.",
            PEER_KEY
        ),
        "wireguard_peer_connected",
        json!({
            "public_key": PEER_KEY,
            "endpoint": "203.0.113.45:51820",
            "allowed_ips": [],
            "server_public_key": "SGVsbG9TZXJ2ZXJLZXlCYXNlNjRFeGFtcGxlVmFsdWUxMjM=",
            "listen_port": 51820
        }),
    )
    .expect_action("authorize_peer")
    .check(ParamCheck::equals("public_key", json!(PEER_KEY)))
    .check(ParamCheck::custom(
        "allowed_ips",
        "pins 10.20.30.2/32 and is not empty",
        |v| {
            let list = v
                .as_array()
                .ok_or_else(|| format!("allowed_ips must be an array, got {}", v))?;
            if list.is_empty() {
                return Err(
                    "allowed_ips must not be empty — a peer with no allowed IPs \
                     can route nothing"
                        .to_string(),
                );
            }
            if list.iter().any(|ip| {
                ip.as_str()
                    .map(|s| s.contains("10.20.30.2"))
                    .unwrap_or(false)
            }) {
                Ok(())
            } else {
                Err(format!("expected the instructed 10.20.30.2/32, got {}", v))
            }
        },
    ))
    .run()
    .await
}

/// A peer that is not on the allow list must be removed from the interface,
/// not left configured.
#[tokio::test]
async fn wireguard_removes_unknown_peer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WireGuard",
        format!(
            "You run a WireGuard VPN with a strict allow list containing \
             exactly one public key: {}. Any other peer must be removed from \
             the interface immediately.",
            PEER_KEY
        ),
        "wireguard_peer_connected",
        json!({
            "public_key": UNKNOWN_KEY,
            "endpoint": "198.51.100.7:51820",
            "allowed_ips": ["10.20.30.9/32"],
            "server_public_key": "SGVsbG9TZXJ2ZXJLZXlCYXNlNjRFeGFtcGxlVmFsdWUxMjM=",
            "listen_port": 51820
        }),
    )
    // The protocol declares both names for the same effect ("reject_peer …
    // Identical in effect to disconnect_peer"), so either is correct here.
    .expect_action("disconnect_peer")
    .or_action("reject_peer")
    .check(ParamCheck::equals("public_key", json!(UNKNOWN_KEY)))
    .run()
    .await
}

/// A handshake the policy answers: the acceptance must be explicit, because
/// nothing is written to the peer without it.
#[tokio::test]
async fn openvpn_accepts_allowed_peer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "OpenVPN",
        "You are an OpenVPN control-plane responder. Clients from the \
         192.0.2.0/24 network are expected: answer their session resets so the \
         handshake can continue.",
        "openvpn_peer_reset",
        json!({
            "peer_addr": "192.0.2.10:1194",
            "client_session_id": "0123456789abcdef",
            "key_id": 0,
            "reset_type": "PControlHardResetClientV2",
            "packet_id": 0,
            "peer_count": 0
        }),
    )
    .expect_action("accept_peer")
    .run()
    .await
}

/// A handshake the policy refuses: the refusal must be an explicit
/// `reject_peer`, since "no decision" also produces silence but for the wrong
/// reason.
#[tokio::test]
async fn openvpn_rejects_peer_outside_allow_list() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "OpenVPN",
        "You are an OpenVPN control-plane responder with an allow list: only \
         clients from 192.0.2.0/24 may be answered. Explicitly refuse every \
         session reset from any other source address.",
        "openvpn_peer_reset",
        json!({
            "peer_addr": "203.0.113.66:1194",
            "client_session_id": "fedcba9876543210",
            "key_id": 0,
            "reset_type": "PControlHardResetClientV2",
            "packet_id": 0,
            "peer_count": 0
        }),
    )
    .expect_action("reject_peer")
    .run()
    .await
}

/// IPsec is receive-only: an IKE_SA_INIT from a scanner is classified, and the
/// classification must carry the analyst's reasoning.
#[tokio::test]
async fn ipsec_handshake_is_classified() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "IPSec/IKEv2",
        "You are an IPsec honeypot. You never reply on the wire and you never \
         classify an attempt as accepted or rejected — you only record what \
         each IKE attempt looks like. Log every handshake you observe with an \
         analyst note describing it.",
        "ipsec_handshake",
        json!({
            "peer_addr": "203.0.113.77:500",
            "packet_size": 328,
            "ike_version": "IKEv2",
            "exchange_type": "IKE_SA_INIT",
            "initiator_spi": "0011223344556677",
            "responder_spi": "0000000000000000",
            "is_initiator": true,
            "is_response": false,
            "message_id": 0,
            "payloads": ["SA", "KE", "NONCE", "NOTIFY"],
            "honeypot_mode": true,
            "responds_to_peer": false,
            "analysis": {
                "expected_payloads": "SA, KE, NONCE",
                "has_encryption": false,
                "has_vendor_id": false,
                "has_certificate": false
            }
        }),
    )
    .expect_action("log_handshake")
    .check(ParamCheck::non_empty("details"))
    .run()
    .await
}
