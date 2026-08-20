//! Live-LLM remote-access suite (event-level): VNC, RDP, Tor relay.
//!
//! VNC and RDP could in principle be driven over a socket, but only past a
//! full RFB/X.224 handshake and (for RDP) a TLS negotiation with a real
//! client; Tor needs a genuine onion-routing peer. In all three the model's
//! decision is transport-independent, so it is graded directly.
//!
//! Protocol facts these cases encode:
//! - **VNC (RFB)**: the framebuffer is redrawn *from scratch* on every
//!   `vnc_render_display` — the command list is the entire screen, not a
//!   delta, so a reply that draws only the changed text erases everything
//!   else. Drawing must stay inside the framebuffer the client asked about.
//!   And a framebuffer update request that should change nothing has its own
//!   answer (`vnc_no_change`); re-rendering identical content wastes a frame.
//! - **VNC clipboard**: `vnc_client_cut_text` is the client pushing *its*
//!   clipboard to us. Sending it straight back is the wrong shape of answer
//!   only if it ignores the content, so the case grades that the reply is
//!   derived from what was pasted.
//! - **RDP (MS-RDPBCGR)**: the X.224 Connection Request offers a *set* of
//!   security protocols and the server selects exactly one **of those
//!   offered**. Selecting something the client did not offer, or accepting
//!   when nothing acceptable was offered, breaks the connection — the refusal
//!   path is a typed failure code, not silence.
//! - **Tor**: this relay implements no RELAY command forwarding. An EXTEND it
//!   cannot honour must be recorded, and a circuit it wants gone is torn down
//!   with a DESTROY naming that circuit id.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// Every command in a render must be one the server can execute, and must sit
/// inside the framebuffer.
fn commands_are_renderable(width: i64, height: i64) -> ParamCheck {
    ParamCheck::custom(
        "commands",
        "known command types, drawn inside the framebuffer",
        move |v| {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("commands must be an array, got {}", v))?;
            if arr.is_empty() {
                return Err("the command list is the whole screen; an empty one draws \
                            nothing"
                    .to_string());
            }
            for c in arr {
                let ty = c
                    .get("type")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| format!("command has no type: {}", c))?;
                if !matches!(
                    ty,
                    "background" | "rectangle" | "rect" | "text" | "line" | "circle"
                ) {
                    return Err(format!(
                        "unknown drawing command {:?}; the server executes background, \
                         rectangle, text, line and circle",
                        ty
                    ));
                }
                for axis in ["x", "y"] {
                    if let Some(n) = c.get(axis).and_then(|n| n.as_i64()) {
                        let bound = if axis == "x" { width } else { height };
                        if n < 0 || n > bound {
                            return Err(format!(
                                "{} = {} falls outside the {}x{} framebuffer the client \
                                 asked about",
                                axis, n, width, height
                            ));
                        }
                    }
                }
            }
            Ok(())
        },
    )
}

// ---------------------------------------------------------------------------
// VNC
// ---------------------------------------------------------------------------

/// The first frame: the whole screen has to be drawn, background included.
#[tokio::test]
async fn vnc_first_frame_draws_the_whole_screen() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "VNC",
        "You are a VNC server showing a status board. The screen has a dark \
         background with the heading NETGET-STATUS near the top left.",
        "vnc_framebuffer_update_request",
        json!({
            "width": 800,
            "height": 600,
            "first_request": true,
            "peer_addr": "203.0.113.70:54000",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("vnc_render_display")
    .check(commands_are_renderable(800, 600))
    .check(ParamCheck::custom(
        "commands",
        "includes the instructed heading",
        |v| {
            let flat = v.to_string().to_uppercase();
            if flat.contains("NETGET-STATUS") {
                Ok(())
            } else {
                Err(format!(
                    "the screen must show the instructed heading NETGET-STATUS, got {}",
                    v
                ))
            }
        },
    ))
    .check_action(|a| {
        // The framebuffer is redrawn from scratch: without a background the
        // previous frame shows through wherever nothing was drawn.
        let has_bg = a
            .get("commands")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter().any(|c| {
                    matches!(
                        c.get("type").and_then(|t| t.as_str()),
                        Some("background") | Some("rectangle") | Some("rect")
                    )
                })
            })
            .unwrap_or(false);
        if has_bg {
            Ok(())
        } else {
            Err(
                "the framebuffer is redrawn from scratch, so a full-screen render must \
                 lay down a background first"
                    .to_string(),
            )
        }
    })
    .run()
    .await
}

/// A keystroke the screen is supposed to react to: the redraw must contain
/// the new state *and* still be a complete screen.
#[tokio::test]
async fn vnc_key_event_redraws_the_full_screen() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "VNC",
        "You are a VNC server showing a keyboard tester. The screen has a \
         dark background, the fixed heading KEY-TESTER, and below it a line \
         showing the last key the user pressed.",
        "vnc_key_event",
        json!({
            "down": true,
            "keysym": 113,
            "key": "q",
            "peer_addr": "203.0.113.70:54000",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("vnc_render_display")
    .check(commands_are_renderable(800, 600))
    .check_action(|a| {
        // Both, because the render replaces the entire framebuffer: showing
        // only the new key would wipe the heading off the screen.
        if !a.to_string().to_uppercase().contains("KEY-TESTER") {
            return Err(format!(
                "the render replaces the whole framebuffer, so the fixed heading \
                 KEY-TESTER must be redrawn too; got {}",
                a
            ));
        }
        // Look for a standalone `q` inside the drawn text only — scanning the
        // whole JSON would match the `q` in any key name the model invented.
        let shows_key = a
            .get("commands")
            .and_then(|c| c.as_array())
            .map(|cmds| {
                cmds.iter()
                    .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                    .any(|t| {
                        t.to_lowercase()
                            .split(|c: char| !c.is_alphanumeric())
                            .any(|w| w == "q")
                    })
            })
            .unwrap_or(false);
        if !shows_key {
            return Err(format!(
                "the screen should show the key that was pressed (q) in its drawn text; \
                 got {}",
                a
            ));
        }
        Ok(())
    })
    .run()
    .await
}

/// A pointer event on a screen with nothing clickable should not burn a
/// frame: the protocol has an explicit "nothing changed" answer.
#[tokio::test]
async fn vnc_pointer_on_static_screen_reports_no_change() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "VNC",
        "You are a VNC server showing a fixed, entirely static notice board. \
         Nothing on the screen reacts to the mouse in any way, and the screen \
         never changes.",
        "vnc_pointer_event",
        json!({
            "x": 410,
            "y": 322,
            "pressed": true,
            "buttons": ["left"],
            "button_mask": 1,
            "peer_addr": "203.0.113.70:54000",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("vnc_no_change")
    .run()
    .await
}

/// The client pushed its clipboard to us. Acknowledging by setting the
/// clipboard is right; ignoring what was pasted is not.
#[tokio::test]
async fn vnc_client_cut_text_is_acknowledged_with_its_content() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "VNC",
        "You are a VNC server with a clipboard bridge. When the client copies \
         text to you, put it back on the client's clipboard in upper case so \
         the user can see it round-tripped.",
        "vnc_client_cut_text",
        json!({
            "text": "netget-clip-7431",
            "peer_addr": "203.0.113.70:54000",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("vnc_set_clipboard")
    .check(ParamCheck::custom(
        "text",
        "derived from the text the client pasted",
        |v| {
            let s = v.as_str().unwrap_or("");
            if s.to_lowercase().contains("netget-clip-7431") {
                Ok(())
            } else {
                Err(format!(
                    "the reply must be built from what the client pasted \
                     (netget-clip-7431), got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// RDP
// ---------------------------------------------------------------------------

/// The server selects one of the protocols the client offered. Picking
/// something that was not on the list breaks the connection.
#[tokio::test]
async fn rdp_selects_a_protocol_the_client_offered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RDP",
        "You are an RDP server that requires network level authentication \
         (CredSSP/NLA). Negotiate that whenever the client supports it.",
        "rdp_connection_request",
        json!({
            "cookie_username": "administrator",
            "requested_protocols": ["TLS", "HYBRID"],
            "requested_protocols_flags": 3,
            "peer_addr": "203.0.113.80:49500",
            "connection_id": "conn-1"
        }),
    )
    .expect_action("send_rdp_negotiation_response")
    .check(ParamCheck::custom(
        "selected_protocol",
        "HYBRID (CredSSP/NLA), which the client offered",
        |v| {
            let s = v.as_str().unwrap_or("").to_uppercase();
            if s == "HYBRID" || s == "HYBRID_EX" {
                Ok(())
            } else if s == "TLS" || s == "RDP" {
                Err(format!(
                    "NLA was required; {:?} negotiates away from CredSSP",
                    v
                ))
            } else {
                Err(format!(
                    "must select one of the protocols the client offered \
                     ([\"TLS\", \"HYBRID\"]), got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// When the client offers nothing the server will accept, the refusal is a
/// typed failure code — an RDP client shows the user the reason.
#[tokio::test]
async fn rdp_refuses_plain_rdp_security_with_a_failure_code() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RDP",
        "You are an RDP server that requires network level authentication \
         (CredSSP/NLA) and will not accept any connection without it.",
        "rdp_connection_request",
        json!({
            "cookie_username": "",
            "requested_protocols": ["RDP"],
            "requested_protocols_flags": 0,
            "peer_addr": "203.0.113.80:49501",
            "connection_id": "conn-2"
        }),
    )
    .expect_action("reject_rdp_connection")
    .check(ParamCheck::custom(
        "failure_code",
        "HYBRID_REQUIRED_BY_SERVER — the code that names this exact refusal",
        |v| {
            let s = v.as_str().unwrap_or("").to_uppercase();
            if s == "HYBRID_REQUIRED_BY_SERVER" {
                Ok(())
            } else if [
                "SSL_REQUIRED_BY_SERVER",
                "SSL_NOT_ALLOWED_BY_SERVER",
                "SSL_CERT_NOT_ON_SERVER",
                "INCONSISTENT_FLAGS",
                "SSL_WITH_USER_AUTH_REQUIRED_BY_SERVER",
            ]
            .contains(&s.as_str())
            {
                Err(format!(
                    "refused for the wrong reason: NLA was required, so the code is \
                     HYBRID_REQUIRED_BY_SERVER, got {:?}",
                    v
                ))
            } else {
                Err(format!(
                    "failure_code must be one of the RDP_NEG_FAILURE codes, got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Tor relay
// ---------------------------------------------------------------------------

/// A circuit came up. There is nothing to send back — the correct answer is
/// a record naming the circuit.
#[tokio::test]
async fn tor_circuit_created_is_recorded() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Tor Relay",
        "You are a Tor middle relay. Keep a note of every circuit that is \
         built through you, including its circuit id.",
        "tor_relay_circuit_created",
        json!({
            "circuit_id": "0x00000005",
            "client_ip": "203.0.113.90"
        }),
    )
    .expect_action("detect_relay_cell")
    .or_action("append_to_log")
    .or_action("append_memory")
    .check_action(|a| {
        // The circuit id as the event spelled it, or the bare number it
        // denotes — but not merely "a 5 appears somewhere in the JSON".
        let flat = a.to_string().to_lowercase();
        let named = flat.contains("0x00000005")
            || flat.contains("circuit 5")
            || flat.contains("circuit_id\":\"5")
            || flat.contains("circuit id 5")
            || flat.contains("circuit 0x5");
        if named {
            Ok(())
        } else {
            Err(format!(
                "the note should name the circuit that was built (0x00000005), or a \
                 later note about the same circuit cannot be tied to it; got {}",
                a
            ))
        }
    })
    .run()
    .await
}

/// This relay does not forward RELAY commands. A circuit trying to EXTEND
/// through it is going nowhere, so tearing it down is honest — and DESTROY
/// names the circuit it is destroying.
#[tokio::test]
async fn tor_unsupported_extend_destroys_the_circuit() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Tor Relay",
        "You are a Tor relay that cannot extend circuits any further — you \
         have no onward connections at all. When a circuit asks you to \
         extend, tear that circuit down instead of leaving the client \
         waiting.",
        "tor_relay_relay_cell",
        json!({
            "circuit_id": "0x00000005",
            "relay_command": "EXTEND",
            "stream_id": 0,
            "length": 78,
            "client_ip": "203.0.113.90"
        }),
    )
    .expect_action("send_destroy")
    .check(ParamCheck::custom(
        "circuit_id",
        "names the circuit from the event",
        |v| {
            let s = v.as_str().map(|s| s.to_lowercase()).unwrap_or_else(|| {
                v.as_u64()
                    .map(|n| format!("0x{:08x}", n))
                    .unwrap_or_default()
            });
            if s.trim_start_matches("0x").trim_start_matches('0') == "5" {
                Ok(())
            } else {
                Err(format!(
                    "must destroy the circuit the cell arrived on (0x00000005), got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}
