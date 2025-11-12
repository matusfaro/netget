//! NFC (Near Field Communication) server protocol actions implementation
//! Virtual NFC tag/card emulation for testing (PC/SC readers typically can't emulate)

use crate::llm::actions::{
    protocol_trait::Protocol, ActionDefinition, ActionResult, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use crate::state::server::ServerId;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// NFC server started event
pub static NFC_SERVER_STARTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_server_started",
        "Virtual NFC tag/card server started (for testing)",
    )
});

/// NFC tag selected event - triggered when virtual tag's application is selected
pub static NFC_TAG_SELECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_tag_selected",
        "Virtual NFC tag application selected by reader",
    )
    .with_parameters(vec![Parameter {
        name: "application_id".to_string(),
        type_hint: "string".to_string(),
        description: "Application ID (AID) that was selected (hex)".to_string(),
        required: true,
    }])
});

/// NFC APDU command received event - triggered when virtual tag receives APDU
pub static NFC_APDU_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_apdu_received",
        "APDU command received by virtual NFC tag",
    )
    .with_parameters(vec![
        Parameter {
            name: "apdu_hex".to_string(),
            type_hint: "string".to_string(),
            description: "APDU command as hex string".to_string(),
            required: true,
        },
        Parameter {
            name: "cla".to_string(),
            type_hint: "string".to_string(),
            description: "Class byte (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "ins".to_string(),
            type_hint: "string".to_string(),
            description: "Instruction byte (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "p1".to_string(),
            type_hint: "string".to_string(),
            description: "Parameter 1 (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "p2".to_string(),
            type_hint: "string".to_string(),
            description: "Parameter 2 (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Command data (hex)".to_string(),
            required: false,
        },
    ])
});

/// NFC server protocol implementation
pub struct NfcServerProtocol;

impl Protocol for NfcServerProtocol {
    fn protocol_name(&self) -> &'static str {
        "nfc"
    }

    fn stack_name(&self) -> &'static str {
        "application"
    }

    fn get_event_types(&self) -> Vec<&LazyLock<EventType>> {
        vec![
            &NFC_SERVER_STARTED_EVENT,
            &NFC_TAG_SELECTED_EVENT,
            &NFC_APDU_RECEIVED_EVENT,
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "set_atr".to_string(),
                description: "Set Answer to Reset (ATR) for virtual tag".to_string(),
                parameters: vec![Parameter {
                    name: "atr_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "ATR bytes as hex string".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "set_atr",
                    "atr_hex": "3B8F8001804F0CA0000003060300030000000068"
                }),
            },
            ActionDefinition {
                name: "set_ndef_message".to_string(),
                description: "Set NDEF message content for virtual tag".to_string(),
                parameters: vec![Parameter {
                    name: "records".to_string(),
                    type_hint: "array".to_string(),
                    description: "Array of NDEF records".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "set_ndef_message",
                    "records": [
                        {
                            "type": "text",
                            "language": "en",
                            "text": "Hello NFC!"
                        },
                        {
                            "type": "uri",
                            "uri": "https://example.com"
                        }
                    ]
                }),
            },
        ]
    }

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![ActionDefinition {
            name: "respond_to_apdu".to_string(),
            description: "Respond to received APDU command with data and status bytes".to_string(),
            parameters: vec![
                Parameter {
                    name: "data_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Response data (hex, optional)".to_string(),
                    required: false,
                },
                Parameter {
                    name: "sw1".to_string(),
                    type_hint: "string".to_string(),
                    description:
                        "Status byte 1 (hex, default: '90' for success)".to_string(),
                    required: false,
                },
                Parameter {
                    name: "sw2".to_string(),
                    type_hint: "string".to_string(),
                    description: "Status byte 2 (hex, default: '00' for success)".to_string(),
                    required: false,
                },
            ],
            example: json!({
                "type": "respond_to_apdu",
                "data_hex": "D2760000850101",
                "sw1": "90",
                "sw2": "00"
            }),
        })]
    }

    fn get_startup_params(&self) -> Vec<Parameter> {
        vec![
            Parameter {
                name: "tag_type".to_string(),
                type_hint: "string".to_string(),
                description: "Virtual tag type: 'type2' (MIFARE), 'type4' (ISO14443-4), 'generic' (default)"
                    .to_string(),
                required: false,
            },
            Parameter {
                name: "uid".to_string(),
                type_hint: "string".to_string(),
                description: "Tag UID (hex, auto-generated if not specified)".to_string(),
                required: false,
            },
        ]
    }

    fn execute_action(
        &self,
        _server_id: ServerId,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing 'type' field in action"))?;

        match action_type {
            "set_atr" => {
                let atr_hex = action["atr_hex"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing 'atr_hex' field"))?;

                // Validate ATR format
                hex::decode(atr_hex).context("Invalid ATR hex string")?;

                Ok(ActionResult::Custom {
                    name: "set_atr".to_string(),
                    data: json!({ "atr_hex": atr_hex }),
                })
            }
            "set_ndef_message" => {
                let records = action["records"]
                    .as_array()
                    .ok_or_else(|| anyhow!("Missing 'records' array"))?;

                Ok(ActionResult::Custom {
                    name: "set_ndef_message".to_string(),
                    data: json!({ "records": records }),
                })
            }
            "respond_to_apdu" => {
                let data_hex = action["data_hex"].as_str().unwrap_or("");
                let sw1 = action["sw1"].as_str().unwrap_or("90");
                let sw2 = action["sw2"].as_str().unwrap_or("00");

                // Validate hex strings
                if !data_hex.is_empty() {
                    hex::decode(data_hex).context("Invalid data_hex")?;
                }
                hex::decode(sw1).context("Invalid sw1")?;
                hex::decode(sw2).context("Invalid sw2")?;

                Ok(ActionResult::Custom {
                    name: "respond_to_apdu".to_string(),
                    data: json!({
                        "data_hex": data_hex,
                        "sw1": sw1,
                        "sw2": sw2
                    }),
                })
            }
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
