//! AMQP server protocol actions

use crate::llm::actions::protocol_trait::{ActionResult, Protocol};
use crate::llm::actions::{ActionDefinition, Server};
use crate::protocol::SpawnContext;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// AMQP server protocol
pub struct AmqpProtocol;

impl AmqpProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for AmqpProtocol {
    /// No actions: the server has no code path that would execute one.
    /// See `metadata()` and `src/server/amqp/CLAUDE.md`.
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    /// No actions: `AmqpServer` never calls the LLM, so no sync action could ever
    /// be requested, and `execute_action` rejects everything.
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "AMQP"
    }

    /// No events: the frame reader logs frames and discards them without ever
    /// constructing an `Event`, so nothing would fire a script or static handler.
    fn get_event_types(&self) -> Vec<crate::protocol::EventType> {
        vec![]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>AMQP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["amqp", "rabbitmq", "broker", "messaging", "queue"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .implementation("Stub AMQP 0.9.1 framing; no method encoder or decoder")
            .llm_control("None: no events are emitted and no actions exist")
            .e2e_testing("Connection setup only; no lapin client completes a handshake")
            .notes(
                "NOT FUNCTIONAL. The broker accepts TCP, checks the 8-byte AMQP protocol \
                 header and replies with a Connection.Start frame whose declared payload \
                 length (20) does not match the 31 bytes actually written, and whose body is \
                 not valid AMQP method encoding, so every conforming client fails the \
                 handshake immediately. Subsequent frames are read, logged and discarded: \
                 Connection.Tune, Connection.Open, Channel.Open, Queue.Declare, Basic.Publish \
                 and Basic.Consume are all unimplemented. No queue, exchange, binding or \
                 message exists. The LLM client handle is accepted and never used, so \
                 instructions, script handlers and static handlers have no effect. Hidden \
                 from the LLM until a real method codec exists.",
            )
            .build()
    }

    fn description(&self) -> &'static str {
        "AMQP 0.9.1 broker (incomplete: handshake stub only, no message queuing)"
    }

    fn example_prompt(&self) -> &'static str {
        "Not usable yet: the AMQP broker cannot complete a client handshake"
    }

    fn group_name(&self) -> &'static str {
        "Application"
    }
    /// All three modes are identical because none of them works: there is no event
    /// to match on and no action to run. The earlier examples advertised an
    /// `accept_connection` action that has never existed in this protocol.
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;
        let unusable = json!({
            "type": "open_server",
            "port": 5672,
            "base_stack": "amqp",
            "instruction": "NOT FUNCTIONAL: the AMQP broker cannot complete a client handshake \
                            and emits no events, so instructions, scripts and static handlers \
                            all have no effect. Do not start this protocol."
        });
        StartupExamples::new(unusable.clone(), unusable.clone(), unusable)
    }
}

// Implement Server trait (server-specific functionality)
impl Server for AmqpProtocol {
    fn spawn(
        &self,
        ctx: SpawnContext,
    ) -> Pin<Box<dyn Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::server::amqp::AmqpServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
            )
            .await
        })
    }

    /// Always an error: the protocol declares no actions, so reaching here means a
    /// caller invented one.
    fn execute_action(&self, action: Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Err(anyhow::anyhow!(
            "AMQP has no actions ('{}' does not exist). The AMQP server is incomplete: it \
             cannot complete a client handshake and never calls the LLM.",
            action_type
        ))
    }
}
