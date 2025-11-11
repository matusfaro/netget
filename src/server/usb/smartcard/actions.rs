//! USB Smart Card Reader (CCID) protocol actions implementation
//!
//! This module implements a virtual smart card using the vpicc crate and vsmartcard infrastructure.
//! This approach avoids implementing USB CCID directly by using the mature vsmartcard project.

#[cfg(feature = "usb-smartcard")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
#[cfg(feature = "usb-smartcard")]
use crate::protocol::EventType;
#[cfg(feature = "usb-smartcard")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-smartcard")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-smartcard")]
use serde_json::json;
#[cfg(feature = "usb-smartcard")]
use std::sync::LazyLock;
#[cfg(feature = "usb-smartcard")]
use tracing::{debug, info};

// Event type definitions
#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_INSERTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_inserted",
        "Smart card inserted into virtual reader",
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        },
        Parameter {
            name: "atr".to_string(),
            type_hint: "string".to_string(),
            description: "Answer To Reset (ATR) hex string".to_string(),
            required: true,
        },
        Parameter {
            name: "card_type".to_string(),
            type_hint: "string".to_string(),
            description: "Card type (PIV, OpenPGP, Generic)".to_string(),
            required: true,
        },
    ])
});

#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_REMOVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_removed",
        "Smart card removed from virtual reader",
    )
    .with_parameters(vec![Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection ID".to_string(),
        required: true,
    }])
});

#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_PIN_REQUESTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_pin_requested",
        "Application requested PIN verification",
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        },
        Parameter {
            name: "pin_reference".to_string(),
            type_hint: "number".to_string(),
            description: "PIN reference number (0-15)".to_string(),
            required: true,
        },
        Parameter {
            name: "retries_remaining".to_string(),
            type_hint: "number".to_string(),
            description: "Number of retries remaining".to_string(),
            required: true,
        },
    ])
});

#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_APDU_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_apdu_received",
        "Application sent APDU command to card",
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        },
        Parameter {
            name: "cla".to_string(),
            type_hint: "number".to_string(),
            description: "Class byte".to_string(),
            required: true,
        },
        Parameter {
            name: "ins".to_string(),
            type_hint: "number".to_string(),
            description: "Instruction byte".to_string(),
            required: true,
        },
        Parameter {
            name: "command".to_string(),
            type_hint: "string".to_string(),
            description: "Human-readable command description".to_string(),
            required: true,
        },
    ])
});

/// USB Smart Card protocol action handler
#[cfg(feature = "usb-smartcard")]
pub struct UsbSmartCardProtocol;

#[cfg(feature = "usb-smartcard")]
impl UsbSmartCardProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-smartcard")]
impl Protocol for UsbSmartCardProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "card_type".to_string(),
                type_hint: "string".to_string(),
                description: "Type of smart card to emulate (piv, openpgp, generic)".to_string(),
                required: false,
                example: json!("generic"),
            },
            ParameterDefinition {
                name: "default_pin".to_string(),
                type_hint: "string".to_string(),
                description: "Default PIN for the card".to_string(),
                required: false,
                example: json!("123456"),
            },
            ParameterDefinition {
                name: "vpcd_host".to_string(),
                type_hint: "string".to_string(),
                description: "vsmartcard vpcd daemon host".to_string(),
                required: false,
                example: json!("localhost"),
            },
            ParameterDefinition {
                name: "vpcd_port".to_string(),
                type_hint: "number".to_string(),
                description: "vsmartcard vpcd daemon port".to_string(),
                required: false,
                example: json!(35963),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "usb-smartcard"
    }

    fn stack_name(&self) -> &'static str {
        "USB Smart Card Reader (vpicc)"
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "insert_card".to_string(),
                description: "Insert virtual smart card into reader".to_string(),
                parameters: vec![Parameter {
                    name: "connection_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "Connection ID".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "insert_card",
                    "connection_id": "conn_1"
                }),
            },
            ActionDefinition {
                name: "remove_card".to_string(),
                description: "Remove virtual smart card from reader".to_string(),
                parameters: vec![Parameter {
                    name: "connection_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "Connection ID".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "remove_card",
                    "connection_id": "conn_1"
                }),
            },
            ActionDefinition {
                name: "set_pin".to_string(),
                description: "Set or change the card PIN".to_string(),
                parameters: vec![
                    Parameter {
                        name: "connection_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Connection ID".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "new_pin".to_string(),
                        type_hint: "string".to_string(),
                        description: "New PIN value".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "set_pin",
                    "connection_id": "conn_1",
                    "new_pin": "123456"
                }),
            },
            ActionDefinition {
                name: "verify_pin".to_string(),
                description: "Verify PIN (approve pending PIN request)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "connection_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Connection ID".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "pin".to_string(),
                        type_hint: "string".to_string(),
                        description: "PIN to verify".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "verify_pin",
                    "connection_id": "conn_1",
                    "pin": "123456"
                }),
            },
            ActionDefinition {
                name: "list_files".to_string(),
                description: "List files on the card".to_string(),
                parameters: vec![Parameter {
                    name: "connection_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "Connection ID".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "list_files",
                    "connection_id": "conn_1"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            SMARTCARD_INSERTED_EVENT.clone(),
            SMARTCARD_REMOVED_EVENT.clone(),
            SMARTCARD_PIN_REQUESTED_EVENT.clone(),
            SMARTCARD_APDU_RECEIVED_EVENT.clone(),
        ]
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["usb", "smartcard", "smart card", "ccid", "vpicc"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .privilege_requirement(PrivilegeRequirement::None)
            .implementation("Virtual smart card using vpicc crate and vsmartcard infrastructure")
            .llm_control("Card insertion, PIN verification, APDU response generation")
            .e2e_testing("Not yet implemented - requires vpcd daemon and PC/SC client")
            .notes("Uses vpicc instead of USB CCID for simplicity. Requires external vpcd daemon.")
            .build()
    }

    fn description(&self) -> &'static str {
        "Virtual smart card reader (CCID) for authentication and secure storage"
    }

    fn example_prompt(&self) -> &'static str {
        "Create a virtual smart card reader on USB that emulates a PIV card with PIN 123456"
    }

    fn group_name(&self) -> &'static str {
        "USB Devices"
    }
}

// Implement Server trait
#[cfg(feature = "usb-smartcard")]
impl Server for UsbSmartCardProtocol {
    fn spawn(
        &self,
        _ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            // TODO: Implement USB smart card server spawning
            // This will require vpicc integration with USB/IP
            anyhow::bail!("USB Smart Card server not yet implemented - requires vpicc integration")
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"].as_str().context("Missing action type")?;

        match action_type {
            "insert_card" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;

                // NOTE: Card state is managed in the SmartCardHandler
                // Virtual card is always "inserted" upon USB/IP connection
                // True card insertion/removal would require vpicc integration
                info!("insert_card called - card is virtually inserted on USB attach");
                Ok(ActionResult::NoAction)
            }
            "remove_card" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;

                info!("remove_card called - card removal requires USB disconnect");
                Ok(ActionResult::NoAction)
            }
            "set_pin" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;
                let new_pin = action["new_pin"].as_str().context("Missing new_pin")?;

                // NOTE: PIN is managed in SmartCardHandler's PIN store
                // Would need direct handler access to modify
                info!("set_pin called - PIN management requires handler access");
                debug!(
                    "Requested PIN change to '{}...'",
                    &new_pin.chars().take(1).collect::<String>()
                );
                Ok(ActionResult::NoAction)
            }
            "verify_pin" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;
                let pin = action["pin"].as_str().context("Missing pin")?;

                // NOTE: PIN verification happens via VERIFY APDU from client
                // LLM observes via smartcard_pin_requested_event
                info!("verify_pin called - PIN verification is client-driven via APDU");
                debug!(
                    "PIN verification with '{}...'",
                    &pin.chars().take(1).collect::<String>()
                );
                Ok(ActionResult::NoAction)
            }
            "list_files" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;

                // NOTE: File system is in SmartCardHandler
                // Currently implements basic RSA key storage, not full ISO 7816-4 FS
                info!("list_files called - card has RSA key store, not full file system yet");
                Ok(ActionResult::NoAction)
            }
            _ => Ok(ActionResult::NoAction),
        }
    }
}
