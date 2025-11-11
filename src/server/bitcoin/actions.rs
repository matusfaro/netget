//! Bitcoin P2P protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use bitcoin::consensus::Encodable;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::Magic;
use bitcoin::p2p::ServiceFlags;
use serde_json::json;
use std::sync::LazyLock;

/// Bitcoin protocol action handler
pub struct BitcoinProtocol;

impl BitcoinProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for BitcoinProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "network".to_string(),
            type_hint: "string".to_string(),
            description:
                "Bitcoin network: 'mainnet', 'testnet', 'signet', or 'regtest' (default: mainnet)"
                    .to_string(),
            required: false,
            example: json!("mainnet"),
        }]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_bitcoin_message_action(),
            send_version_action(),
            send_verack_action(),
            send_ping_action(),
            send_pong_action(),
            send_getaddr_action(),
            close_this_connection_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Bitcoin P2P"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_bitcoin_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>Bitcoin"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bitcoin", "btc", "p2p", "blockchain"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("Bitcoin P2P protocol using rust-bitcoin crate for message parsing")
                .llm_control("LLM decides how to respond to all P2P messages (version, getdata, ping, etc.)")
                .e2e_testing("Bitcoin P2P client (TBD)")
                .notes("Not a real full node - LLM controls all responses. Supports mainnet/testnet/signet/regtest networks.")
                .build()
    }
    fn description(&self) -> &'static str {
        "Bitcoin P2P protocol server (LLM-controlled, not a real full node)"
    }
    fn example_prompt(&self) -> &'static str {
        "Run Bitcoin P2P server on port 8333; respond to version with our own version, handle ping/pong"
    }
    fn group_name(&self) -> &'static str {
        "Blockchain"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for BitcoinProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            // Extract network from startup_params (default: mainnet)
            let network = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_string("network"))
                .unwrap_or_else(|| "mainnet".to_string());

            use crate::server::bitcoin::BitcoinServer;
            BitcoinServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                network,
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
            "send_bitcoin_message" => self.execute_send_bitcoin_message(action),
            "send_version" => self.execute_send_version(action),
            "send_verack" => self.execute_send_verack(action),
            "send_ping" => self.execute_send_ping(action),
            "send_pong" => self.execute_send_pong(action),
            "send_getaddr" => self.execute_send_getaddr(action),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown Bitcoin action: {}", action_type)),
        }
    }
}

impl BitcoinProtocol {
    /// Execute send_bitcoin_message action (send raw hex-encoded message)
    fn execute_send_bitcoin_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let hex_data = action
            .get("hex_data")
            .and_then(|v| v.as_str())
            .context("Missing 'hex_data' parameter")?;

        let bytes = hex::decode(hex_data).context("Invalid hex data")?;

        Ok(ActionResult::Output(bytes))
    }

    /// Execute send_version action
    fn execute_send_version(&self, action: serde_json::Value) -> Result<ActionResult> {
        let network = action
            .get("network")
            .and_then(|v| v.as_str())
            .unwrap_or("mainnet");

        let magic = match network {
            "mainnet" | "main" => Magic::BITCOIN,
            "testnet" | "test" => Magic::TESTNET3,
            "signet" => Magic::SIGNET,
            "regtest" => Magic::REGTEST,
            _ => Magic::BITCOIN,
        };

        // Get optional parameters with defaults
        let version = action
            .get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(70015) as u32;

        let services = action.get("services").and_then(|v| v.as_u64()).unwrap_or(0);

        let timestamp = action
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            });

        let user_agent = action
            .get("user_agent")
            .and_then(|v| v.as_str())
            .unwrap_or("/NetGet:0.1.0/");

        let start_height = action
            .get("start_height")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;

        let relay = action
            .get("relay")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Create version message
        let receiver = Address::new(
            &std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
            ServiceFlags::NONE,
        );
        let sender = Address::new(
            &std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
            ServiceFlags::from(services),
        );

        let version_msg = NetworkMessage::Version(VersionMessage {
            version,
            services: ServiceFlags::from(services),
            timestamp,
            receiver,
            sender,
            nonce: rand::random(),
            user_agent: user_agent.to_string(),
            start_height,
            relay,
        });

        // Encode to bytes
        let raw_msg = RawNetworkMessage::new(magic, version_msg);
        let mut bytes = Vec::new();
        raw_msg
            .consensus_encode(&mut bytes)
            .context("Failed to encode version message")?;

        Ok(ActionResult::Output(bytes))
    }

    /// Execute send_verack action
    fn execute_send_verack(&self, action: serde_json::Value) -> Result<ActionResult> {
        let network = action
            .get("network")
            .and_then(|v| v.as_str())
            .unwrap_or("mainnet");

        let magic = match network {
            "mainnet" | "main" => Magic::BITCOIN,
            "testnet" | "test" => Magic::TESTNET3,
            "signet" => Magic::SIGNET,
            "regtest" => Magic::REGTEST,
            _ => Magic::BITCOIN,
        };

        let raw_msg = RawNetworkMessage::new(magic, NetworkMessage::Verack);
        let mut bytes = Vec::new();
        raw_msg
            .consensus_encode(&mut bytes)
            .context("Failed to encode verack message")?;

        Ok(ActionResult::Output(bytes))
    }

    /// Execute send_ping action
    fn execute_send_ping(&self, action: serde_json::Value) -> Result<ActionResult> {
        let network = action
            .get("network")
            .and_then(|v| v.as_str())
            .unwrap_or("mainnet");

        let magic = match network {
            "mainnet" | "main" => Magic::BITCOIN,
            "testnet" | "test" => Magic::TESTNET3,
            "signet" => Magic::SIGNET,
            "regtest" => Magic::REGTEST,
            _ => Magic::BITCOIN,
        };

        let nonce = action
            .get("nonce")
            .and_then(|v| v.as_u64())
            .unwrap_or_else(|| rand::random());

        let raw_msg = RawNetworkMessage::new(magic, NetworkMessage::Ping(nonce));
        let mut bytes = Vec::new();
        raw_msg
            .consensus_encode(&mut bytes)
            .context("Failed to encode ping message")?;

        Ok(ActionResult::Output(bytes))
    }

    /// Execute send_pong action
    fn execute_send_pong(&self, action: serde_json::Value) -> Result<ActionResult> {
        let network = action
            .get("network")
            .and_then(|v| v.as_str())
            .unwrap_or("mainnet");

        let magic = match network {
            "mainnet" | "main" => Magic::BITCOIN,
            "testnet" | "test" => Magic::TESTNET3,
            "signet" => Magic::SIGNET,
            "regtest" => Magic::REGTEST,
            _ => Magic::BITCOIN,
        };

        let nonce = action
            .get("nonce")
            .and_then(|v| v.as_u64())
            .context("Missing 'nonce' parameter for pong")?;

        let raw_msg = RawNetworkMessage::new(magic, NetworkMessage::Pong(nonce));
        let mut bytes = Vec::new();
        raw_msg
            .consensus_encode(&mut bytes)
            .context("Failed to encode pong message")?;

        Ok(ActionResult::Output(bytes))
    }

    /// Execute send_getaddr action
    fn execute_send_getaddr(&self, action: serde_json::Value) -> Result<ActionResult> {
        let network = action
            .get("network")
            .and_then(|v| v.as_str())
            .unwrap_or("mainnet");

        let magic = match network {
            "mainnet" | "main" => Magic::BITCOIN,
            "testnet" | "test" => Magic::TESTNET3,
            "signet" => Magic::SIGNET,
            "regtest" => Magic::REGTEST,
            _ => Magic::BITCOIN,
        };

        let raw_msg = RawNetworkMessage::new(magic, NetworkMessage::GetAddr);
        let mut bytes = Vec::new();
        raw_msg
            .consensus_encode(&mut bytes)
            .context("Failed to encode getaddr message")?;

        Ok(ActionResult::Output(bytes))
    }
}

/// Action definition for send_bitcoin_message
fn send_bitcoin_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_bitcoin_message".to_string(),
        description: "Send a raw Bitcoin P2P message (hex-encoded bytes)".to_string(),
        parameters: vec![Parameter {
            name: "hex_data".to_string(),
            type_hint: "string".to_string(),
            description: "Hex-encoded Bitcoin message (including header)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_bitcoin_message",
            "hex_data": "f9beb4d976657261636b000000000000000000005df6e0e2"
        }),
    }
}

/// Action definition for send_version
fn send_version_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_version".to_string(),
        description: "Send a Bitcoin version message (handshake)".to_string(),
        parameters: vec![
            Parameter {
                name: "network".to_string(),
                type_hint: "string".to_string(),
                description: "Network: 'mainnet', 'testnet', 'signet', 'regtest'".to_string(),
                required: false,
            },
            Parameter {
                name: "version".to_string(),
                type_hint: "number".to_string(),
                description: "Protocol version (default: 70015)".to_string(),
                required: false,
            },
            Parameter {
                name: "services".to_string(),
                type_hint: "number".to_string(),
                description: "Service flags (default: 0)".to_string(),
                required: false,
            },
            Parameter {
                name: "user_agent".to_string(),
                type_hint: "string".to_string(),
                description: "User agent string (default: '/NetGet:0.1.0/')".to_string(),
                required: false,
            },
            Parameter {
                name: "start_height".to_string(),
                type_hint: "number".to_string(),
                description: "Blockchain height (default: 0)".to_string(),
                required: false,
            },
            Parameter {
                name: "relay".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether to relay transactions (default: false)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_version",
            "network": "mainnet",
            "version": 70015,
            "user_agent": "/NetGet:0.1.0/",
            "start_height": 0,
            "relay": false
        }),
    }
}

/// Action definition for send_verack
fn send_verack_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_verack".to_string(),
        description: "Send a Bitcoin verack message (acknowledge version)".to_string(),
        parameters: vec![Parameter {
            name: "network".to_string(),
            type_hint: "string".to_string(),
            description: "Network: 'mainnet', 'testnet', 'signet', 'regtest'".to_string(),
            required: false,
        }],
        example: json!({
            "type": "send_verack",
            "network": "mainnet"
        }),
    }
}

/// Action definition for send_ping
fn send_ping_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ping".to_string(),
        description: "Send a Bitcoin ping message".to_string(),
        parameters: vec![
            Parameter {
                name: "network".to_string(),
                type_hint: "string".to_string(),
                description: "Network: 'mainnet', 'testnet', 'signet', 'regtest'".to_string(),
                required: false,
            },
            Parameter {
                name: "nonce".to_string(),
                type_hint: "number".to_string(),
                description: "Ping nonce (default: random)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_ping",
            "network": "mainnet",
            "nonce": 123456789
        }),
    }
}

/// Action definition for send_pong
fn send_pong_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_pong".to_string(),
        description: "Send a Bitcoin pong message (response to ping)".to_string(),
        parameters: vec![
            Parameter {
                name: "network".to_string(),
                type_hint: "string".to_string(),
                description: "Network: 'mainnet', 'testnet', 'signet', 'regtest'".to_string(),
                required: false,
            },
            Parameter {
                name: "nonce".to_string(),
                type_hint: "number".to_string(),
                description: "Nonce from ping message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_pong",
            "network": "mainnet",
            "nonce": 123456789
        }),
    }
}

/// Action definition for send_getaddr
fn send_getaddr_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_getaddr".to_string(),
        description: "Send a Bitcoin getaddr message (request peer addresses)".to_string(),
        parameters: vec![Parameter {
            name: "network".to_string(),
            type_hint: "string".to_string(),
            description: "Network: 'mainnet', 'testnet', 'signet', 'regtest'".to_string(),
            required: false,
        }],
        example: json!({
            "type": "send_getaddr",
            "network": "mainnet"
        }),
    }
}

/// Action definition for close_this_connection
fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current Bitcoin P2P connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_this_connection"
        }),
    }
}

// ============================================================================
// Bitcoin Event Type Constants
// ============================================================================

/// Bitcoin connection opened event
pub static BITCOIN_CONNECTION_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "bitcoin_connection_opened",
        "New Bitcoin P2P connection established (decide whether to send version or wait)",
    )
    .with_actions(vec![send_version_action(), close_this_connection_action()])
});

/// Bitcoin message received event
pub static BITCOIN_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("bitcoin_message_received", "Bitcoin P2P message received")
        .with_parameters(vec![
            Parameter {
                name: "message_type".to_string(),
                type_hint: "string".to_string(),
                description: "Type of message (version, verack, ping, pong, getdata, etc.)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "object".to_string(),
                description: "Parsed message data (structure depends on message type)".to_string(),
                required: true,
            },
        ])
        .with_actions(vec![
            send_bitcoin_message_action(),
            send_version_action(),
            send_verack_action(),
            send_ping_action(),
            send_pong_action(),
            send_getaddr_action(),
            close_this_connection_action(),
        ])
});

/// Get Bitcoin event types
pub fn get_bitcoin_event_types() -> Vec<EventType> {
    vec![
        BITCOIN_CONNECTION_OPENED_EVENT.clone(),
        BITCOIN_MESSAGE_RECEIVED_EVENT.clone(),
    ]
}
