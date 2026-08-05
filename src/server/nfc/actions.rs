//! NFC (Near Field Communication) server protocol actions implementation
//! Virtual NFC tag/card emulation for testing (PC/SC readers typically can't emulate)

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Set the virtual tag's Answer to Reset
pub static SET_ATR_ACTION: LazyLock<ActionDefinition> = LazyLock::new(set_atr_action);
/// Set the virtual tag's NDEF message
pub static SET_NDEF_MESSAGE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(set_ndef_message_action);
/// Answer an APDU command sent to the virtual tag
pub static RESPOND_TO_APDU_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(respond_to_apdu_action);

/// NFC server started event
pub static NFC_SERVER_STARTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_server_started",
        "Virtual NFC tag/card server started (for testing)",
        json!({
            "type": "set_ndef_message",
            "records": [{"type": "text", "language": "en", "text": "Hello NFC!"}]
        }),
    )
    .with_actions(vec![
        SET_ATR_ACTION.clone(),
        SET_NDEF_MESSAGE_ACTION.clone(),
    ])
});

/// NFC tag selected event - triggered when virtual tag's application is selected
pub static NFC_TAG_SELECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_tag_selected",
        "Virtual NFC tag application selected by reader",
        json!({
            "type": "respond_to_apdu",
            "data_hex": "D2760000850101",
            "sw1": "90",
            "sw2": "00"
        }),
    )
    .with_parameters(vec![Parameter {
        name: "application_id".to_string(),
        type_hint: "string".to_string(),
        description: "Application ID (AID) that was selected (hex)".to_string(),
        required: true,
    }])
    .with_actions(vec![RESPOND_TO_APDU_ACTION.clone()])
});

/// NFC APDU command received event - triggered when virtual tag receives APDU
pub static NFC_APDU_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfc_apdu_received",
        "APDU command received by virtual NFC tag",
        json!({
            "type": "respond_to_apdu",
            "data_hex": "D2760000850101",
            "sw1": "90",
            "sw2": "00"
        }),
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
    .with_actions(vec![RESPOND_TO_APDU_ACTION.clone()])
});

/// Action definition for set_atr (async)
fn set_atr_action() -> ActionDefinition {
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFC set ATR ({atr_hex_len} bytes)")
                .with_debug("NFC set_atr: atr={atr_hex}"),
        ),
    }
}

/// Action definition for set_ndef_message (async)
fn set_ndef_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_ndef_message".to_string(),
        description: "Set NDEF message content for virtual tag".to_string(),
        parameters: vec![Parameter {
            name: "records".to_string(),
            type_hint: "array".to_string(),
            description:
                "Array of NDEF records, each an object such as {\"type\": \"text\", \"language\": \"en\", \"text\": \"...\"} or {\"type\": \"uri\", \"uri\": \"...\"}"
                    .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFC set NDEF ({records_len} records)")
                .with_debug("NFC set_ndef_message: records={records_len}"),
        ),
    }
}

/// Action definition for respond_to_apdu (sync)
fn respond_to_apdu_action() -> ActionDefinition {
    ActionDefinition {
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
                description: "Status byte 1 (hex, default: '90' for success)".to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFC APDU response SW={sw1}{sw2}")
                .with_debug("NFC respond_to_apdu: data={data_hex} sw1={sw1} sw2={sw2}"),
        ),
    }
}

/// Parse a hex string, rejecting anything that is not an even run of hex digits
/// so a malformed value fails where it is produced instead of being logged as if
/// it had been accepted.
fn parse_hex(field: &str, value: &str) -> Result<Vec<u8>> {
    hex::decode(value).map_err(|e| anyhow!("Invalid hex in '{field}' ({value}): {e}"))
}

/// NFC server protocol implementation
pub struct NfcServerProtocol;

impl Protocol for NfcServerProtocol {
    fn protocol_name(&self) -> &'static str {
        "nfc"
    }

    fn stack_name(&self) -> &'static str {
        "application"
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .privilege_requirement(PrivilegeRequirement::None)
            .implementation("Virtual NFC tag/card simulation via PC/SC metadata")
            .llm_control("ATR configuration, NDEF message content, APDU response simulation")
            .e2e_testing("Virtual only - cannot test with real readers (hardware cannot emulate)")
            .notes("Simulation only, and no socket is bound. Only nfc_server_started fires: nothing feeds the virtual tag APDUs, so nfc_apdu_received / nfc_tag_selected never fire and respond_to_apdu never runs. Most PC/SC readers cannot emulate cards; use Android HCE or smart card simulator hardware for real card emulation.")
            .build()
    }

    fn description(&self) -> &'static str {
        "Virtual NFC tag/card emulation for testing (simulation only, not usable with real readers)"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            (*NFC_SERVER_STARTED_EVENT).clone(),
            (*NFC_TAG_SELECTED_EVENT).clone(),
            (*NFC_APDU_RECEIVED_EVENT).clone(),
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![SET_ATR_ACTION.clone(), SET_NDEF_MESSAGE_ACTION.clone()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![RESPOND_TO_APDU_ACTION.clone()]
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["nfc", "smart card", "pcsc", "tag", "card emulation"]
    }

    fn example_prompt(&self) -> &'static str {
        "Create a virtual NFC tag on port {AVAILABLE_PORT} that responds to APDU commands"
    }

    fn group_name(&self) -> &'static str {
        "NFC & Smart Cards"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles NFC tag emulation
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "nfc",
                "instruction": "Act as a virtual NFC tag that responds to APDU commands",
                "startup_params": {
                    "tag_type": "type4",
                    "uid": "04A1B2C3D4E5F6"
                }
            }),
            // Script mode: Code-based NFC handling
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "nfc",
                "startup_params": {
                    "tag_type": "type4"
                },
                "event_handlers": [{
                    "event_pattern": "nfc_server_started",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<nfc_server_handler>"
                    }
                }]
            }),
            // Static mode: Fixed NFC tag responses
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "nfc",
                "startup_params": {
                    "tag_type": "type4"
                },
                "event_handlers": [
                    {
                        "event_pattern": "nfc_server_started",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "set_ndef_message",
                                "records": [{"type": "text", "language": "en", "text": "Hello NFC!"}]
                            }]
                        }
                    },
                    {
                        "event_pattern": "nfc_apdu_received",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "respond_to_apdu",
                                "data_hex": "D2760000850101",
                                "sw1": "90",
                                "sw2": "00"
                            }]
                        }
                    }
                ]
            }),
        )
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "tag_type".to_string(),
                type_hint: "string".to_string(),
                description:
                    "Virtual tag type: 'type2' (MIFARE), 'type4' (ISO14443-4), 'generic' (default)"
                        .to_string(),
                required: false,

                example: json!("tag_type"),
            },
            ParameterDefinition {
                name: "uid".to_string(),
                type_hint: "string".to_string(),
                description: "Tag UID (hex, auto-generated if not specified)".to_string(),
                required: false,

                example: json!("uid"),
            },
        ]
    }
}

// Implement Server trait
impl Server for NfcServerProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::nfc::NfcServer;

            // Build startup params JSON manually since StartupParams doesn't expose to_json
            let startup_params_json = if let Some(ref params) = ctx.startup_params {
                serde_json::json!({
                    "tag_type": params.get_optional_string("tag_type")?,
                    "uid": params.get_optional_string("uid")?,
                })
            } else {
                serde_json::json!({})
            };

            NfcServer::start(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                startup_params_json,
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
            "set_atr" => {
                let atr_hex = action
                    .get("atr_hex")
                    .and_then(|v| v.as_str())
                    .context("Missing 'atr_hex' parameter")?;
                parse_hex("atr_hex", atr_hex)?;
                Ok(ActionResult::Custom {
                    name: "set_atr".to_string(),
                    data: json!({ "atr_hex": atr_hex }),
                })
            }
            "set_ndef_message" => {
                let records = action
                    .get("records")
                    .and_then(|v| v.as_array())
                    .context("Missing 'records' parameter")?;
                Ok(ActionResult::Custom {
                    name: "set_ndef_message".to_string(),
                    data: json!({ "records": records }),
                })
            }
            "respond_to_apdu" => {
                let data_hex = action.get("data_hex").and_then(|v| v.as_str());
                if let Some(data_hex) = data_hex {
                    parse_hex("data_hex", data_hex)?;
                }
                let sw1 = action.get("sw1").and_then(|v| v.as_str()).unwrap_or("90");
                let sw2 = action.get("sw2").and_then(|v| v.as_str()).unwrap_or("00");
                parse_hex("sw1", sw1)?;
                parse_hex("sw2", sw2)?;
                Ok(ActionResult::Custom {
                    name: "respond_to_apdu".to_string(),
                    data: json!({
                        "data_hex": data_hex.unwrap_or(""),
                        "sw1": sw1,
                        "sw2": sw2,
                    }),
                })
            }
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
