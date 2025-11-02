//! Tor Directory protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// Tor Directory protocol action handler
pub struct TorDirectoryProtocol;

impl TorDirectoryProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_serve_consensus(&self, action: serde_json::Value) -> Result<ActionResult> {
        let consensus_data = action
            .get("consensus_data")
            .and_then(|v| v.as_str())
            .context("Missing 'consensus_data' parameter")?;

        debug!("Tor Directory serving consensus ({} bytes before signing)", consensus_data.len());

        // Sign the consensus document with Ed25519 authority keys
        use super::consensus_signer;
        let keys = &*super::AUTHORITY_KEYS;

        // Build complete consensus with footer and signature
        let consensus_with_footer = format!(
            "{}{}",
            consensus_data,
            consensus_signer::build_directory_footer()
        );

        let signed_consensus = consensus_signer::sign_consensus(&consensus_with_footer, keys)
            .context("Failed to sign consensus")?;

        debug!("Tor Directory signed consensus ({} bytes after signing)", signed_consensus.len());

        // Return HTTP response with signed consensus
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            signed_consensus.len(),
            signed_consensus
        );

        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_serve_microdescriptors(&self, action: serde_json::Value) -> Result<ActionResult> {
        let microdescriptors = action
            .get("microdescriptors")
            .and_then(|v| v.as_str())
            .context("Missing 'microdescriptors' parameter")?;

        debug!("Tor Directory serving microdescriptors ({} bytes)", microdescriptors.len());

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            microdescriptors.len(),
            microdescriptors
        );

        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_serve_relay_descriptor(&self, action: serde_json::Value) -> Result<ActionResult> {
        let descriptor = action
            .get("descriptor")
            .and_then(|v| v.as_str())
            .context("Missing 'descriptor' parameter")?;

        debug!("Tor Directory serving relay descriptor");

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            descriptor.len(),
            descriptor
        );

        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_serve_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status_code = action
            .get("status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(404);

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Not Found");

        debug!("Tor Directory serving error {}: {}", status_code, message);

        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: 0\r\n\r\n",
            status_code,
            message
        );

        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_log_directory_request(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        debug!("Tor Directory: {}", message);

        Ok(ActionResult::Custom {
            name: "tor_directory_log".to_string(),
            data: json!({
                "logged": true,
                "message": message
            })
        })
    }
}

impl Server for TorDirectoryProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::tor_directory::TorDirectoryServer;
            TorDirectoryServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            download_real_consensus_action(),
            generate_fake_consensus_action(),
            inject_honeypot_relay_action(),
            list_connected_clients_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            serve_consensus_action(),
            serve_microdescriptors_action(),
            serve_relay_descriptor_action(),
            serve_error_action(),
            log_directory_request_action(),
            close_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "serve_consensus" => self.execute_serve_consensus(action),
            "serve_microdescriptors" => self.execute_serve_microdescriptors(action),
            "serve_relay_descriptor" => self.execute_serve_relay_descriptor(action),
            "serve_error" => self.execute_serve_error(action),
            "log_directory_request" => self.execute_log_directory_request(action),
            "close_connection" => Ok(ActionResult::CloseConnection),
            // Async actions return custom results
            "download_real_consensus" | "generate_fake_consensus"
            | "inject_honeypot_relay" | "list_connected_clients" => {
                Ok(ActionResult::Custom {
                    name: "tor_directory_async".to_string(),
                    data: json!({
                        "action": action_type,
                        "note": "Async action - implementation in server logic"
                    })
                })
            },
            _ => Err(anyhow::anyhow!("Unknown Tor Directory action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Tor Directory"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_tor_directory_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>TorDirectory"
    }

    fn description(&self) -> &'static str {
        "Tor directory server serving consensus documents"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a Tor directory on port 9030 serving fake consensus documents"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["directory", "consensus", "tor_directory", "tor-directory", "directory authority"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, ProtocolState};

        ProtocolMetadataV2::builder()
            .state(ProtocolState::Experimental)
            .implementation("Manual HTTP parsing, LLM consensus generation")
            .llm_control("Consensus documents, microdescriptors, server descriptors")
            .e2e_testing("curl / Tor client")
            .notes("No signing, read-only directory mirror")
            .build()
    }

    fn group_name(&self) -> &'static str {
        "Network Services"
    }
}

// ============================================================================
// Action Definitions - Sync Actions (Network Event Triggered)
// ============================================================================

fn serve_consensus_action() -> ActionDefinition {
    ActionDefinition {
        name: "serve_consensus".to_string(),
        description: "Serve Tor network consensus document".to_string(),
        parameters: vec![
            Parameter {
                name: "consensus_data".to_string(),
                type_hint: "string".to_string(),
                description: "The consensus document content to serve".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "serve_consensus",
            "consensus_data": "network-status-version 3\nvote-status consensus\n..."
        }),
    }
}

fn serve_microdescriptors_action() -> ActionDefinition {
    ActionDefinition {
        name: "serve_microdescriptors".to_string(),
        description: "Serve relay microdescriptors".to_string(),
        parameters: vec![
            Parameter {
                name: "microdescriptors".to_string(),
                type_hint: "string".to_string(),
                description: "The microdescriptor data to serve".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "serve_microdescriptors",
            "microdescriptors": "onion-key\n-----BEGIN RSA PUBLIC KEY-----\n...\n"
        }),
    }
}

fn serve_relay_descriptor_action() -> ActionDefinition {
    ActionDefinition {
        name: "serve_relay_descriptor".to_string(),
        description: "Serve relay descriptor by fingerprint".to_string(),
        parameters: vec![
            Parameter {
                name: "descriptor".to_string(),
                type_hint: "string".to_string(),
                description: "The relay descriptor data to serve".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "serve_relay_descriptor",
            "descriptor": "router nickname 1.2.3.4 9001 0 0\n..."
        }),
    }
}

fn serve_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "serve_error".to_string(),
        description: "Serve HTTP error response".to_string(),
        parameters: vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 404)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message (default: 'Not Found')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "serve_error",
            "status_code": 404,
            "message": "Not Found"
        }),
    }
}

fn log_directory_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "log_directory_request".to_string(),
        description: "Log directory request for analysis".to_string(),
        parameters: vec![
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Log message describing the request".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "log_directory_request",
            "message": "Client requested consensus from 192.168.1.100"
        }),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the connection after response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}

// ============================================================================
// Action Definitions - Async Actions (User Triggered)
// ============================================================================

fn download_real_consensus_action() -> ActionDefinition {
    ActionDefinition {
        name: "download_real_consensus".to_string(),
        description: "Download real consensus from Tor directory authorities (requires explicit user consent)".to_string(),
        parameters: vec![
            Parameter {
                name: "authority_url".to_string(),
                type_hint: "string".to_string(),
                description: "Directory authority URL (default: first available authority)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "download_real_consensus",
            "authority_url": "http://128.31.0.34:9131/tor/status-vote/current/consensus"
        }),
    }
}

fn generate_fake_consensus_action() -> ActionDefinition {
    ActionDefinition {
        name: "generate_fake_consensus".to_string(),
        description: "Generate fake/honeypot consensus for local testing".to_string(),
        parameters: vec![
            Parameter {
                name: "relay_count".to_string(),
                type_hint: "number".to_string(),
                description: "Number of fake relays to include (default: 10)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "generate_fake_consensus",
            "relay_count": 10
        }),
    }
}

fn inject_honeypot_relay_action() -> ActionDefinition {
    ActionDefinition {
        name: "inject_honeypot_relay".to_string(),
        description: "Inject honeypot relay into consensus".to_string(),
        parameters: vec![
            Parameter {
                name: "nickname".to_string(),
                type_hint: "string".to_string(),
                description: "Relay nickname".to_string(),
                required: true,
            },
            Parameter {
                name: "ip".to_string(),
                type_hint: "string".to_string(),
                description: "Relay IP address".to_string(),
                required: true,
            },
            Parameter {
                name: "port".to_string(),
                type_hint: "number".to_string(),
                description: "Relay OR port".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "inject_honeypot_relay",
            "nickname": "HoneypotRelay1",
            "ip": "192.168.1.100",
            "port": 9001
        }),
    }
}

fn list_connected_clients_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_connected_clients".to_string(),
        description: "List clients that have downloaded directory information".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_connected_clients"
        }),
    }
}

// ============================================================================
// Action Constants
// ============================================================================

pub static SERVE_CONSENSUS_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| serve_consensus_action());
pub static SERVE_MICRODESCRIPTORS_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| serve_microdescriptors_action());
pub static SERVE_RELAY_DESCRIPTOR_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| serve_relay_descriptor_action());
pub static SERVE_ERROR_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| serve_error_action());
pub static LOG_DIRECTORY_REQUEST_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| log_directory_request_action());
pub static CLOSE_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| close_connection_action());

// ============================================================================
// Event Type Constants
// ============================================================================

/// Tor Directory request event - triggered when client requests directory data
pub static TOR_DIRECTORY_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tor_directory_request",
        "Tor directory HTTP request received from client"
    )
    .with_parameters(vec![
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "The requested URL path (e.g., '/tor/status-vote/current/consensus')".to_string(),
            required: true,
        },
        Parameter {
            name: "client_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Client IP address".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        SERVE_CONSENSUS_ACTION.clone(),
        SERVE_MICRODESCRIPTORS_ACTION.clone(),
        SERVE_RELAY_DESCRIPTOR_ACTION.clone(),
        SERVE_ERROR_ACTION.clone(),
        LOG_DIRECTORY_REQUEST_ACTION.clone(),
        CLOSE_CONNECTION_ACTION.clone(),
    ])
});

pub fn get_tor_directory_event_types() -> Vec<EventType> {
    vec![
        TOR_DIRECTORY_REQUEST_EVENT.clone(),
    ]
}
