//! USB Smart Card Reader (CCID) protocol actions implementation
//!
//! This module implements a virtual smart card using the vpicc crate and vsmartcard infrastructure.
//! This approach avoids implementing USB CCID directly by using the mature vsmartcard project.

#[cfg(feature = "usb-smartcard")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-smartcard")]
use crate::protocol::EventType;
#[cfg(feature = "usb-smartcard")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-smartcard")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-smartcard")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-smartcard")]
use serde_json::json;
#[cfg(feature = "usb-smartcard")]
use std::collections::HashMap;
#[cfg(feature = "usb-smartcard")]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "usb-smartcard")]
use tokio::sync::Mutex;

// Event type definitions
#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_INSERTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_inserted",
        "Smart card inserted into virtual reader",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID"),
        Parameter::new("atr", "string", "Answer To Reset (ATR) hex string"),
        Parameter::new("card_type", "string", "Card type (PIV, OpenPGP, Generic)"),
    ])
});

#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_REMOVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_removed",
        "Smart card removed from virtual reader",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID"),
    ])
});

#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_PIN_REQUESTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_pin_requested",
        "Application requested PIN verification",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID"),
        Parameter::new("pin_reference", "number", "PIN reference number (0-15)"),
        Parameter::new("retries_remaining", "number", "Number of retries remaining"),
    ])
});

#[cfg(feature = "usb-smartcard")]
pub static SMARTCARD_APDU_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smartcard_apdu_received",
        "Application sent APDU command to card",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID"),
        Parameter::new("cla", "number", "Class byte"),
        Parameter::new("ins", "number", "Instruction byte"),
        Parameter::new("command", "string", "Human-readable command description"),
    ])
});

/// USB Smart Card protocol action handler
#[cfg(feature = "usb-smartcard")]
pub struct UsbSmartCardProtocol {
    /// Map of active connections
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

#[cfg(feature = "usb-smartcard")]
pub struct ConnectionData {
    /// Card insertion state
    pub card_inserted: bool,
    /// Current PIN state
    pub pin_verified: bool,
    /// PIN retry counter
    pub pin_retries: u8,
}

#[cfg(feature = "usb-smartcard")]
impl UsbSmartCardProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-smartcard")]
impl Protocol for UsbSmartCardProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "card_type".to_string(),
                param_type: "string".to_string(),
                description: "Type of smart card to emulate (piv, openpgp, generic)".to_string(),
                required: false,
                default_value: Some("generic".to_string()),
            },
            crate::llm::actions::ParameterDefinition {
                name: "default_pin".to_string(),
                param_type: "string".to_string(),
                description: "Default PIN for the card".to_string(),
                required: false,
                default_value: Some("123456".to_string()),
            },
            crate::llm::actions::ParameterDefinition {
                name: "vpcd_host".to_string(),
                param_type: "string".to_string(),
                description: "vsmartcard vpcd daemon host".to_string(),
                required: false,
                default_value: Some("localhost".to_string()),
            },
            crate::llm::actions::ParameterDefinition {
                name: "vpcd_port".to_string(),
                param_type: "number".to_string(),
                description: "vsmartcard vpcd daemon port".to_string(),
                required: false,
                default_value: Some("35963".to_string()),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "usb-smartcard"
    }

    fn stack_name(&self) -> &'static str {
        "USB Smart Card Reader (vpicc)"
    }

    fn get_async_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "insert_card".to_string(),
                description: "Insert virtual smart card into reader".to_string(),
                parameters: vec![
                    Parameter::new("connection_id", "string", "Connection ID"),
                ],
            },
            ActionDefinition {
                name: "remove_card".to_string(),
                description: "Remove virtual smart card from reader".to_string(),
                parameters: vec![
                    Parameter::new("connection_id", "string", "Connection ID"),
                ],
            },
            ActionDefinition {
                name: "set_pin".to_string(),
                description: "Set or change the card PIN".to_string(),
                parameters: vec![
                    Parameter::new("connection_id", "string", "Connection ID"),
                    Parameter::new("new_pin", "string", "New PIN value"),
                ],
            },
            ActionDefinition {
                name: "verify_pin".to_string(),
                description: "Verify PIN (approve pending PIN request)".to_string(),
                parameters: vec![
                    Parameter::new("connection_id", "string", "Connection ID"),
                    Parameter::new("pin", "string", "PIN to verify"),
                ],
            },
            ActionDefinition {
                name: "list_files".to_string(),
                description: "List files on the card".to_string(),
                parameters: vec![
                    Parameter::new("connection_id", "string", "Connection ID"),
                ],
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
        connection_id: Option<ConnectionId>,
        _app_state: Arc<AppState>,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Missing action type")?;

        match action_type {
            "insert_card" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;

                // NOTE: Card state is managed in the SmartCardHandler
                // Virtual card is always "inserted" upon USB/IP connection
                // True card insertion/removal would require vpicc integration
                info!("insert_card called - card is virtually inserted on USB attach");
                Ok(ActionResult::Message {
                    message: "Smart card is virtually inserted upon USB connection".to_string()
                })
            }
            "remove_card" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;

                info!("remove_card called - card removal requires USB disconnect");
                Ok(ActionResult::Message {
                    message: "To remove card, disconnect the USB/IP session".to_string()
                })
            }
            "set_pin" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;
                let new_pin = action["new_pin"]
                    .as_str()
                    .context("Missing new_pin")?;

                // NOTE: PIN is managed in SmartCardHandler's PIN store
                // Would need direct handler access to modify
                info!("set_pin called - PIN management requires handler access");
                Ok(ActionResult::Message {
                    message: format!("PIN change to '{}...' requires direct handler access. Use CHANGE REFERENCE DATA APDU instead.",
                        &new_pin.chars().take(1).collect::<String>())
                })
            }
            "verify_pin" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;
                let pin = action["pin"]
                    .as_str()
                    .context("Missing pin")?;

                // NOTE: PIN verification happens via VERIFY APDU from client
                // LLM observes via smartcard_pin_requested_event
                info!("verify_pin called - PIN verification is client-driven via APDU");
                Ok(ActionResult::Message {
                    message: format!("PIN verification with '{}...' happens via VERIFY APDU from PC/SC client",
                        &pin.chars().take(1).collect::<String>())
                })
            }
            "list_files" => {
                let _conn_id = action["connection_id"]
                    .as_str()
                    .context("Missing connection_id")?;

                // NOTE: File system is in SmartCardHandler
                // Currently implements basic RSA key storage, not full ISO 7816-4 FS
                info!("list_files called - card has RSA key store, not full file system yet");
                Ok(ActionResult::Message {
                    message: "Current implementation: RSA key storage (see SmartCardKeyStore). Full ISO 7816-4 file system not yet implemented.".to_string()
                })
            }
            _ => Ok(ActionResult::NoAction),
        }
    }

    fn get_event_types(&self) -> Vec<&EventType> {
        vec![
            &SMARTCARD_INSERTED_EVENT,
            &SMARTCARD_REMOVED_EVENT,
            &SMARTCARD_PIN_REQUESTED_EVENT,
            &SMARTCARD_APDU_RECEIVED_EVENT,
        ]
    }
}

// Implement Server trait
#[cfg(feature = "usb-smartcard")]
impl Server for UsbSmartCardProtocol {}
