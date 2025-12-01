//! WHOIS client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// WHOIS client connected event
pub static WHOIS_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "whois_connected",
        "WHOIS client successfully connected to server",
        json!({}),
    )
    .with_parameters(vec![Parameter {
        name: "remote_addr".to_string(),
        type_hint: "string".to_string(),
        description: "WHOIS server address".to_string(),
        required: true,
    }])
});

/// WHOIS client response received event
pub static WHOIS_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "whois_response_received",
        "Response received from WHOIS server",
        json!({}),
    )
    .with_parameters(vec![
        Parameter {
            name: "response".to_string(),
            type_hint: "string".to_string(),
            description: "The WHOIS response text".to_string(),
            required: true,
        },
        Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "The original query (domain or IP)".to_string(),
            required: true,
        },
    ])
});

/// WHOIS client protocol action handler
pub struct WhoisClientProtocol;

impl Default for WhoisClientProtocol {
    fn default() -> Self {
        Self
    }
}

impl WhoisClientProtocol {
    pub fn new() -> Self {
        Self::default()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for WhoisClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "query_whois".to_string(),
                description: "Query WHOIS information for a domain or IP address".to_string(),
                parameters: vec![Parameter {
                    name: "query".to_string(),
                    type_hint: "string".to_string(),
                    description:
                        "Domain name or IP address to query (e.g., 'example.com' or '8.8.8.8')"
                            .to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "query_whois",
                    "query": "example.com"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the WHOIS server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        // WHOIS is a simple request-response protocol
        // No sync actions needed (no response to responses)
        vec![]
    }
    fn protocol_name(&self) -> &'static str {
        "WHOIS"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("whois_connected", "Triggered when WHOIS client connects to server", json!({"type": "placeholder", "event_id": "whois_connected"})),
            EventType::new("whois_response_received", "Triggered when WHOIS client receives a response", json!({"type": "placeholder", "event_id": "whois_response_received"})),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>WHOIS"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["whois", "whois client", "domain lookup", "ip lookup"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Direct TCP to port 43 with text protocol")
            .llm_control("Full control over WHOIS queries and response parsing")
            .e2e_testing("Public WHOIS servers (whois.iana.org, etc.)")
            .build()
    }
    fn description(&self) -> &'static str {
        "WHOIS client for domain and IP address lookups"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to WHOIS at whois.iana.org:43 and query 'example.com'"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM controls WHOIS queries
            json!({
                "type": "open_client",
                "remote_addr": "whois.verisign-grs.com:43",
                "base_stack": "whois",
                "instruction": "Query example.com and extract the registrar, creation date, and expiration date"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_client",
                "remote_addr": "whois.iana.org:43",
                "base_stack": "whois",
                "event_handlers": [{
                    "event_pattern": "whois_response_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<whois_client_handler>"
                    }
                }]
            }),
            // Static mode: Fixed WHOIS query on connect
            json!({
                "type": "open_client",
                "remote_addr": "whois.verisign-grs.com:43",
                "base_stack": "whois",
                "event_handlers": [
                    {
                        "event_pattern": "whois_connected",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "query_whois",
                                "query": "example.com"
                            }]
                        }
                    },
                    {
                        "event_pattern": "whois_response_received",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "disconnect"
                            }]
                        }
                    }
                ]
            }),
        )
    }
}

// Implement Client trait (client-specific functionality)
impl Client for WhoisClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::whois::WhoisClient;
            WhoisClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "query_whois" => {
                let query = action
                    .get("query")
                    .and_then(|v| v.as_str())
                    .context("Missing 'query' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "whois_query".to_string(),
                    data: json!({
                        "query": query,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!(
                "Unknown WHOIS client action: {}",
                action_type
            )),
        }
    }
}
