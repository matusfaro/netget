//! NFC (Near Field Communication) server protocol actions implementation
//! Virtual NFC tag/card emulation for testing (PC/SC readers typically can't emulate)

use crate::llm::actions::{
    protocol_trait::Protocol, ActionDefinition, ParameterDefinition,
};
use crate::llm::ActionResult;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
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
    .with_parameters(vec![ParameterDefinition {
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
        ParameterDefinition {
            name: "apdu_hex".to_string(),
            type_hint: "string".to_string(),
            description: "APDU command as hex string".to_string(),
            required: true,
        },
        ParameterDefinition {
            name: "cla".to_string(),
            type_hint: "string".to_string(),
            description: "Class byte (hex)".to_string(),
            required: true,
        },
        ParameterDefinition {
            name: "ins".to_string(),
            type_hint: "string".to_string(),
            description: "Instruction byte (hex)".to_string(),
            required: true,
        },
        ParameterDefinition {
            name: "p1".to_string(),
            type_hint: "string".to_string(),
            description: "Parameter 1 (hex)".to_string(),
            required: true,
        },
        ParameterDefinition {
            name: "p2".to_string(),
            type_hint: "string".to_string(),
            description: "Parameter 2 (hex)".to_string(),
            required: true,
        },
        ParameterDefinition {
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
            .notes("Simulation only. Most PC/SC readers cannot emulate cards. Use Android HCE or smart card simulator hardware for real card emulation.")
            .build()
    }

    fn description(&self) -> &'static str {
        "Virtual NFC tag/card emulation for testing (simulation only, not usable with real readers)"
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
                parameters: vec![ParameterDefinition {
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
                parameters: vec![ParameterDefinition {
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
                ParameterDefinition {
                    name: "data_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Response data (hex, optional)".to_string(),
                    required: false,
                },
                ParameterDefinition {
                    name: "sw1".to_string(),
                    type_hint: "string".to_string(),
                    description:
                        "Status byte 1 (hex, default: '90' for success)".to_string(),
                    required: false,
                },
                ParameterDefinition {
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
        }]
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "tag_type".to_string(),
                type_hint: "string".to_string(),
                description: "Virtual tag type: 'type2' (MIFARE), 'type4' (ISO14443-4), 'generic' (default)"
                    .to_string(),
                required: false,
            },
            ParameterDefinition {
                name: "uid".to_string(),
                type_hint: "string".to_string(),
                description: "Tag UID (hex, auto-generated if not specified)".to_string(),
                required: false,
            },
        ]
    }
}
