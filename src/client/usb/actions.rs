//! USB client protocol actions implementation

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

/// USB device opened event
pub static USB_DEVICE_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_device_opened",
        "USB device successfully opened and interface claimed",
        json!({
            "type": "control_transfer",
            "request_type": 0x80,
            "request": 0x06,
            "value": 0x0100,
            "index": 0,
            "length": 18
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "vendor_id".to_string(),
            type_hint: "string".to_string(),
            description: "USB vendor ID (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "product_id".to_string(),
            type_hint: "string".to_string(),
            description: "USB product ID (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "manufacturer".to_string(),
            type_hint: "string".to_string(),
            description: "Device manufacturer string".to_string(),
            required: false,
        },
        Parameter {
            name: "product".to_string(),
            type_hint: "string".to_string(),
            description: "Device product string".to_string(),
            required: false,
        },
    ])
});

/// USB control transfer response event
pub static USB_CONTROL_RESPONSE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_control_response",
        "Response received from USB control transfer",
        json!({
            "type": "wait_for_more"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Response data (as hex string)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of response in bytes".to_string(),
            required: true,
        },
    ])
});

/// USB bulk data received event
pub static USB_BULK_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_bulk_data_received",
        "Data received from USB bulk endpoint",
        json!({
            "type": "bulk_transfer_out",
            "endpoint": 0x02,
            "data_hex": "48656c6c6f"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "The data received (as hex string)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of data in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "endpoint".to_string(),
            type_hint: "number".to_string(),
            description: "Endpoint address".to_string(),
            required: true,
        },
    ])
});

/// USB interrupt data received event
pub static USB_INTERRUPT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_interrupt_data_received",
        "Data received from USB interrupt endpoint",
        json!({
            "type": "wait_for_more"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "The data received (as hex string)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of data in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "endpoint".to_string(),
            type_hint: "number".to_string(),
            description: "Endpoint address".to_string(),
            required: true,
        },
    ])
});

/// USB client protocol action handler
pub struct UsbClientProtocol;

impl UsbClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for UsbClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "list_usb_devices".to_string(),
                description: "List all connected USB devices".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_usb_devices"
                }),
            },
            ActionDefinition {
                name: "detach_device".to_string(),
                description: "Detach from the USB device and close connection".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "detach_device"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "control_transfer".to_string(),
                description: "Send a USB control transfer request".to_string(),
                parameters: vec![
                    Parameter {
                        name: "request_type".to_string(),
                        type_hint: "number".to_string(),
                        description: "Request type byte (bmRequestType)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "request".to_string(),
                        type_hint: "number".to_string(),
                        description: "Request byte (bRequest)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "value".to_string(),
                        type_hint: "number".to_string(),
                        description: "Value word (wValue)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "number".to_string(),
                        description: "Index word (wIndex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Data to send (hex encoded, empty for IN transfers)"
                            .to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "length".to_string(),
                        type_hint: "number".to_string(),
                        description: "Expected response length (for IN transfers)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "control_transfer",
                    "request_type": 0x80,
                    "request": 0x06,
                    "value": 0x0100,
                    "index": 0,
                    "length": 18
                }),
            },
            ActionDefinition {
                name: "bulk_transfer_out".to_string(),
                description: "Send data via USB bulk OUT endpoint".to_string(),
                parameters: vec![
                    Parameter {
                        name: "endpoint".to_string(),
                        type_hint: "number".to_string(),
                        description: "Bulk OUT endpoint address".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Data to send (hex encoded)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "bulk_transfer_out",
                    "endpoint": 0x02,
                    "data_hex": "48656c6c6f"
                }),
            },
            ActionDefinition {
                name: "bulk_transfer_in".to_string(),
                description: "Read data from USB bulk IN endpoint".to_string(),
                parameters: vec![
                    Parameter {
                        name: "endpoint".to_string(),
                        type_hint: "number".to_string(),
                        description: "Bulk IN endpoint address".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "length".to_string(),
                        type_hint: "number".to_string(),
                        description: "Number of bytes to read".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "bulk_transfer_in",
                    "endpoint": 0x81,
                    "length": 64
                }),
            },
            ActionDefinition {
                name: "interrupt_transfer_in".to_string(),
                description: "Read data from USB interrupt IN endpoint".to_string(),
                parameters: vec![
                    Parameter {
                        name: "endpoint".to_string(),
                        type_hint: "number".to_string(),
                        description: "Interrupt IN endpoint address".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "length".to_string(),
                        type_hint: "number".to_string(),
                        description: "Number of bytes to read".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "interrupt_transfer_in",
                    "endpoint": 0x83,
                    "length": 8
                }),
            },
            ActionDefinition {
                name: "claim_interface".to_string(),
                description: "Claim a USB interface for exclusive access".to_string(),
                parameters: vec![Parameter {
                    name: "interface_number".to_string(),
                    type_hint: "number".to_string(),
                    description: "Interface number to claim".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "claim_interface",
                    "interface_number": 0
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more data before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "USB"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("usb_device_opened", "Triggered when USB device is opened and interface claimed", json!({"type": "placeholder", "event_id": "usb_device_opened"})),
            EventType::new("usb_control_response", "Triggered when USB control transfer completes", json!({"type": "placeholder", "event_id": "usb_control_response"})),
            EventType::new("usb_bulk_data_received", "Triggered when data is received from bulk endpoint", json!({"type": "placeholder", "event_id": "usb_bulk_data_received"})),
            EventType::new("usb_interrupt_data_received", "Triggered when data is received from interrupt endpoint", json!({"type": "placeholder", "event_id": "usb_interrupt_data_received"})),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "USB"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["usb", "usb device", "usb client", "connect to usb"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("nusb pure-Rust USB library for device access")
            .llm_control("Full control over USB transfers (control, bulk, interrupt)")
            .e2e_testing("Requires actual USB device for testing")
            .build()
    }

    fn description(&self) -> &'static str {
        "USB client for low-level USB device interaction"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to USB device with vendor ID 0x1234, product ID 0x5678"
    }

    fn group_name(&self) -> &'static str {
        "Hardware"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles USB device operations
            json!({
                "type": "open_client",
                "remote_addr": "usb:1234:5678",
                "base_stack": "usb",
                "instruction": "Connect to USB device and get device descriptor"
            }),
            // Script mode: Code-based USB handling
            json!({
                "type": "open_client",
                "remote_addr": "usb:1234:5678",
                "base_stack": "usb",
                "event_handlers": [{
                    "event_pattern": "usb_device_opened",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<usb_client_handler>"
                    }
                }]
            }),
            // Static mode: Fixed USB action
            json!({
                "type": "open_client",
                "remote_addr": "usb:1234:5678",
                "base_stack": "usb",
                "event_handlers": [{
                    "event_pattern": "usb_device_opened",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "control_transfer",
                            "request_type": 128,
                            "request": 6,
                            "value": 256,
                            "index": 0,
                            "length": 18
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Client trait (client-specific functionality)
impl Client for UsbClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::usb::UsbClient;
            UsbClient::connect_with_llm_actions(
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
            "control_transfer" => {
                let request_type = action
                    .get("request_type")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'request_type' field")?
                    as u8;

                let request = action
                    .get("request")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'request' field")? as u8;

                let value = action
                    .get("value")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'value' field")? as u16;

                let index = action
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'index' field")? as u16;

                let data_hex = action
                    .get("data_hex")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let length = action.get("length").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

                let data = if !data_hex.is_empty() {
                    hex::decode(data_hex).context("Invalid hex data")?
                } else {
                    Vec::new()
                };

                Ok(ClientActionResult::Custom {
                    name: "control_transfer".to_string(),
                    data: json!({
                        "request_type": request_type,
                        "request": request,
                        "value": value,
                        "index": index,
                        "data": data,
                        "length": length,
                    }),
                })
            }
            "bulk_transfer_out" => {
                let endpoint = action
                    .get("endpoint")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'endpoint' field")? as u8;

                let data_hex = action
                    .get("data_hex")
                    .and_then(|v| v.as_str())
                    .context("Missing 'data_hex' field")?;

                let data = hex::decode(data_hex).context("Invalid hex data")?;

                Ok(ClientActionResult::Custom {
                    name: "bulk_transfer_out".to_string(),
                    data: json!({
                        "endpoint": endpoint,
                        "data": data,
                    }),
                })
            }
            "bulk_transfer_in" => {
                let endpoint = action
                    .get("endpoint")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'endpoint' field")? as u8;

                let length = action
                    .get("length")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'length' field")? as usize;

                Ok(ClientActionResult::Custom {
                    name: "bulk_transfer_in".to_string(),
                    data: json!({
                        "endpoint": endpoint,
                        "length": length,
                    }),
                })
            }
            "interrupt_transfer_in" => {
                let endpoint = action
                    .get("endpoint")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'endpoint' field")? as u8;

                let length = action
                    .get("length")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'length' field")? as usize;

                Ok(ClientActionResult::Custom {
                    name: "interrupt_transfer_in".to_string(),
                    data: json!({
                        "endpoint": endpoint,
                        "length": length,
                    }),
                })
            }
            "claim_interface" => {
                let interface_number = action
                    .get("interface_number")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'interface_number' field")?
                    as u8;

                Ok(ClientActionResult::Custom {
                    name: "claim_interface".to_string(),
                    data: json!({
                        "interface_number": interface_number,
                    }),
                })
            }
            "detach_device" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown USB client action: {}",
                action_type
            )),
        }
    }
}
