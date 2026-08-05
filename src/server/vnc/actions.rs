//! VNC protocol actions.
//!
//! **The protocol is marked `DevelopmentState::Incomplete` and is therefore hidden from the
//! LLM.** The RFB implementation in `src/server/vnc/mod.rs` never consults the model:
//! `VncServer::spawn_with_llm_actions` takes the `OllamaClient` as `_llm_client` and drops
//! it, no `Event` is ever constructed, `call_llm` is never called, and `get_event_types()`
//! returns the empty trait default. Every `FramebufferUpdateRequest` is answered with a
//! hardcoded red/green gradient (`send_test_framebuffer`), whatever the user's instruction
//! said. Key and pointer events are logged and dropped.
//!
//! Five actions used to be declared here — `vnc_auth_success`, `vnc_auth_deny`,
//! `vnc_render_display`, `send_framebuffer_update` and `disconnect_vnc_client`. All five were
//! unreachable: no event advertised them (there are no events), and the connection loop never
//! calls `execute_action`, so even a static handler naming one would have changed nothing on
//! the wire. `vnc_render_display` in particular promised the model control of the display and
//! returned `NoAction`. They are removed rather than left as a promise the code does not keep.
//!
//! The rendering half of the feature does exist and works: `crate::display::DisplayCanvas`
//! renders a `Vec<DisplayCommand>` to an RGBA buffer, and
//! `VncServer::send_framebuffer_update` already ships such a buffer as a Raw-encoded
//! rectangle. What is missing is the wiring — an `EventType` for
//! `vnc_framebuffer_update_request` advertising a render action via `.with_actions(...)`, a
//! `call_llm` in the message loop, and deserialization of the returned commands into
//! `DisplayCommand`. See `src/server/vnc/CLAUDE.md`.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition,
};
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::Value as JsonValue;

/// VNC protocol implementation
#[derive(Clone)]
pub struct VncProtocol;

impl VncProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for VncProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for VncProtocol {
    fn protocol_name(&self) -> &'static str {
        "VNC"
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>VNC"
    }
    fn description(&self) -> &'static str {
        "VNC remote desktop server (Incomplete: displays a fixed test pattern, not LLM-controlled)"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a VNC server on port 5900"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["vnc", "rfb", "remote desktop", "framebuffer"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            // Incomplete, deliberately: `is_available_to_llm()` returns false, so the model is
            // not offered a remote desktop whose contents it cannot influence.
            .state(DevelopmentState::Incomplete)
            .implementation("Manual RFB 3.8, Raw encoding, tiny-skia display canvas")
            .llm_control(
                "NONE - the LLM client is dropped on startup, no event is raised, and every \
                 framebuffer request is answered with a hardcoded gradient",
            )
            .e2e_testing("custom RFB client in tests/server/vnc/test.rs")
            .notes(
                "Fixed 800x600 framebuffer regardless of the requested size; 'None' security \
                 only (no VNC-Auth, so any client is admitted); Raw encoding only; the \
                 client's SetPixelFormat is read and ignored, so a client that asks for \
                 anything other than 32bpp BGRX gets garbled pixels; key/pointer events and \
                 clipboard text are logged and discarded.",
            )
            .build()
    }
    fn group_name(&self) -> &'static str {
        "Network Services"
    }

    /// Structurally valid examples are mandatory (`tests/startup_examples_validation_test.rs`
    /// requires a script handler and a static handler), but VNC declares no event types, so no
    /// `event_pattern` here can match and no handler below can fire.
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            json!({
                "type": "open_server",
                "port": 5900,
                "base_stack": "vnc",
                "instruction": "NOTE: vnc ignores this instruction entirely - clients always \
                                see a fixed gradient test pattern"
            }),
            json!({
                "type": "open_server",
                "port": 5900,
                "base_stack": "vnc",
                "event_handlers": [{
                    "event_pattern": "vnc_framebuffer_update_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "# never runs: vnc raises no events"
                    }
                }]
            }),
            json!({
                "type": "open_server",
                "port": 5900,
                "base_stack": "vnc",
                "event_handlers": [{
                    "event_pattern": "vnc_framebuffer_update_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "show_message",
                            "message": "never runs: vnc raises no events"
                        }]
                    }
                }]
            }),
        )
    }

    /// No async actions: the connection loop has no path that would execute one.
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }

    /// No sync actions: the server raises no events, so no action could ever be offered.
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        Vec::new()
    }
}

// Implement Server trait (server-specific functionality)
impl Server for VncProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::vnc::VncServer;
            VncServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }

    /// VNC has no actions. Anything routed here is a caller bug, so report it rather than
    /// returning `NoAction` and letting the caller believe the display changed.
    fn execute_action(&self, action: JsonValue) -> Result<ActionResult> {
        let action_type = action["type"].as_str().unwrap_or("<missing type>");
        Err(anyhow!(
            "VNC declares no actions (the protocol is Incomplete: the framebuffer is a fixed \
             test pattern and the LLM is never consulted); refusing action '{}'",
            action_type
        ))
    }
}
