//! USB CDC ACM Serial protocol actions

#[cfg(feature = "usb-serial")]
use crate::llm::actions::{protocol_trait::{ActionResult, Protocol, Server}, ActionDefinition, Parameter};
#[cfg(feature = "usb-serial")]
use crate::{protocol::EventType, server::connection::ConnectionId, state::app_state::AppState};
#[cfg(feature = "usb-serial")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-serial")]
use serde_json::json;
#[cfg(feature = "usb-serial")]
use std::{collections::HashMap, sync::{Arc, LazyLock}};
#[cfg(feature = "usb-serial")]
use tokio::sync::Mutex;

#[cfg(feature = "usb-serial")]
pub static USB_SERIAL_ATTACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("usb_serial_attached", "Host attached to USB serial port")
        .with_parameters(vec![Parameter::new("connection_id", "string", "Connection ID")])
});

#[cfg(feature = "usb-serial")]
pub static USB_SERIAL_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("usb_serial_detached", "Host detached from USB serial port")
        .with_parameters(vec![Parameter::new("connection_id", "string", "Connection ID")])
});

#[cfg(feature = "usb-serial")]
pub static USB_SERIAL_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("usb_serial_data_received", "Data received from host")
        .with_parameters(vec![
            Parameter::new("connection_id", "string", "Connection ID"),
            Parameter::new("data", "string", "Received data as string")
        ])
});

#[cfg(feature = "usb-serial")]
pub struct UsbSerialProtocol {
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

#[cfg(feature = "usb-serial")]
pub struct ConnectionData {}

#[cfg(feature = "usb-serial")]
impl UsbSerialProtocol {
    pub fn new() -> Self {
        Self { connections: Arc::new(Mutex::new(HashMap::new())) }
    }
}

#[cfg(feature = "usb-serial")]
impl Protocol for UsbSerialProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> { vec![] }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> { vec![] }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_data".to_string(),
                description: "Send data to serial port".to_string(),
                parameters: vec![Parameter::new("data", "string", "Data to send")],
                example: json!({"type": "send_data", "data": "Hello\n"}),
            },
            ActionDefinition {
                name: "set_line_coding".to_string(),
                description: "Set baud rate and line parameters".to_string(),
                parameters: vec![
                    Parameter::new("baud_rate", "number", "Bits per second (e.g., 115200)"),
                    Parameter::new("data_bits", "number", "5, 6, 7, 8, or 16").optional(),
                    Parameter::new("parity", "string", "'none', 'odd', 'even', 'mark', 'space'").optional(),
                    Parameter::new("stop_bits", "number", "1, 1.5, or 2").optional(),
                ],
                example: json!({"type": "set_line_coding", "baud_rate": 9600}),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more data".to_string(),
                parameters: vec![],
                example: json!({"type": "wait_for_more"}),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str { "USB-Serial" }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![USB_SERIAL_ATTACHED_EVENT.clone(), USB_SERIAL_DETACHED_EVENT.clone(), USB_SERIAL_DATA_RECEIVED_EVENT.clone()]
    }
    fn stack_name(&self) -> &'static str { "USB>CDC>ACM" }
    fn keywords(&self) -> Vec<&'static str> { vec!["usb", "serial", "cdc", "acm", "uart", "tty"] }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        crate::protocol::metadata::ProtocolMetadataV2::new(
            crate::protocol::metadata::ProtocolState::Experimental,
            "Virtual USB CDC ACM serial port using USB/IP protocol",
            "LLM controls serial data transmission and line parameters",
            "E2E tests using Linux usbip client and /dev/ttyACM0",
            crate::protocol::metadata::PrivilegeRequirement::None,
        ).with_notes("Appears as /dev/ttyACM0 on Linux after usbip attach")
    }
    fn description(&self) -> &'static str { "Virtual USB serial port (CDC ACM)" }
    fn example_prompt(&self) -> &'static str { "Create a USB serial port and echo back any data received" }
    fn group_name(&self) -> &'static str { "USB Devices" }
}

#[cfg(feature = "usb-serial")]
impl Server for UsbSerialProtocol {
    fn spawn(&self, ctx: crate::protocol::SpawnContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::server::usb::serial::UsbSerialServer::spawn_with_llm_actions(
                ctx.listen_addr, ctx.llm_client, ctx.state, ctx.status_tx, ctx.server_id
            ).await
        })
    }

    fn execute_action(&self, action: serde_json::Value, _connection_id: Option<ConnectionId>, _app_state: &AppState) -> Result<ActionResult> {
        let action_type = action["type"].as_str().context("Action must have 'type' field")?;
        match action_type {
            "send_data" => {
                let data = action["data"].as_str().context("send_data requires 'data' field")?;
                // TODO: Implement serial data transmission via USB/IP
                Ok(ActionResult::NoAction)
            }
            "set_line_coding" => {
                let baud_rate = action["baud_rate"].as_u64().context("Requires 'baud_rate'")? as u32;
                // TODO: Implement line coding configuration
                Ok(ActionResult::NoAction)
            }
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
