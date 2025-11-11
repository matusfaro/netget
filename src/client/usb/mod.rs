//! USB client implementation
pub mod actions;

pub use actions::UsbClientProtocol;

use anyhow::{anyhow, Context, Result};
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, RequestBuffer};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::usb::actions::{
    USB_BULK_DATA_RECEIVED_EVENT, USB_CONTROL_RESPONSE_EVENT, USB_DEVICE_OPENED_EVENT,
    USB_INTERRUPT_DATA_RECEIVED_EVENT,
};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    #[allow(dead_code)]
    state: ConnectionState,
    #[allow(dead_code)]
    queued_events: Vec<Event>,
    memory: String,
}

/// USB device information parsed from remote_addr or instruction
#[derive(Debug, Clone)]
struct UsbDeviceInfo {
    vendor_id: u16,
    product_id: u16,
    interface_number: u8,
}

impl UsbDeviceInfo {
    /// Parse USB device info from string like "vid:1234,pid:5678" or "1234:5678"
    fn from_string(s: &str) -> Result<Self> {
        // Try formats:
        // - "vid:1234,pid:5678,interface:0"
        // - "1234:5678:0"
        // - "0x1234:0x5678:0"

        let parts: Vec<&str> = s.split(',').collect();

        let mut vendor_id: Option<u16> = None;
        let mut product_id: Option<u16> = None;
        let mut interface_number: u8 = 0; // Default to interface 0

        if parts.len() == 1 {
            // Colon-separated format: "1234:5678" or "1234:5678:0"
            let colon_parts: Vec<&str> = s.split(':').collect();
            if colon_parts.len() >= 2 {
                vendor_id = Some(Self::parse_hex_u16(colon_parts[0])?);
                product_id = Some(Self::parse_hex_u16(colon_parts[1])?);
                if colon_parts.len() >= 3 {
                    interface_number = colon_parts[2].parse()?;
                }
            }
        } else {
            // Comma-separated key:value format
            for part in parts {
                let kv: Vec<&str> = part.trim().split(':').collect();
                if kv.len() == 2 {
                    match kv[0].trim().to_lowercase().as_str() {
                        "vid" | "vendor" => vendor_id = Some(Self::parse_hex_u16(kv[1].trim())?),
                        "pid" | "product" => product_id = Some(Self::parse_hex_u16(kv[1].trim())?),
                        "interface" | "if" => interface_number = kv[1].trim().parse()?,
                        _ => {}
                    }
                }
            }
        }

        Ok(UsbDeviceInfo {
            vendor_id: vendor_id.ok_or_else(|| anyhow!("Missing vendor_id"))?,
            product_id: product_id.ok_or_else(|| anyhow!("Missing product_id"))?,
            interface_number,
        })
    }

    /// Parse hex string with optional 0x prefix
    fn parse_hex_u16(s: &str) -> Result<u16> {
        let s = s.trim();
        if s.starts_with("0x") || s.starts_with("0X") {
            u16::from_str_radix(&s[2..], 16).context("Invalid hex number")
        } else {
            // Try hex first, then decimal
            u16::from_str_radix(s, 16)
                .or_else(|_| s.parse::<u16>())
                .context("Invalid number")
        }
    }
}

/// USB client that connects to a USB device
pub struct UsbClient;

impl UsbClient {
    /// Connect to a USB device with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse USB device info
        let device_info = UsbDeviceInfo::from_string(&remote_addr)
            .context("Failed to parse USB device info from remote_addr")?;

        info!(
            "USB client {} connecting to device VID:{:04x} PID:{:04x}",
            client_id, device_info.vendor_id, device_info.product_id
        );

        // Find and open USB device
        let device_info_clone = device_info.clone();
        let device = tokio::task::spawn_blocking(move || {
            let devices = nusb::list_devices()
                .context("Failed to list USB devices")?;

            for dev_info in devices {
                if dev_info.vendor_id() == device_info_clone.vendor_id
                    && dev_info.product_id() == device_info_clone.product_id
                {
                    return dev_info.open().context("Failed to open USB device");
                }
            }

            Err(anyhow!(
                "USB device VID:{:04x} PID:{:04x} not found",
                device_info_clone.vendor_id,
                device_info_clone.product_id
            ))
        })
        .await??;

        // Get manufacturer/product strings
        // Use language ID 0x0409 (English - United States)
        const LANG_EN_US: u16 = 0x0409;

        let (manufacturer, product) = tokio::task::spawn_blocking({
            let device = device.clone();
            move || -> Result<(Option<String>, Option<String>)> {
                // Try to get manufacturer string (index 1 is common)
                let manufacturer = device.get_string_descriptor(1, LANG_EN_US, Duration::from_secs(1)).ok();

                // Try to get product string (index 2 is common)
                let product = device.get_string_descriptor(2, LANG_EN_US, Duration::from_secs(1)).ok();

                Ok((manufacturer, product))
            }
        })
        .await??;

        info!(
            "USB client {} opened device: {} {}",
            client_id,
            manufacturer.as_deref().unwrap_or("Unknown"),
            product.as_deref().unwrap_or("Unknown")
        );

        // Claim interface
        let interface = tokio::task::spawn_blocking({
            let device = device.clone();
            let interface_num = device_info.interface_number;
            move || {
                device.claim_interface(interface_num)
                    .context(format!("Failed to claim interface {}", interface_num))
            }
        })
        .await??;

        info!(
            "USB client {} claimed interface {}",
            client_id, device_info.interface_number
        );

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] USB client {} connected to VID:{:04x} PID:{:04x}",
            client_id, device_info.vendor_id, device_info.product_id
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_events: Vec::new(),
            memory: String::new(),
        }));

        // Create a fake socket address (USB doesn't use sockets)
        // We'll use the vendor/product IDs encoded in the address
        let fake_addr: SocketAddr = format!(
            "127.{}.{}.{}:{}",
            (device_info.vendor_id >> 8) & 0xFF,
            device_info.vendor_id & 0xFF,
            (device_info.product_id >> 8) & 0xFF,
            device_info.product_id & 0xFF
        )
        .parse()
        .unwrap();

        // Send initial connected event
        let protocol = Arc::new(UsbClientProtocol::new());
        let event = Event::new(
            &USB_DEVICE_OPENED_EVENT,
            serde_json::json!({
                "vendor_id": format!("{:04x}", device_info.vendor_id),
                "product_id": format!("{:04x}", device_info.product_id),
                "manufacturer": manufacturer.unwrap_or_else(|| "Unknown".to_string()),
                "product": product.unwrap_or_else(|| "Unknown".to_string()),
            }),
        );

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute initial actions
                    Self::execute_actions(
                        actions,
                        protocol.clone(),
                        &interface,
                        client_id,
                        &app_state,
                        &llm_client,
                        &status_tx,
                        &client_data,
                    )
                    .await;
                }
                Err(e) => {
                    error!("LLM error for USB client {}: {}", client_id, e);
                }
            }
        }

        // Spawn monitoring task (USB devices don't actively send data like sockets)
        // The LLM will initiate transfers via actions
        tokio::spawn(async move {
            // Keep client alive - cleanup handled on disconnect
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });

        Ok(fake_addr)
    }

    /// Execute USB actions from LLM
    async fn execute_actions(
        actions: Vec<serde_json::Value>,
        protocol: Arc<UsbClientProtocol>,
        interface: &nusb::Interface,
        client_id: ClientId,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
        client_data: &Arc<Mutex<ClientData>>,
    ) {
        use crate::llm::actions::client_trait::Client;

        for action in actions {
            match protocol.as_ref().execute_action(action) {
                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom {
                    name,
                    data,
                }) => {
                    match name.as_str() {
                        "control_transfer" => {
                            let request_type = data["request_type"].as_u64().unwrap() as u8;
                            let request = data["request"].as_u64().unwrap() as u8;
                            let value = data["value"].as_u64().unwrap() as u16;
                            let index = data["index"].as_u64().unwrap() as u16;
                            let out_data = data["data"]
                                .as_array()
                                .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect::<Vec<u8>>())
                                .unwrap_or_default();
                            let length = data["length"].as_u64().unwrap_or(0) as usize;

                            trace!(
                                "USB client {} control transfer: type={:02x} req={:02x} val={:04x} idx={:04x}",
                                client_id, request_type, request, value, index
                            );

                            // Execute control transfer
                            let interface_clone = interface.clone();
                            let result = if out_data.is_empty() && length > 0 {
                                // IN transfer
                                let control_in = ControlIn {
                                    control_type: ControlType::Vendor,
                                    recipient: Recipient::Device,
                                    request,
                                    value,
                                    index,
                                    length: length as u16,
                                };
                                let result = interface_clone.control_in(control_in).await;
                                Ok::<Vec<u8>, nusb::Error>(result.data.to_vec())
                            } else {
                                // OUT transfer
                                let control_out = ControlOut {
                                    control_type: ControlType::Vendor,
                                    recipient: Recipient::Device,
                                    request,
                                    value,
                                    index,
                                    data: &out_data,
                                };
                                let _ = interface_clone.control_out(control_out).await;
                                Ok(Vec::new())
                            };

                            match result {
                                Ok(response_data) if !response_data.is_empty() => {
                                    debug!(
                                        "USB client {} control transfer received {} bytes",
                                        client_id,
                                        response_data.len()
                                    );

                                    // Send event to LLM
                                    let event = Event::new(
                                        &USB_CONTROL_RESPONSE_EVENT,
                                        serde_json::json!({
                                            "data_hex": hex::encode(&response_data),
                                            "data_length": response_data.len(),
                                        }),
                                    );

                                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                        if let Ok(ClientLlmResult { actions: new_actions, memory_updates }) = call_llm_for_client(
                                            llm_client,
                                            app_state,
                                            client_id.to_string(),
                                            &instruction,
                                            &client_data.lock().await.memory,
                                            Some(&event),
                                            protocol.as_ref(),
                                            status_tx,
                                        ).await {
                                            if let Some(mem) = memory_updates {
                                                client_data.lock().await.memory = mem;
                                            }
                                            Box::pin(Self::execute_actions(
                                                new_actions,
                                                protocol.clone(),
                                                interface,
                                                client_id,
                                                app_state,
                                                llm_client,
                                                status_tx,
                                                client_data,
                                            )).await;
                                        }
                                    }
                                }
                                Ok(_) => {
                                    trace!("USB client {} control transfer completed", client_id);
                                }
                                Err(e) => {
                                    error!("USB client {} control transfer error: {}", client_id, e);
                                }
                            }
                        }
                        "bulk_transfer_out" => {
                            let endpoint = data["endpoint"].as_u64().unwrap() as u8;
                            let out_data = data["data"]
                                .as_array()
                                .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect::<Vec<u8>>())
                                .unwrap_or_default();

                            trace!(
                                "USB client {} bulk OUT transfer: endpoint={:02x} length={}",
                                client_id, endpoint, out_data.len()
                            );

                            let interface_clone = interface.clone();
                            let result = interface_clone.bulk_out(endpoint, out_data).await;

                            match result.status {
                                Ok(_) => {
                                    trace!("USB client {} bulk OUT completed", client_id);
                                }
                                Err(e) => {
                                    error!("USB client {} bulk OUT error: {}", client_id, e);
                                }
                            }
                        }
                        "bulk_transfer_in" => {
                            let endpoint = data["endpoint"].as_u64().unwrap() as u8;
                            let length = data["length"].as_u64().unwrap() as usize;

                            trace!(
                                "USB client {} bulk IN transfer: endpoint={:02x} length={}",
                                client_id, endpoint, length
                            );

                            let interface_clone = interface.clone();
                            let buffer = RequestBuffer::new(length);
                            let result = interface_clone.bulk_in(endpoint, buffer).await;

                            let response_data = result.data.to_vec();
                            if !response_data.is_empty() {
                                    debug!(
                                        "USB client {} bulk IN received {} bytes",
                                        client_id,
                                        response_data.len()
                                    );

                                    // Send event to LLM
                                    let event = Event::new(
                                        &USB_BULK_DATA_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "data_hex": hex::encode(&response_data),
                                            "data_length": response_data.len(),
                                            "endpoint": endpoint,
                                        }),
                                    );

                                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                        if let Ok(ClientLlmResult { actions: new_actions, memory_updates }) = call_llm_for_client(
                                            llm_client,
                                            app_state,
                                            client_id.to_string(),
                                            &instruction,
                                            &client_data.lock().await.memory,
                                            Some(&event),
                                            protocol.as_ref(),
                                            status_tx,
                                        ).await {
                                            if let Some(mem) = memory_updates {
                                                client_data.lock().await.memory = mem;
                                            }
                                            Box::pin(Self::execute_actions(
                                                new_actions,
                                                protocol.clone(),
                                                interface,
                                                client_id,
                                                app_state,
                                                llm_client,
                                                status_tx,
                                                client_data,
                                            )).await;
                                        }
                                    }
                            }
                        }
                        "interrupt_transfer_in" => {
                            let endpoint = data["endpoint"].as_u64().unwrap() as u8;
                            let length = data["length"].as_u64().unwrap() as usize;

                            trace!(
                                "USB client {} interrupt IN transfer: endpoint={:02x} length={}",
                                client_id, endpoint, length
                            );

                            let interface_clone = interface.clone();
                            let buffer = RequestBuffer::new(length);
                            let result = interface_clone.interrupt_in(endpoint, buffer).await;

                            let response_data = result.data.to_vec();
                            if !response_data.is_empty() {
                                    debug!(
                                        "USB client {} interrupt IN received {} bytes",
                                        client_id,
                                        response_data.len()
                                    );

                                    // Send event to LLM
                                    let event = Event::new(
                                        &USB_INTERRUPT_DATA_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "data_hex": hex::encode(&response_data),
                                            "data_length": response_data.len(),
                                            "endpoint": endpoint,
                                        }),
                                    );

                                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                        if let Ok(ClientLlmResult { actions: new_actions, memory_updates }) = call_llm_for_client(
                                            llm_client,
                                            app_state,
                                            client_id.to_string(),
                                            &instruction,
                                            &client_data.lock().await.memory,
                                            Some(&event),
                                            protocol.as_ref(),
                                            status_tx,
                                        ).await {
                                            if let Some(mem) = memory_updates {
                                                client_data.lock().await.memory = mem;
                                            }
                                            Box::pin(Self::execute_actions(
                                                new_actions,
                                                protocol.clone(),
                                                interface,
                                                client_id,
                                                app_state,
                                                llm_client,
                                                status_tx,
                                                client_data,
                                            )).await;
                                        }
                                    }
                            }
                        }
                        "claim_interface" => {
                            let interface_num = data["interface_number"].as_u64().unwrap() as u8;
                            info!(
                                "USB client {} interface {} already claimed, skipping",
                                client_id, interface_num
                            );
                        }
                        _ => {
                            warn!("USB client {} unknown custom action: {}", client_id, name);
                        }
                    }
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                    info!("USB client {} disconnecting", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    error!("USB client {} action error: {}", client_id, e);
                }
            }
        }
    }
}
