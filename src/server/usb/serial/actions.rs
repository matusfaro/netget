//! USB CDC ACM Serial protocol actions

#[cfg(feature = "usb-serial")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-serial")]
use crate::protocol::log_template::LogTemplate;
#[cfg(feature = "usb-serial")]
use crate::{protocol::EventType, server::connection::ConnectionId, state::app_state::AppState};
#[cfg(feature = "usb-serial")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-serial")]
use serde_json::json;
#[cfg(feature = "usb-serial")]
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};
#[cfg(feature = "usb-serial")]
use tokio::sync::Mutex;

#[cfg(feature = "usb-serial")]
pub static USB_SERIAL_ATTACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("usb_serial_attached", "Host attached to USB serial port", json!({"type": "placeholder", "event_id": "usb_serial_attached"})).with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        },
    ])
});

#[cfg(feature = "usb-serial")]
pub static USB_SERIAL_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("usb_serial_detached", "Host detached from USB serial port", json!({"type": "placeholder", "event_id": "usb_serial_detached"})).with_parameters(
        vec![Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        }],
    )
});

#[cfg(feature = "usb-serial")]
pub static USB_SERIAL_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("usb_serial_data_received", "Data received from host", json!({"type": "placeholder", "event_id": "usb_serial_data_received"})).with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        },
        Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Received data as string".to_string(),
            required: true,
        },
    ])
});

#[cfg(feature = "usb-serial")]
pub struct UsbSerialProtocol {
    #[allow(dead_code)]
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

#[cfg(feature = "usb-serial")]
#[derive(Clone)]
pub struct ConnectionData {}

#[cfg(feature = "usb-serial")]
impl UsbSerialProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(feature = "usb-serial")]
impl Protocol for UsbSerialProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_data".to_string(),
                description: "Send data to serial port".to_string(),
                parameters: vec![Parameter {
                    name: "data".to_string(),
                    type_hint: "string".to_string(),
                    description: "Data to send".to_string(),
                    required: true,
                }],
                example: json!({"type": "send_data", "data": "Hello\n"}),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> USB serial send {data_len}B")
                        .with_debug("USB-Serial send_data: data='{data}'"),
                ),
            },
            ActionDefinition {
                name: "set_line_coding".to_string(),
                description: "Set baud rate and line parameters".to_string(),
                parameters: vec![
                    Parameter {
                        name: "baud_rate".to_string(),
                        type_hint: "number".to_string(),
                        description: "Bits per second (e.g., 115200)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_bits".to_string(),
                        type_hint: "number".to_string(),
                        description: "5, 6, 7, 8, or 16".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "parity".to_string(),
                        type_hint: "string".to_string(),
                        description: "'none', 'odd', 'even', 'mark', 'space'".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "stop_bits".to_string(),
                        type_hint: "number".to_string(),
                        description: "1, 1.5, or 2".to_string(),
                        required: false,
                    },
                ],
                example: json!({"type": "set_line_coding", "baud_rate": 9600}),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> USB serial line coding {baud_rate} baud")
                        .with_debug("USB-Serial set_line_coding: baud={baud_rate} data_bits={data_bits} parity={parity} stop_bits={stop_bits}"),
                ),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more data".to_string(),
                parameters: vec![],
                example: json!({"type": "wait_for_more"}),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> USB serial wait for more")
                        .with_debug("USB-Serial wait_for_more"),
                ),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "USB-Serial"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            USB_SERIAL_ATTACHED_EVENT.clone(),
            USB_SERIAL_DETACHED_EVENT.clone(),
            USB_SERIAL_DATA_RECEIVED_EVENT.clone(),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "USB>CDC>ACM"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["usb", "serial", "cdc", "acm", "uart", "tty"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        crate::protocol::metadata::ProtocolMetadataV2::builder()
            .state(crate::protocol::metadata::DevelopmentState::Experimental)
            .implementation("Virtual USB CDC ACM serial port using USB/IP protocol")
            .llm_control("LLM controls serial data transmission and line parameters")
            .e2e_testing("E2E tests using Linux usbip client and /dev/ttyACM0")
            .privilege_requirement(crate::protocol::metadata::PrivilegeRequirement::None)
            .notes("Appears as /dev/ttyACM0 on Linux after usbip attach")
            .build()
    }
    fn description(&self) -> &'static str {
        "Virtual USB serial port (CDC ACM)"
    }
    fn example_prompt(&self) -> &'static str {
        "Create a USB serial port and echo back any data received"
    }
    fn group_name(&self) -> &'static str {
        "USB Devices"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles USB serial device
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "usb-serial",
                "instruction": "Create a USB serial port and echo back any data received"
            }),
            // Script mode: Code-based serial handling
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "usb-serial",
                "event_handlers": [{
                    "event_pattern": "usb_serial_data_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<serial_handler>"
                    }
                }]
            }),
            // Static mode: Fixed serial response
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "usb-serial",
                "event_handlers": [{
                    "event_pattern": "usb_serial_attached",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_data",
                            "data": "Welcome to NetGet USB Serial!\r\n"
                        }]
                    }
                }]
            }),
        )
    }
}

#[cfg(feature = "usb-serial")]
impl Server for UsbSerialProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            crate::server::usb::serial::UsbSerialServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;
        match action_type {
            "send_data" => {
                let _data = action["data"]
                    .as_str()
                    .context("send_data requires 'data' field")?;
                // TODO: Implement serial data transmission via USB/IP
                Ok(ActionResult::NoAction)
            }
            "set_line_coding" => {
                let _baud_rate = action["baud_rate"]
                    .as_u64()
                    .context("Requires 'baud_rate'")? as u32;
                // TODO: Implement line coding configuration
                Ok(ActionResult::NoAction)
            }
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
