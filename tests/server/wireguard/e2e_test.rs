//! Tests for the WireGuard VPN server.
//!
//! # What these tests actually validate, and why they look like this
//!
//! NetGet's WireGuard server is a *thin orchestration layer* over
//! `defguard_wireguard_rs`. NetGet implements **none** of the WireGuard protocol
//! itself - no Noise_IK handshake, no ChaCha20-Poly1305, no packet parsing. All
//! of that lives in the platform backend: the kernel module on Linux, or the
//! **external `wireguard-go` binary** on macOS (defguard literally shells out to
//! `Command::new("wireguard-go")`). Creating the interface therefore needs root
//! (and, on macOS, wireguard-go installed and in PATH).
//!
//! Consequences for testing:
//!
//! * There is **no NetGet-authored handshake/crypto code to drive** with real
//!   curve25519 keys - that layer simply does not exist in this repo.
//! * A real end-to-end handshake (a `wg` client, `wg-quick`, or an in-process
//!   `boringtun` peer) can only run against a *running* server, which needs root
//!   + a backend. It has never been run and cannot run in CI. It lives here only
//!   as a root-gated `#[ignore]`d harness that **fails loudly** rather than
//!   skipping-as-pass (see `test_wireguard_real_backend_startup`).
//! * `boringtun` was evaluated as an in-process real WireGuard peer. It is a
//!   genuine, independent implementation (BSD-3-Clause), but with the server
//!   unable to start here it has nothing to handshake against; a
//!   boringtun-against-boringtun test would validate Cloudflare's library, not
//!   NetGet, so it was deliberately NOT added.
//!
//! What we CAN validate without privilege is the code NetGet actually owns: the
//! action executors that implement the LLM's control surface
//! (`authorize_peer` / `reject_peer` / `disconnect_peer` /
//! `set_peer_traffic_limit`) and the event/action declarations that decide what
//! the model is even allowed to answer with. Those are covered below and run in
//! CI.
//!
//! The previous version of this file was fictional: it mocked a
//! `wireguard_packet_received` event and a `log_packet` action that **do not
//! exist** in the implementation (the server raises `wireguard_peer_connected`
//! and has no packet-sniffing honeypot path at all), and every test was
//! `#[ignore]`d behind root, so the mismatch never surfaced.

#![cfg(feature = "wireguard")]

use crate::server::helpers::*;
// `::netget::` (absolute extern-crate path) is required: `use crate::server::helpers::*`
// glob-imports the `helpers::netget` module, which would otherwise shadow the crate name.
use ::netget::llm::actions::protocol_trait::{ActionResult, Protocol, Server};
use ::netget::server::wireguard::actions::{WireguardProtocol, WIREGUARD_PEER_CONNECTED_EVENT};
use serde_json::json;

/// Decode an `ActionResult::Output(json_bytes)` back into a `serde_json::Value`.
///
/// The executors return their structured decision as JSON-encoded bytes in
/// `Output`; the server layer decodes and applies it. Any other variant is a
/// test failure with a descriptive message.
fn output_json(result: ActionResult) -> serde_json::Value {
    match result {
        ActionResult::Output(bytes) => {
            serde_json::from_slice(&bytes).expect("executor Output was not valid JSON")
        }
        other => panic!("expected ActionResult::Output, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// authorize_peer
// ---------------------------------------------------------------------------

#[test]
fn authorize_peer_returns_structured_authorization() {
    let proto = WireguardProtocol::new();
    let result = proto
        .execute_action(json!({
            "type": "authorize_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "allowed_ips": ["10.20.30.2/32"],
            "endpoint": "203.0.113.45:51820",
            "message": "ok"
        }))
        .expect("authorize_peer with valid params must succeed");

    let out = output_json(result);
    assert_eq!(out["action"], "authorize_peer");
    assert_eq!(
        out["public_key"],
        "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg="
    );
    assert_eq!(out["allowed_ips"], json!(["10.20.30.2/32"]));
    // Endpoint is echoed back in canonical SocketAddr form for the server to apply.
    assert_eq!(out["endpoint"], "203.0.113.45:51820");
    assert_eq!(out["message"], "ok");
}

#[test]
fn authorize_peer_endpoint_is_optional() {
    let proto = WireguardProtocol::new();
    let out = output_json(
        proto
            .execute_action(json!({
                "type": "authorize_peer",
                "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
                "allowed_ips": ["10.20.30.9/32"]
            }))
            .expect("authorize_peer without endpoint must succeed"),
    );
    assert_eq!(out["action"], "authorize_peer");
    assert!(
        out["endpoint"].is_null(),
        "omitted endpoint must serialize as null"
    );
    // Default message is applied when none is supplied.
    assert_eq!(out["message"], "Peer authorized");
}

#[test]
fn authorize_peer_rejects_missing_public_key() {
    let proto = WireguardProtocol::new();
    let err = proto
        .execute_action(json!({
            "type": "authorize_peer",
            "allowed_ips": ["10.20.30.2/32"]
        }))
        .expect_err("missing public_key must be an error, not a silent pass");
    assert!(
        err.to_string().contains("public_key"),
        "error should name the missing field, got: {err}"
    );
}

#[test]
fn authorize_peer_rejects_empty_allowed_ips() {
    let proto = WireguardProtocol::new();
    // An empty allowed_ips would configure a peer that can route nothing; the
    // executor must refuse it rather than push a useless peer to the interface.
    let err = proto
        .execute_action(json!({
            "type": "authorize_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "allowed_ips": []
        }))
        .expect_err("empty allowed_ips must be an error");
    assert!(
        err.to_string().contains("allowed_ips"),
        "error should name allowed_ips, got: {err}"
    );
}

#[test]
fn authorize_peer_rejects_missing_allowed_ips() {
    let proto = WireguardProtocol::new();
    proto
        .execute_action(json!({
            "type": "authorize_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg="
        }))
        .expect_err("missing allowed_ips must be an error");
}

// ---------------------------------------------------------------------------
// disconnect_peer
// ---------------------------------------------------------------------------

#[test]
fn disconnect_peer_returns_structured_disconnect() {
    let proto = WireguardProtocol::new();
    let out = output_json(
        proto
            .execute_action(json!({
                "type": "disconnect_peer",
                "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
                "reason": "suspicious traffic"
            }))
            .expect("disconnect_peer with valid params must succeed"),
    );
    assert_eq!(out["action"], "disconnect_peer");
    assert_eq!(
        out["public_key"],
        "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg="
    );
    assert_eq!(out["reason"], "suspicious traffic");
}

#[test]
fn disconnect_peer_defaults_reason_and_requires_public_key() {
    let proto = WireguardProtocol::new();

    // Default reason when omitted.
    let out = output_json(
        proto
            .execute_action(json!({
                "type": "disconnect_peer",
                "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg="
            }))
            .expect("disconnect_peer without reason must succeed"),
    );
    assert_eq!(out["reason"], "Disconnected by admin");

    // But public_key is mandatory - there is nothing to disconnect without it.
    proto
        .execute_action(json!({ "type": "disconnect_peer" }))
        .expect_err("disconnect_peer without public_key must be an error");
}

// ---------------------------------------------------------------------------
// reject_peer / set_peer_traffic_limit (documented no-ops)
// ---------------------------------------------------------------------------

#[test]
fn reject_peer_is_a_noop_result() {
    // reject_peer's interface effect (peer removal) is applied by the server
    // loop, not the stateless executor; the executor itself is a NoAction. This
    // pins that documented behavior so it is not "fixed" into an Output later.
    let proto = WireguardProtocol::new();
    let result = proto
        .execute_action(json!({
            "type": "reject_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "reason": "unknown key"
        }))
        .expect("reject_peer must succeed");
    assert!(matches!(result, ActionResult::NoAction));
}

#[test]
fn set_peer_traffic_limit_is_unenforced_noop() {
    // set_peer_traffic_limit is documented as NOT enforced (no tc/iptables). It
    // must resolve to NoAction so nothing pretends a limit was applied.
    let proto = WireguardProtocol::new();
    let result = proto
        .execute_action(json!({
            "type": "set_peer_traffic_limit",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "limit_mbps": 100
        }))
        .expect("set_peer_traffic_limit must succeed");
    assert!(matches!(result, ActionResult::NoAction));
}

// ---------------------------------------------------------------------------
// dispatch errors
// ---------------------------------------------------------------------------

#[test]
fn unknown_action_is_rejected() {
    let proto = WireguardProtocol::new();
    proto
        .execute_action(json!({ "type": "not_a_real_action" }))
        .expect_err("unknown action type must error");
}

#[test]
fn missing_type_is_rejected() {
    let proto = WireguardProtocol::new();
    proto
        .execute_action(json!({ "public_key": "x" }))
        .expect_err("action without a 'type' must error");
}

// ---------------------------------------------------------------------------
// event / action declaration integrity
//
// call_llm builds the model's tool list from EventType.actions, NOT from
// get_sync_actions(). These tests guard that the peer-connected event advertises
// exactly the interface-changing actions and does not advertise the unenforced
// traffic-limit no-op (which would promise enforcement that never happens).
// ---------------------------------------------------------------------------

#[test]
fn peer_connected_event_advertises_the_interface_changing_actions() {
    let action_names: Vec<&str> = WIREGUARD_PEER_CONNECTED_EVENT
        .actions
        .iter()
        .map(|a| a.name.as_str())
        .collect();

    assert!(
        action_names.contains(&"authorize_peer"),
        "peer_connected must offer authorize_peer, got {action_names:?}"
    );
    assert!(
        action_names.contains(&"reject_peer"),
        "peer_connected must offer reject_peer, got {action_names:?}"
    );
    assert!(
        action_names.contains(&"disconnect_peer"),
        "peer_connected must offer disconnect_peer, got {action_names:?}"
    );
    // The unenforced no-op must NOT be advertised on the event.
    assert!(
        !action_names.contains(&"set_peer_traffic_limit"),
        "set_peer_traffic_limit is unenforced and must not be advertised, got {action_names:?}"
    );
}

#[test]
fn protocol_exposes_no_user_triggered_actions() {
    // get_async_actions() is intentionally empty: the stateless protocol struct
    // holds no handle to the running server, so a user-triggered action could
    // not touch the interface. This also documents part of caveat (1) in
    // metadata: there is no way to PRE-add a peer before it handshakes.
    let proto = WireguardProtocol::new();
    let state = ::netget::state::app_state::AppState::new();
    assert!(
        proto.get_async_actions(&state).is_empty(),
        "WireGuard must advertise no user-triggered async actions"
    );
}

#[test]
fn metadata_is_beta_not_stable() {
    use ::netget::protocol::metadata::DevelopmentState;
    // Stable requires validation against a real client, which has never happened
    // for this protocol (root + a WireGuard backend required). It must not claim
    // Stable. If a future change earns Stable via a real interop test, update
    // this assertion together with the metadata and both CLAUDE.md files.
    let proto = WireguardProtocol::new();
    assert_eq!(
        proto.metadata().state,
        DevelopmentState::Beta,
        "WireGuard must be Beta until a real client completes a handshake against it"
    );
}

// ---------------------------------------------------------------------------
// Root-gated real-backend harness.
//
// This is the ONLY path that could exercise a real handshake, and it needs root
// plus a WireGuard backend (the kernel module, or the external `wireguard-go`
// binary on macOS). It is #[ignore]d so the normal suite never runs it, and when
// run with `--ignored` it FAILS LOUDLY if the interface cannot be created - it
// never skips-as-pass. That is deliberate: a green tick here must mean the real
// backend actually came up, not that the test quietly gave up.
//
// NOTE for whoever runs this with privilege: even once the interface is up, a
// real client will NOT drive the wireguard_peer_connected flow as-is. WireGuard
// responders drop a handshake whose static key is not already a configured peer,
// but NetGet only authorizes peers reactively after they appear - see caveat (1)
// in metadata(). Earning Stable requires fixing that (pre-registering the peer's
// public key), then asserting: the handshake response message, that a transport
// data packet authenticates and decrypts to the expected plaintext, and that a
// forged/replayed packet is rejected. A `boringtun` in-process initiator is the
// recommended driver for that work.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "Requires root + a WireGuard backend (kernel, or wireguard-go on macOS). \
             Run explicitly with --ignored under privilege; fails loudly otherwise."]
async fn test_wireguard_real_backend_startup() -> E2EResult<()> {
    // Drive NetGet's real spawn path (which internally uses defguard's backend)
    // and require that the interface actually comes up. Without root and/or a
    // backend, `create_interface` fails, NetGet's spawn returns Err, and
    // server_startup reports an error - which we detect and PANIC on. This never
    // skips-as-pass: it returns Ok only after confirming the real interface was
    // created.
    let config = NetGetConfig::new("Start a WireGuard VPN server on port 0").with_mock(|mock| {
        mock.on_instruction_containing("WireGuard")
            .respond_with_actions(json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "wireguard",
                    "instruction": "Accept VPN clients"
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let server = start_netget_server(config).await?;

    // The server subprocess logs "Interface created successfully" only when the
    // real backend brought the interface up. Anything else is a hard failure.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let created = server
        .output_contains("Interface created successfully")
        .await;
    let failed = server
        .output_contains("Failed to create WireGuard interface")
        .await
        || server
            .output_contains("Failed to create WireGuard API")
            .await;

    if !created || failed {
        let out = server.get_output().await;
        server.stop().await.ok();
        panic!(
            "WireGuard interface did not come up. This test requires root and a WireGuard \
             backend (on macOS, the external `wireguard-go` binary in PATH). It fails rather \
             than skipping.\n--- server output ---\n{}",
            out.join("\n")
        );
    }

    println!("✓ WireGuard interface created via the real backend");
    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}
