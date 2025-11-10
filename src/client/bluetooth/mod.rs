//! Bluetooth Low Energy (BLE) client implementation
pub mod actions;

pub use actions::BluetoothClientProtocol;

use anyhow::{Context, Result};
use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::bluetooth::actions::{
    BLUETOOTH_CONNECTED_EVENT, BLUETOOTH_DATA_READ_EVENT,
    BLUETOOTH_NOTIFICATION_RECEIVED_EVENT, BLUETOOTH_SCAN_COMPLETE_EVENT,
    BLUETOOTH_SERVICES_DISCOVERED_EVENT,
};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    memory: String,
    peripheral: Option<Peripheral>,
    manager: Manager,
    adapter: Adapter,
}

/// Bluetooth Low Energy client that connects to BLE devices
pub struct BluetoothClient;

impl BluetoothClient {
    /// Connect to a BLE device with integrated LLM actions
    ///
    /// Note: For BLE, the "remote_addr" parameter is actually the device name or address to connect to.
    /// If empty or "scan", the client will scan for devices and wait for LLM to select one.
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("Bluetooth client {} initializing", client_id);

        // Initialize BLE manager and adapter
        let manager = Manager::new()
            .await
            .context("Failed to create BLE manager")?;

        let adapters = manager.adapters().await?;
        let adapter = adapters
            .into_iter()
            .next()
            .context("No Bluetooth adapters found")?;

        info!("Bluetooth client {} using adapter: {:?}", client_id, adapter.adapter_info().await?);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] Bluetooth client {} initialized", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            memory: String::new(),
            peripheral: None,
            manager: manager.clone(),
            adapter: adapter.clone(),
        }));

        // Spawn LLM integration task
        let client_data_clone = client_data.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();

        tokio::spawn(async move {
            // Initial scan if requested
            if remote_addr.is_empty() || remote_addr == "scan" {
                if let Err(e) = Self::perform_scan(
                    &client_data_clone,
                    &app_state_clone,
                    &status_tx_clone,
                    &llm_client_clone,
                    client_id,
                    5,
                ).await {
                    error!("Bluetooth scan error: {}", e);
                }
            } else {
                // Try to connect to specified device
                if let Err(e) = Self::connect_to_device(
                    &client_data_clone,
                    &app_state_clone,
                    &status_tx_clone,
                    &llm_client_clone,
                    client_id,
                    Some(remote_addr.clone()),
                    None,
                ).await {
                    error!("Bluetooth connection error: {}", e);
                    app_state_clone.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                    let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                }
            }

            // Main event loop - wait for LLM actions
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                // Check if client is still active
                if let Some(client) = app_state_clone.get_client(client_id).await {
                    if matches!(client.status, ClientStatus::Disconnected | ClientStatus::Error(_)) {
                        break;
                    }
                } else {
                    break;
                }
            }

            info!("Bluetooth client {} event loop ended", client_id);
        });

        // Return a dummy socket address (BLE doesn't use IP sockets)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Perform a BLE scan for nearby devices
    async fn perform_scan(
        client_data: &Arc<Mutex<ClientData>>,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        llm_client: &OllamaClient,
        client_id: ClientId,
        duration_secs: u64,
    ) -> Result<()> {
        let adapter = {
            let data = client_data.lock().await;
            data.adapter.clone()
        };

        info!("Bluetooth client {} starting scan for {} seconds", client_id, duration_secs);
        let _ = status_tx.send(format!("[CLIENT] Scanning for BLE devices..."));

        adapter.start_scan(ScanFilter::default()).await?;
        tokio::time::sleep(Duration::from_secs(duration_secs)).await;
        adapter.stop_scan().await?;

        // Get discovered devices
        let peripherals = adapter.peripherals().await?;

        let mut devices = Vec::new();
        for peripheral in peripherals {
            if let Ok(Some(props)) = peripheral.properties().await {
                let device_info = serde_json::json!({
                    "address": props.address.to_string(),
                    "name": props.local_name.unwrap_or_else(|| "Unknown".to_string()),
                    "rssi": props.rssi,
                });
                devices.push(device_info);
            }
        }

        info!("Bluetooth client {} found {} devices", client_id, devices.len());
        let _ = status_tx.send(format!("[CLIENT] Found {} BLE devices", devices.len()));

        // Call LLM with scan results
        let protocol = Arc::new(BluetoothClientProtocol::new());
        let event = Event::new(
            &BLUETOOTH_SCAN_COMPLETE_EVENT,
            serde_json::json!({
                "devices": devices,
            }),
        );

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute actions
                    for action in actions {
                        if let Err(e) = Self::execute_llm_action(
                            action,
                            client_data,
                            app_state,
                            status_tx,
                            llm_client,
                            client_id,
                        ).await {
                            error!("Error executing Bluetooth action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for Bluetooth client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Connect to a specific BLE device
    async fn connect_to_device(
        client_data: &Arc<Mutex<ClientData>>,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        llm_client: &OllamaClient,
        client_id: ClientId,
        device_address: Option<String>,
        device_name: Option<String>,
    ) -> Result<()> {
        let adapter = {
            let data = client_data.lock().await;
            data.adapter.clone()
        };

        // Scan for devices if we need to find by name
        if device_name.is_some() {
            info!("Bluetooth client {} scanning for device by name", client_id);
            adapter.start_scan(ScanFilter::default()).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;
            adapter.stop_scan().await?;
        }

        let peripherals = adapter.peripherals().await?;

        // Find the target device
        let mut target_peripheral: Option<Peripheral> = None;
        for peripheral in peripherals {
            if let Ok(Some(props)) = peripheral.properties().await {
                let matches = if let Some(ref addr) = device_address {
                    props.address.to_string().eq_ignore_ascii_case(addr)
                } else if let Some(ref name) = device_name {
                    props.local_name.as_ref().map_or(false, |n| n.contains(name))
                } else {
                    false
                };

                if matches {
                    target_peripheral = Some(peripheral);
                    break;
                }
            }
        }

        let peripheral = target_peripheral
            .context("Device not found")?;

        // Connect to the device
        info!("Bluetooth client {} connecting to device", client_id);
        let _ = status_tx.send(format!("[CLIENT] Connecting to BLE device..."));

        peripheral.connect().await?;
        peripheral.discover_services().await?;

        let device_props = peripheral.properties().await?.context("No device properties")?;
        let device_addr = device_props.address.to_string();
        let device_name_str = device_props.local_name.unwrap_or_else(|| "Unknown".to_string());

        info!("Bluetooth client {} connected to {} ({})", client_id, device_name_str, device_addr);
        let _ = status_tx.send(format!("[CLIENT] Connected to {}", device_name_str));

        // Store peripheral
        {
            let mut data = client_data.lock().await;
            data.peripheral = Some(peripheral.clone());
        }

        // Set up notification handler using stream-based API
        let client_data_clone = client_data.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();
        let peripheral_clone = peripheral.clone();

        // Spawn task to handle notification stream
        tokio::spawn(async move {
            match peripheral_clone.notifications().await {
                Ok(mut notification_stream) => {
                    use futures::StreamExt;
                    while let Some(notification) = notification_stream.next().await {
                        let client_data = client_data_clone.clone();
                        let app_state = app_state_clone.clone();
                        let status_tx = status_tx_clone.clone();
                        let llm_client = llm_client_clone.clone();

                        trace!("Bluetooth notification received from {:?}", notification.uuid);

                        // Call LLM with notification
                        let protocol = Arc::new(BluetoothClientProtocol::new());
                        let event = Event::new(
                            &BLUETOOTH_NOTIFICATION_RECEIVED_EVENT,
                            serde_json::json!({
                                "service_uuid": "unknown", // btleplug doesn't provide service UUID in notification
                                "characteristic_uuid": notification.uuid.to_string(),
                                "value_hex": hex::encode(&notification.value),
                            }),
                        );

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
                            ).await {
                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                    // Update memory
                                    if let Some(mem) = memory_updates {
                                        client_data.lock().await.memory = mem;
                                    }

                                    // Execute actions
                                    for action in actions {
                                        if let Err(e) = Self::execute_llm_action(
                                            action,
                                            &client_data,
                                            &app_state,
                                            &status_tx,
                                            &llm_client,
                                            client_id,
                                        ).await {
                                            error!("Error executing Bluetooth action: {}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for Bluetooth notification: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to get notification stream: {}", e);
                }
            }
        });

        // Call LLM with connected event
        let protocol = Arc::new(BluetoothClientProtocol::new());
        let event = Event::new(
            &BLUETOOTH_CONNECTED_EVENT,
            serde_json::json!({
                "device_address": device_addr,
                "device_name": device_name_str,
            }),
        );

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute actions
                    for action in actions {
                        if let Err(e) = Self::execute_llm_action(
                            action,
                            client_data,
                            app_state,
                            status_tx,
                            llm_client,
                            client_id,
                        ).await {
                            error!("Error executing Bluetooth action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for Bluetooth client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Execute an action returned by the LLM
    async fn execute_llm_action(
        action: serde_json::Value,
        client_data: &Arc<Mutex<ClientData>>,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        llm_client: &OllamaClient,
        client_id: ClientId,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::Client;
        let protocol = BluetoothClientProtocol::new();

        match protocol.execute_action(action)? {
            crate::llm::actions::client_trait::ClientActionResult::Custom { name, data } => {
                match name.as_str() {
                    "scan_devices" => {
                        let duration_secs = data["duration_secs"].as_u64().unwrap_or(5);
                        Box::pin(Self::perform_scan(client_data, app_state, status_tx, llm_client, client_id, duration_secs)).await?;
                    }
                    "connect_device" => {
                        let device_address = data["device_address"].as_str().map(|s| s.to_string());
                        let device_name = data["device_name"].as_str().map(|s| s.to_string());
                        Self::connect_to_device(client_data, app_state, status_tx, llm_client, client_id, device_address, device_name).await?;
                    }
                    "discover_services" => {
                        Self::discover_services(client_data, app_state, status_tx, llm_client, client_id).await?;
                    }
                    "read_characteristic" => {
                        let service_uuid = Uuid::parse_str(data["service_uuid"].as_str().context("Missing service_uuid")?)?;
                        let char_uuid = Uuid::parse_str(data["characteristic_uuid"].as_str().context("Missing characteristic_uuid")?)?;
                        Self::read_characteristic(client_data, app_state, status_tx, llm_client, client_id, service_uuid, char_uuid).await?;
                    }
                    "write_characteristic" => {
                        let service_uuid = Uuid::parse_str(data["service_uuid"].as_str().context("Missing service_uuid")?)?;
                        let char_uuid = Uuid::parse_str(data["characteristic_uuid"].as_str().context("Missing characteristic_uuid")?)?;
                        let value_bytes = data["value_bytes"].as_array()
                            .context("Missing value_bytes")?
                            .iter()
                            .map(|v| v.as_u64().unwrap_or(0) as u8)
                            .collect::<Vec<u8>>();
                        let with_response = data["with_response"].as_bool().unwrap_or(true);
                        Self::write_characteristic(client_data, service_uuid, char_uuid, value_bytes, with_response).await?;
                    }
                    "subscribe_notifications" => {
                        let service_uuid = Uuid::parse_str(data["service_uuid"].as_str().context("Missing service_uuid")?)?;
                        let char_uuid = Uuid::parse_str(data["characteristic_uuid"].as_str().context("Missing characteristic_uuid")?)?;
                        Self::subscribe_notifications(client_data, service_uuid, char_uuid).await?;
                    }
                    "unsubscribe_notifications" => {
                        let service_uuid = Uuid::parse_str(data["service_uuid"].as_str().context("Missing service_uuid")?)?;
                        let char_uuid = Uuid::parse_str(data["characteristic_uuid"].as_str().context("Missing characteristic_uuid")?)?;
                        Self::unsubscribe_notifications(client_data, service_uuid, char_uuid).await?;
                    }
                    _ => {
                        warn!("Unknown custom action: {}", name);
                    }
                }
            }
            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                Self::disconnect(client_data, app_state, client_id).await?;
            }
            _ => {
                debug!("Unhandled action result type");
            }
        }

        Ok(())
    }

    /// Discover GATT services and characteristics
    async fn discover_services(
        client_data: &Arc<Mutex<ClientData>>,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        llm_client: &OllamaClient,
        client_id: ClientId,
    ) -> Result<()> {
        let peripheral = {
            let data = client_data.lock().await;
            data.peripheral.clone().context("Not connected to device")?
        };

        info!("Bluetooth client {} discovering services", client_id);

        let services = peripheral.services();
        let mut services_data = Vec::new();

        for service in services {
            let mut characteristics_data = Vec::new();

            for char in service.characteristics {
                let mut properties = Vec::new();
                if char.properties.contains(CharPropFlags::READ) {
                    properties.push("read".to_string());
                }
                if char.properties.contains(CharPropFlags::WRITE) {
                    properties.push("write".to_string());
                }
                if char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
                    properties.push("write_without_response".to_string());
                }
                if char.properties.contains(CharPropFlags::NOTIFY) {
                    properties.push("notify".to_string());
                }
                if char.properties.contains(CharPropFlags::INDICATE) {
                    properties.push("indicate".to_string());
                }

                characteristics_data.push(serde_json::json!({
                    "uuid": char.uuid.to_string(),
                    "properties": properties,
                }));
            }

            services_data.push(serde_json::json!({
                "uuid": service.uuid.to_string(),
                "primary": service.primary,
                "characteristics": characteristics_data,
            }));
        }

        info!("Bluetooth client {} discovered {} services", client_id, services_data.len());

        // Call LLM with services discovered event
        let protocol = Arc::new(BluetoothClientProtocol::new());
        let event = Event::new(
            &BLUETOOTH_SERVICES_DISCOVERED_EVENT,
            serde_json::json!({
                "services": services_data,
            }),
        );

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute actions
                    for action in actions {
                        if let Err(e) = Self::execute_llm_action(
                            action,
                            client_data,
                            app_state,
                            status_tx,
                            llm_client,
                            client_id,
                        ).await {
                            error!("Error executing Bluetooth action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for Bluetooth client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Read a characteristic value
    async fn read_characteristic(
        client_data: &Arc<Mutex<ClientData>>,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        llm_client: &OllamaClient,
        client_id: ClientId,
        service_uuid: Uuid,
        char_uuid: Uuid,
    ) -> Result<()> {
        let peripheral = {
            let data = client_data.lock().await;
            data.peripheral.clone().context("Not connected to device")?
        };

        debug!("Reading characteristic {} from service {}", char_uuid, service_uuid);

        // Find the characteristic
        let services = peripheral.services();
        let characteristic = services
            .iter()
            .find(|s| s.uuid == service_uuid)
            .and_then(|s| s.characteristics.iter().find(|c| c.uuid == char_uuid))
            .context("Characteristic not found")?;

        let value = peripheral.read(characteristic).await?;

        info!("Read {} bytes from characteristic {}", value.len(), char_uuid);

        // Call LLM with read data
        let protocol = Arc::new(BluetoothClientProtocol::new());
        let event = Event::new(
            &BLUETOOTH_DATA_READ_EVENT,
            serde_json::json!({
                "service_uuid": service_uuid.to_string(),
                "characteristic_uuid": char_uuid.to_string(),
                "value_hex": hex::encode(&value),
            }),
        );

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute actions
                    for action in actions {
                        if let Err(e) = Self::execute_llm_action(
                            action,
                            client_data,
                            app_state,
                            status_tx,
                            llm_client,
                            client_id,
                        ).await {
                            error!("Error executing Bluetooth action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for Bluetooth client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Write a characteristic value
    async fn write_characteristic(
        client_data: &Arc<Mutex<ClientData>>,
        service_uuid: Uuid,
        char_uuid: Uuid,
        value: Vec<u8>,
        with_response: bool,
    ) -> Result<()> {
        let peripheral = {
            let data = client_data.lock().await;
            data.peripheral.clone().context("Not connected to device")?
        };

        debug!("Writing {} bytes to characteristic {} from service {}", value.len(), char_uuid, service_uuid);

        // Find the characteristic
        let services = peripheral.services();
        let characteristic = services
            .iter()
            .find(|s| s.uuid == service_uuid)
            .and_then(|s| s.characteristics.iter().find(|c| c.uuid == char_uuid))
            .context("Characteristic not found")?;

        let write_type = if with_response {
            WriteType::WithResponse
        } else {
            WriteType::WithoutResponse
        };

        peripheral.write(characteristic, &value, write_type).await?;

        info!("Wrote {} bytes to characteristic {}", value.len(), char_uuid);

        Ok(())
    }

    /// Subscribe to notifications from a characteristic
    async fn subscribe_notifications(
        client_data: &Arc<Mutex<ClientData>>,
        service_uuid: Uuid,
        char_uuid: Uuid,
    ) -> Result<()> {
        let peripheral = {
            let data = client_data.lock().await;
            data.peripheral.clone().context("Not connected to device")?
        };

        debug!("Subscribing to characteristic {} from service {}", char_uuid, service_uuid);

        // Find the characteristic
        let services = peripheral.services();
        let characteristic = services
            .iter()
            .find(|s| s.uuid == service_uuid)
            .and_then(|s| s.characteristics.iter().find(|c| c.uuid == char_uuid))
            .context("Characteristic not found")?;

        peripheral.subscribe(characteristic).await?;

        info!("Subscribed to notifications from characteristic {}", char_uuid);

        Ok(())
    }

    /// Unsubscribe from notifications from a characteristic
    async fn unsubscribe_notifications(
        client_data: &Arc<Mutex<ClientData>>,
        service_uuid: Uuid,
        char_uuid: Uuid,
    ) -> Result<()> {
        let peripheral = {
            let data = client_data.lock().await;
            data.peripheral.clone().context("Not connected to device")?
        };

        debug!("Unsubscribing from characteristic {} from service {}", char_uuid, service_uuid);

        // Find the characteristic
        let services = peripheral.services();
        let characteristic = services
            .iter()
            .find(|s| s.uuid == service_uuid)
            .and_then(|s| s.characteristics.iter().find(|c| c.uuid == char_uuid))
            .context("Characteristic not found")?;

        peripheral.unsubscribe(characteristic).await?;

        info!("Unsubscribed from notifications from characteristic {}", char_uuid);

        Ok(())
    }

    /// Disconnect from the BLE device
    async fn disconnect(
        client_data: &Arc<Mutex<ClientData>>,
        app_state: &Arc<AppState>,
        client_id: ClientId,
    ) -> Result<()> {
        let peripheral = {
            let mut data = client_data.lock().await;
            data.peripheral.take()
        };

        if let Some(peripheral) = peripheral {
            peripheral.disconnect().await?;
            info!("Bluetooth client {} disconnected", client_id);
            app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
        }

        Ok(())
    }
}
