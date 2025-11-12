//! NFC (Near Field Communication) client protocol actions implementation
//! Uses PC/SC API for cross-platform smart card/NFC reader support

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult, ConnectContext},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

/// NFC reader list event - triggered after listing available PC/SC readers
pub static NFC_READERS_LISTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_readers_listed",
        "Available NFC/smart card readers enumerated via PC/SC",
    )
    .with_parameters(vec![Parameter {
        name: "readers".to_string(),
        type_hint: "array".to_string(),
        description: "List of reader names (strings)".to_string(),
        required: true,
    }])
});

/// NFC card detected event - triggered when a card/tag is detected in reader
pub static NFC_CARD_DETECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_card_detected",
        "NFC card/tag detected in reader",
    )
    .with_parameters(vec![
        Parameter {
            name: "atr".to_string(),
            type_hint: "string".to_string(),
            description: "Answer to Reset (ATR) hex string".to_string(),
            required: true,
        },
        Parameter {
            name: "protocol".to_string(),
            type_hint: "string".to_string(),
            description: "Active protocol (T0, T1, etc.)".to_string(),
            required: false,
        },
    ])
});

/// NFC APDU response event - triggered after sending APDU command
pub static NFC_APDU_RESPONSE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_apdu_response",
        "APDU response received from card/tag",
    )
    .with_parameters(vec![
        Parameter {
            name: "response_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Response APDU as hex string (includes SW1 SW2 status bytes)".to_string(),
            required: true,
        },
        Parameter {
            name: "sw1".to_string(),
            type_hint: "string".to_string(),
            description: "Status byte 1 (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "sw2".to_string(),
            type_hint: "string".to_string(),
            description: "Status byte 2 (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Response data (without status bytes) as hex string".to_string(),
            required: false,
        },
    ])
});

/// NFC NDEF data read event - triggered after successfully reading NDEF message
pub static NFC_NDEF_READ_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_ndef_read",
        "NDEF message read from NFC tag",
    )
    .with_parameters(vec![Parameter {
        name: "records".to_string(),
        type_hint: "array".to_string(),
        description: "Array of NDEF records with type, payload, etc.".to_string(),
        required: true,
    }])
});

/// NFC card disconnected event
pub static NFC_CARD_DISCONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_card_disconnected",
        "NFC card/tag disconnected from reader",
    )
});

/// NFC client protocol implementation
pub struct NfcClientProtocol;

impl Protocol for NfcClientProtocol {
    fn protocol_name(&self) -> &'static str {
        "nfc"
    }

    fn stack_name(&self) -> &'static str {
        "application"
    }

    fn get_event_types(&self) -> Vec<&LazyLock<EventType>> {
        vec![
            &NFC_READERS_LISTED_EVENT,
            &NFC_CARD_DETECTED_EVENT,
            &NFC_APDU_RESPONSE_EVENT,
            &NFC_NDEF_READ_EVENT,
            &NFC_CARD_DISCONNECTED_EVENT,
        ]
    }

    fn get_startup_params(&self) -> Vec<Parameter> {
        vec![
            Parameter {
                name: "reader_index".to_string(),
                type_hint: "number".to_string(),
                description: "Index of PC/SC reader to use (0-based, default: 0)".to_string(),
                required: false,
            },
            Parameter {
                name: "reader_name".to_string(),
                type_hint: "string".to_string(),
                description: "Name of specific reader to use (optional, overrides reader_index)"
                    .to_string(),
                required: false,
            },
        ]
    }
}

impl Client for NfcClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            // NFC client uses PC/SC, not socket addresses
            // Remote addr is ignored, we use reader_index from startup params
            crate::client::nfc::NfcClient::connect_with_llm_actions(
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.client_id,
                ctx.startup_params,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "list_readers".to_string(),
                description: "List available NFC/smart card readers via PC/SC".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_readers"
                }),
            },
            ActionDefinition {
                name: "connect_card".to_string(),
                description: "Connect to NFC card/tag in reader (waits for card if not present)"
                    .to_string(),
                parameters: vec![
                    Parameter {
                        name: "timeout_ms".to_string(),
                        type_hint: "number".to_string(),
                        description: "Timeout in milliseconds to wait for card (default: 30000)"
                            .to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "connect_card",
                    "timeout_ms": 30000
                }),
            },
            ActionDefinition {
                name: "disconnect_card".to_string(),
                description: "Disconnect from current card/tag".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect_card"
                }),
            },
        ]
    }

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_apdu".to_string(),
                description: "Send APDU command to card/tag (structured format)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "cla".to_string(),
                        type_hint: "string".to_string(),
                        description: "Class byte (hex, e.g., '00')".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "ins".to_string(),
                        type_hint: "string".to_string(),
                        description: "Instruction byte (hex, e.g., 'A4' for SELECT)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "p1".to_string(),
                        type_hint: "string".to_string(),
                        description: "Parameter 1 byte (hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "p2".to_string(),
                        type_hint: "string".to_string(),
                        description: "Parameter 2 byte (hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data".to_string(),
                        type_hint: "string".to_string(),
                        description: "Command data (hex string, optional)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "le".to_string(),
                        type_hint: "string".to_string(),
                        description: "Expected response length (hex, optional)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "send_apdu",
                    "cla": "00",
                    "ins": "A4",
                    "p1": "04",
                    "p2": "00",
                    "data": "D2760000850101",
                    "le": "00"
                }),
            },
            ActionDefinition {
                name: "send_apdu_raw".to_string(),
                description: "Send raw APDU command (hex string)".to_string(),
                parameters: vec![Parameter {
                    name: "apdu_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Raw APDU command as hex string".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_apdu_raw",
                    "apdu_hex": "00A4040007D276000085010100"
                }),
            },
            ActionDefinition {
                name: "read_ndef".to_string(),
                description: "Read NDEF message from NFC tag (high-level)".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "read_ndef"
                }),
            },
            ActionDefinition {
                name: "write_ndef".to_string(),
                description: "Write NDEF message to NFC tag".to_string(),
                parameters: vec![Parameter {
                    name: "records".to_string(),
                    type_hint: "array".to_string(),
                    description: "Array of NDEF records to write".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "write_ndef",
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
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more events without taking action".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing 'type' field in action"))?;

        match action_type {
            "send_apdu" => {
                // Structured APDU command
                let cla = action["cla"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing 'cla' field"))?;
                let ins = action["ins"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing 'ins' field"))?;
                let p1 = action["p1"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing 'p1' field"))?;
                let p2 = action["p2"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing 'p2' field"))?;
                let data = action["data"].as_str().unwrap_or("");
                let le = action["le"].as_str().unwrap_or("");

                // Construct APDU from structured fields
                let mut apdu_hex = format!("{}{}{}{}", cla, ins, p1, p2);
                if !data.is_empty() {
                    let data_len = data.len() / 2;
                    apdu_hex.push_str(&format!("{:02X}", data_len));
                    apdu_hex.push_str(data);
                }
                if !le.is_empty() {
                    apdu_hex.push_str(le);
                }

                Ok(ClientActionResult::Custom {
                    name: "send_apdu".to_string(),
                    data: json!({ "apdu_hex": apdu_hex }),
                })
            }
            "send_apdu_raw" => {
                let apdu_hex = action["apdu_hex"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing 'apdu_hex' field"))?;

                Ok(ClientActionResult::Custom {
                    name: "send_apdu".to_string(),
                    data: json!({ "apdu_hex": apdu_hex }),
                })
            }
            "read_ndef" => Ok(ClientActionResult::Custom {
                name: "read_ndef".to_string(),
                data: json!({}),
            }),
            "write_ndef" => {
                let records = action["records"]
                    .as_array()
                    .ok_or_else(|| anyhow!("Missing 'records' array"))?;

                Ok(ClientActionResult::Custom {
                    name: "write_ndef".to_string(),
                    data: json!({ "records": records }),
                })
            }
            "disconnect_card" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
