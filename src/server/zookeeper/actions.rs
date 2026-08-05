//! ZooKeeper server protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// The single response action. Declared here rather than inline so the event type and
/// `get_sync_actions()` hand the model the same definition.
fn zookeeper_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "zookeeper_response".to_string(),
        description: "Send a ZooKeeper reply. The reply header (xid, zxid, error code) is built \
                      for you; `data_hex` is the Jute-serialized reply body, which you must \
                      encode by hand — see the known limitation in src/server/zookeeper/CLAUDE.md."
            .to_string(),
        parameters: vec![
            Parameter {
                name: "xid".to_string(),
                type_hint: "integer".to_string(),
                description: "Transaction ID. Must be the `xid` of the request being answered — \
                              a ZooKeeper client matches replies to requests by this value and \
                              will hang or desynchronize if it does not match. Omit it and the \
                              request's own xid is used."
                    .to_string(),
                required: false,
            },
            Parameter {
                name: "zxid".to_string(),
                type_hint: "integer".to_string(),
                description: "ZooKeeper transaction ID (server-assigned change counter)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "error_code".to_string(),
                type_hint: "integer".to_string(),
                description: "Error code (0 = OK, -101 = NONODE, -110 = NODEEXISTS, -102 = NOAUTH)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "data_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Jute-serialized reply body, hex encoded. Omit or leave empty for \
                              operations whose reply carries no body."
                    .to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "zookeeper_response",
            "xid": 1,
            "zxid": 100,
            "error_code": 0,
            "data_hex": "0000000000000064"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> ZooKeeper response (err={error_code})")
                .with_debug(
                    "ZooKeeper zookeeper_response: xid={xid} zxid={zxid} error_code={error_code}",
                ),
        ),
    }
}

// Event type constants
pub static ZOOKEEPER_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "zookeeper_request",
        "ZooKeeper client sent a request (create, delete, getData, setData, etc.)",
        json!({
            "type": "zookeeper_response",
            "xid": "{{event.xid}}",
            "zxid": 100,
            "error_code": 0,
            "data_hex": "0000000000000064"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "xid".to_string(),
            type_hint: "integer".to_string(),
            description: "Request transaction ID. Echo this back as the response `xid` — a \
                          static handler can do so with {{event.xid}}."
                .to_string(),
            required: true,
        },
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "Operation type (create, delete, getData, setData, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "op_code".to_string(),
            type_hint: "integer".to_string(),
            description: "Numeric ZooKeeper opcode, for operations with no name mapping"
                .to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "ZNode path (e.g., /myapp/config). Empty when the request carries no path."
                .to_string(),
            required: false,
        },
    ])
    .with_actions(vec![zookeeper_response_action()])
    .with_log_template(
        LogTemplate::new()
            .with_info("ZooKeeper {operation} {path}")
            .with_debug("ZooKeeper request xid={xid} op={operation} path={path}")
            .with_trace("ZooKeeper request: {json_pretty(.)}"),
    )
});

/// ZooKeeper protocol implementation
pub struct ZookeeperProtocol;

impl ZookeeperProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for ZookeeperProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![zookeeper_response_action()]
    }

    fn protocol_name(&self) -> &'static str {
        "ZooKeeper"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![(*ZOOKEEPER_REQUEST_EVENT).clone()]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>ZooKeeper"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["zookeeper", "zk"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .implementation(
                "Hand-rolled parsing of the ZooKeeper request header only. No session \
                 handshake: a client's opening ConnectRequest is misread as an ordinary \
                 request and no ConnectResponse is ever produced, so no real ZooKeeper \
                 client can establish a session.",
            )
            .llm_control(
                "Reply header (xid, zxid, error code) only. The reply body must be \
                 hand-encoded as Jute hex by the model, which it cannot do reliably.",
            )
            .e2e_testing(
                "Hand-built byte sequences over a raw TcpStream. No real ZooKeeper client \
                 has ever completed a session against this server.",
            )
            .notes(
                "INCOMPLETE - hidden from the LLM. See src/server/zookeeper/CLAUDE.md for \
                 the route back to Experimental. No storage: the model answers every request.",
            )
            .build()
    }

    fn description(&self) -> &'static str {
        "ZooKeeper distributed coordination server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a ZooKeeper server on port 2181"
    }

    fn group_name(&self) -> &'static str {
        "Database"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles all ZooKeeper responses intelligently
            json!({
                "type": "open_server",
                "port": 2181,
                "base_stack": "zookeeper",
                "instruction": "ZooKeeper distributed coordination server handling znode operations"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_server",
                "port": 2181,
                "base_stack": "zookeeper",
                "event_handlers": [{
                    "event_pattern": "zookeeper_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<zookeeper_handler>"
                    }
                }]
            }),
            // Static mode: Fixed responses
            json!({
                "type": "open_server",
                "port": 2181,
                "base_stack": "zookeeper",
                "event_handlers": [{
                    "event_pattern": "zookeeper_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "zookeeper_response",
                            // Echo the request's xid; a literal would break correlation.
                            "xid": "{{event.xid}}",
                            "zxid": 1,
                            "error_code": 0,
                            "data_hex": ""
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for ZookeeperProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::zookeeper::ZookeeperServer;
            let send_first = ctx
                .startup_params
                .as_ref()
                .map(|p| p.get_optional_bool("send_first"))
                .transpose()?
                .flatten()
                .unwrap_or(false);

            ZookeeperServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                send_first,
                ctx.server_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "zookeeper_response" => {
                // `xid` is deliberately passed through as an Option rather than defaulted to 0
                // here: the connection loop knows the xid of the request being answered and
                // substitutes it when the handler omitted one. Defaulting to 0 at this layer
                // would put a reply on the wire that no client can correlate.
                let xid = action.get("xid").and_then(|v| v.as_i64()).map(|v| v as i32);
                let zxid = action.get("zxid").and_then(|v| v.as_i64()).unwrap_or(0);
                let error_code = action
                    .get("error_code")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;

                // Reject invalid hex loudly instead of silently sending a body-less reply:
                // a truncated body desynchronizes the client for the rest of the connection.
                let body = match action.get("data_hex").and_then(|v| v.as_str()) {
                    Some(s) if !s.is_empty() => hex::decode(s).map_err(|e| {
                        anyhow!("zookeeper_response: 'data_hex' is not valid hex: {}", e)
                    })?,
                    _ => Vec::new(),
                };

                Ok(ActionResult::Custom {
                    name: "zookeeper_response".to_string(),
                    data: json!({
                        "xid": xid,
                        "zxid": zxid,
                        "error_code": error_code,
                        "body_hex": hex::encode(&body),
                    }),
                })
            }
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
