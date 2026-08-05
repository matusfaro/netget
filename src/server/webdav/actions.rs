//! WebDAV protocol actions.
//!
//! **The protocol is marked `DevelopmentState::Incomplete` and is therefore hidden from
//! the LLM.** Two independent reasons, both structural rather than cosmetic:
//!
//! 1. **No LLM integration at all.** `WebDavServer::spawn_with_llm_actions` takes the
//!    `OllamaClient` as `_llm_client` and drops it. No `Event` is ever constructed, no
//!    `EventType` is declared, `call_llm` is never called, and `get_event_types()` returns
//!    the empty default. The server instruction a user writes is read by nobody.
//! 2. **It serves a real in-process filesystem.** Every request is answered by
//!    `dav_server::memfs::MemFs`, a live read/write filesystem living in the process. That
//!    is exactly the storage a protocol is forbidden to implement: the model is supposed to
//!    supply every file and directory listing, and here it supplies none of them.
//!
//! The six actions this file used to declare (`read_file`, `create_file`, `create_directory`,
//! `delete_resource`, `list_directory`, `get_properties`) were removed. None of them was
//! reachable — no event advertised them — and every executor arm returned
//! `ActionResult::NoAction` after parsing and discarding its `path`, so even a hand-written
//! static handler naming one would have changed nothing on the wire.
//!
//! Making this protocol real means implementing `dav_server::fs::DavFileSystem` against the
//! LLM the way `src/server/nfs/` implements `NFSFileSystem`, and deleting `MemFs`. That is
//! noted as future work, not attempted here.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition,
};
use crate::state::app_state::AppState;
use anyhow::Result;

/// WebDAV protocol action handler
pub struct WebDavProtocol;

impl WebDavProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebDavProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for WebDavProtocol {
    /// No async actions: nothing the LLM could say would reach the MemFs that answers requests.
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }

    /// No sync actions: the server raises no events, so no action could ever be offered.
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        Vec::new()
    }

    fn protocol_name(&self) -> &'static str {
        "WebDAV"
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>WEBDAV"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["webdav", "dav"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            // Incomplete, deliberately: `is_available_to_llm()` returns false so the model is
            // never offered a protocol whose instruction it cannot influence. See the module
            // docs above for the two reasons.
            .state(DevelopmentState::Incomplete)
            .implementation("dav-server v0.8 DavHandler over an in-process MemFs")
            .llm_control(
                "NONE - the LLM client is dropped on startup, no event is raised and no \
                 action reaches the wire",
            )
            .e2e_testing("curl -X PROPFIND / cadaver - exercises MemFs, never the model")
            .notes(
                "Serves a real in-memory read/write filesystem (MemFs) instead of asking the \
                 LLM, which is the storage a protocol must not implement. Files persist for \
                 the life of the server and are lost on restart. No authentication, no TLS, \
                 locks accepted but never enforced (FakeLs).",
            )
            .build()
    }
    fn description(&self) -> &'static str {
        "WebDAV file server (Incomplete: serves an in-memory filesystem, not LLM-controlled)"
    }
    fn example_prompt(&self) -> &'static str {
        "Start a WebDAV server on port 8080"
    }
    fn group_name(&self) -> &'static str {
        "Web & File"
    }

    /// Structurally valid examples are mandatory (`tests/startup_examples_validation_test.rs`
    /// requires a script handler and a static handler), but WebDAV declares no event types, so
    /// no `event_pattern` written here can ever match and no handler below can ever fire. They
    /// are kept only to satisfy the validator; the protocol is hidden from the LLM regardless.
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "webdav",
                "instruction": "NOTE: webdav ignores this instruction entirely - requests are \
                                answered by an in-process MemFs, never by the model"
            }),
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "webdav",
                "event_handlers": [{
                    "event_pattern": "webdav_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "# never runs: webdav raises no events"
                    }
                }]
            }),
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "webdav",
                "event_handlers": [{
                    "event_pattern": "webdav_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "show_message",
                            "message": "never runs: webdav raises no events"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for WebDavProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::webdav::WebDavServer;
            WebDavServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }

    /// WebDAV has no actions. Anything routed here is a caller bug, so say so rather than
    /// returning `NoAction` and letting the caller believe the request was served.
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing type>");
        Err(anyhow::anyhow!(
            "WebDAV declares no actions (the protocol is Incomplete: requests are answered by \
             an in-process MemFs, not by the LLM); refusing action '{}'",
            action_type
        ))
    }
}
