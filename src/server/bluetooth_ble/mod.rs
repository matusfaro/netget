//! Bluetooth Low Energy (BLE) GATT server implementation
//!
//! Cross-platform peripheral/server mode using ble-peripheral-rust
//! Platforms: Windows (WinRT), macOS (CoreBluetooth), Linux (BlueZ)

pub mod actions;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use actions::{BluetoothBleProtocol, BLUETOOTH_BLE_STARTED_EVENT, BLUETOOTH_STATE_CHANGED_EVENT,
    BLUETOOTH_READ_REQUEST_EVENT, BLUETOOTH_WRITE_REQUEST_EVENT, BLUETOOTH_SUBSCRIBE_EVENT};

#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::{Peripheral, PeripheralImpl};
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::peripheral_event::{PeripheralEvent, RequestResponse, ReadRequestResponse, WriteRequestResponse};
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::service::Service;
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::characteristic::Characteristic;
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::properties::{CharacteristicProperty, AttributePermission};
#[cfg(feature = "bluetooth-ble")]
use uuid::Uuid;

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-characteristic data for tracking pending requests
#[derive(Debug)]
struct CharacteristicData {
    uuid: String,
    properties: Vec<String>,
    permissions: Vec<String>,
    current_value: Vec<u8>,
}

/// Server data for BLE peripheral
struct ServerData {
    peripheral: Option<Peripheral>,
    state: ConnectionState,
    memory: String,
    characteristics: HashMap<String, CharacteristicData>,
    queued_events: Vec<PeripheralEvent>,
}

/// Bluetooth Low Energy GATT server
pub struct BluetoothBle;

impl BluetoothBle {
    /// Spawn the BLE GATT server with integrated LLM actions
    #[cfg(feature = "bluetooth-ble")]
    pub async fn spawn_with_llm_actions(
        device_name: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        instruction: String,
    ) -> Result<std::net::SocketAddr> {
        // Create event channel for peripheral events
        let (event_tx, event_rx) = mpsc::channel::<PeripheralEvent>(256);

        // Create BLE peripheral
        let mut peripheral = Peripheral::new(event_tx).await
            .context("Failed to create BLE peripheral")?;

        info!("Bluetooth server created, waiting for adapter to power on");
        let _ = status_tx.send(format!("[INFO] Bluetooth server created for device '{}'", device_name));

        // Wait for Bluetooth adapter to be powered on
        let mut retries = 0;
        while !peripheral.is_powered().await.unwrap_or(false) {
            if retries == 0 {
                warn!("Bluetooth adapter is not powered on, waiting...");
                let _ = status_tx.send("[WARN] Bluetooth adapter not powered on, waiting...".to_string());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            retries += 1;
            if retries > 20 {
                anyhow::bail!("Bluetooth adapter failed to power on after 10 seconds");
            }
        }

        info!("Bluetooth adapter powered on");
        let _ = status_tx.send("[INFO] Bluetooth adapter powered on".to_string());

        // Create server data
        let server_data = Arc::new(Mutex::new(ServerData {
            peripheral: Some(peripheral),
            state: ConnectionState::Idle,
            memory: String::new(),
            characteristics: HashMap::new(),
            queued_events: Vec::new(),
        }));

        let protocol = Arc::new(BluetoothBleProtocol::new());

        // Call LLM with server started event to get initial configuration
        let started_event = Event::new(
            &BLUETOOTH_BLE_STARTED_EVENT,
            serde_json::json!({
                "device_name": device_name,
                "instruction": instruction,
            })
        );

        info!("Calling LLM for initial Bluetooth server configuration");
        let llm_result = call_llm(
            &llm_client,
            &app_state,
            server_id,
            None, // No connection_id for server-level actions
            &started_event,
            protocol.as_ref(),
        ).await?;

        // Execute initial actions (add services, start advertising, etc.)
        for action in llm_result.raw_actions {
            debug!("Executing initial Bluetooth action: {:?}", action.get("type"));
            Self::execute_action(
                &server_data,
                &device_name,
                action,
                &status_tx,
            ).await?;
        }

        // Spawn event processing loop
        let llm_client_clone = llm_client.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let server_data_clone = server_data.clone();
        let protocol_clone = protocol.clone();

        tokio::spawn(async move {
            Self::event_loop(
                event_rx,
                server_id,
                llm_client_clone,
                app_state_clone,
                status_tx_clone,
                server_data_clone,
                protocol_clone,
            ).await;
        });

        // Return a dummy SocketAddr (BLE doesn't use IP addresses)
        // Use a unique "port" based on server_id for display purposes
        let dummy_addr: std::net::SocketAddr = format!("127.0.0.1:{}", 5900 + server_id.as_u32() % 100)
            .parse()
            .unwrap();
        Ok(dummy_addr)
    }

    /// Execute a single LLM action
    #[cfg(feature = "bluetooth-ble")]
    async fn execute_action(
        server_data: &Arc<Mutex<ServerData>>,
        device_name: &str,
        action: serde_json::Value,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "add_service" => {
                Self::execute_add_service(server_data, action, status_tx).await
            }
            "start_advertising" => {
                Self::execute_start_advertising(server_data, device_name, action, status_tx).await
            }
            "stop_advertising" => {
                Self::execute_stop_advertising(server_data, status_tx).await
            }
            "send_notification" => {
                Self::execute_send_notification(server_data, action, status_tx).await
            }
            "respond_to_read" | "send_read_response" => {
                // Read responses are handled inline in event loop
                Ok(())
            }
            "respond_to_write" | "send_write_response" => {
                // Write responses are handled inline in event loop
                Ok(())
            }
            _ => {
                warn!("Unknown Bluetooth action type: {}", action_type);
                Ok(())
            }
        }
    }

    /// Add a GATT service
    #[cfg(feature = "bluetooth-ble")]
    async fn execute_add_service(
        server_data: &Arc<Mutex<ServerData>>,
        action: serde_json::Value,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let uuid_str = action["uuid"]
            .as_str()
            .context("add_service requires 'uuid' field")?;
        let primary = action["primary"].as_bool().unwrap_or(true);

        let uuid = Uuid::parse_str(uuid_str)
            .context("Invalid service UUID")?;

        let chars_json = action["characteristics"]
            .as_array()
            .context("add_service requires 'characteristics' array")?;

        let mut characteristics = Vec::new();
        let mut server_data_guard = server_data.lock().await;

        for char_json in chars_json {
            let char_uuid_str = char_json["uuid"]
                .as_str()
                .context("characteristic requires 'uuid' field")?;
            let char_uuid = Uuid::parse_str(char_uuid_str)
                .context("Invalid characteristic UUID")?;

            // Parse properties
            let props_json = char_json["properties"]
                .as_array()
                .context("characteristic requires 'properties' array")?;
            let mut properties = Vec::new();
            for prop in props_json {
                let prop_str = prop.as_str().context("property must be string")?;
                properties.push(match prop_str.to_lowercase().as_str() {
                    "read" => CharacteristicProperty::Read,
                    "write" => CharacteristicProperty::Write,
                    "notify" => CharacteristicProperty::Notify,
                    "indicate" => CharacteristicProperty::Indicate,
                    "write_without_response" => CharacteristicProperty::WriteWithoutResponse,
                    _ => {
                        warn!("Unknown property: {}, defaulting to Read", prop_str);
                        CharacteristicProperty::Read
                    }
                });
            }

            // Parse permissions
            let empty_perms = Vec::new();
            let perms_json = char_json["permissions"]
                .as_array()
                .unwrap_or(&empty_perms);
            let mut permissions = Vec::new();
            for perm in perms_json {
                let perm_str = perm.as_str().context("permission must be string")?;
                permissions.push(match perm_str.to_lowercase().as_str() {
                    "readable" => AttributePermission::Readable,
                    "writeable" => AttributePermission::Writeable,
                    _ => {
                        warn!("Unknown permission: {}", perm_str);
                        continue;
                    }
                });
            }

            // Parse initial value (hex-encoded)
            let initial_value = if let Some(val_str) = char_json["initial_value"].as_str() {
                let val_str = val_str.trim_start_matches("0x");
                hex::decode(val_str).unwrap_or_default()
            } else {
                Vec::new()
            };

            // Store characteristic data for tracking
            server_data_guard.characteristics.insert(
                char_uuid_str.to_string(),
                CharacteristicData {
                    uuid: char_uuid_str.to_string(),
                    properties: props_json.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                    permissions: perms_json.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                    current_value: initial_value.clone(),
                },
            );

            characteristics.push(Characteristic {
                uuid: char_uuid,
                properties,
                permissions,
                value: if initial_value.is_empty() { None } else { Some(initial_value) },
                descriptors: Vec::new(), // TODO: support descriptors if needed
            });
        }

        let service = Service {
            uuid,
            primary,
            characteristics,
        };

        if let Some(ref mut peripheral) = server_data_guard.peripheral {
            peripheral.add_service(&service).await
                .context("Failed to add service to peripheral")?;

            info!("Added BLE service {} with {} characteristics", uuid_str, chars_json.len());
            let _ = status_tx.send(format!(
                "[INFO] Added BLE service {} with {} characteristics",
                uuid_str,
                chars_json.len()
            ));
        }

        Ok(())
    }

    /// Start BLE advertising
    #[cfg(feature = "bluetooth-ble")]
    async fn execute_start_advertising(
        server_data: &Arc<Mutex<ServerData>>,
        device_name: &str,
        action: serde_json::Value,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let name = action["device_name"]
            .as_str()
            .unwrap_or(device_name);

        // Parse service UUIDs if provided
        let service_uuids: Vec<Uuid> = action["service_uuids"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect()
            })
            .unwrap_or_else(Vec::new);

        let mut server_data_guard = server_data.lock().await;
        if let Some(ref mut peripheral) = server_data_guard.peripheral {
            peripheral.start_advertising(name, &service_uuids).await
                .context("Failed to start advertising")?;

            info!("Started BLE advertising as '{}' with {} service(s)", name, service_uuids.len());
            let _ = status_tx.send(format!("[INFO] Started BLE advertising as '{}' with {} service(s)", name, service_uuids.len()));
        }

        Ok(())
    }

    /// Stop BLE advertising
    #[cfg(feature = "bluetooth-ble")]
    async fn execute_stop_advertising(
        server_data: &Arc<Mutex<ServerData>>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut server_data_guard = server_data.lock().await;
        if let Some(ref mut peripheral) = server_data_guard.peripheral {
            peripheral.stop_advertising().await
                .context("Failed to stop advertising")?;

            info!("Stopped BLE advertising");
            let _ = status_tx.send("[INFO] Stopped BLE advertising".to_string());
        }

        Ok(())
    }

    /// Send notification to subscribed clients
    #[cfg(feature = "bluetooth-ble")]
    async fn execute_send_notification(
        server_data: &Arc<Mutex<ServerData>>,
        action: serde_json::Value,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let char_uuid_str = action["characteristic_uuid"]
            .as_str()
            .context("send_notification requires 'characteristic_uuid' field")?;
        let value_str = action["value"]
            .as_str()
            .context("send_notification requires 'value' field (hex-encoded)")?;

        let char_uuid = Uuid::parse_str(char_uuid_str)
            .context("Invalid characteristic UUID")?;
        let value_str = value_str.trim_start_matches("0x");
        let value = hex::decode(value_str)
            .context("Value must be hex-encoded")?;

        let mut server_data_guard = server_data.lock().await;

        // Update stored value
        if let Some(char_data) = server_data_guard.characteristics.get_mut(char_uuid_str) {
            char_data.current_value = value.clone();
        }

        if let Some(ref mut peripheral) = server_data_guard.peripheral {
            peripheral.update_characteristic(char_uuid, value.clone()).await
                .context("Failed to send notification")?;

            debug!("Sent notification on {} with {} bytes", char_uuid_str, value.len());
            let _ = status_tx.send(format!(
                "[DEBUG] Sent BLE notification on {} ({} bytes)",
                char_uuid_str,
                value.len()
            ));
        }

        Ok(())
    }

    /// Main event processing loop
    #[cfg(feature = "bluetooth-ble")]
    async fn event_loop(
        mut event_rx: mpsc::Receiver<PeripheralEvent>,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_data: Arc<Mutex<ServerData>>,
        protocol: Arc<BluetoothBleProtocol>,
    ) {
        while let Some(event) = event_rx.recv().await {
            match event {
                PeripheralEvent::StateUpdate { is_powered, .. } => {
                    info!("Bluetooth state update: powered = {}", is_powered);
                    let _ = status_tx.send(format!("[INFO] Bluetooth state: powered = {}", is_powered));

                    // Create event for LLM
                    let llm_event = Event::new(
                        &BLUETOOTH_STATE_CHANGED_EVENT,
                        serde_json::json!({
                            "state": if is_powered { "powered_on" } else { "powered_off" },
                        })
                    );

                    // Call LLM with state change
                    let _ = Self::call_llm_for_event(
                        &server_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &server_data,
                        &protocol,
                        llm_event,
                    ).await;
                }
                PeripheralEvent::ReadRequest { request, offset, responder } => {
                    let char_uuid_str = request.characteristic.to_string();

                    debug!("BLE read request on characteristic {} at offset {}", char_uuid_str, offset);
                    let _ = status_tx.send(format!(
                        "[DEBUG] BLE read request on {} (offset: {})",
                        char_uuid_str,
                        offset
                    ));

                    // Check current state
                    let current_state = {
                        let guard = server_data.lock().await;
                        guard.state.clone()
                    };

                    match current_state {
                        ConnectionState::Idle => {
                            // Update state to Processing
                            server_data.lock().await.state = ConnectionState::Processing;

                            // Create read request event
                            let llm_event = Event::new(
                                &BLUETOOTH_READ_REQUEST_EVENT,
                                serde_json::json!({
                                    "characteristic_uuid": char_uuid_str,
                                    "offset": offset,
                                })
                            );

                            // Call LLM
                            match Self::call_llm_for_event(
                                &server_id,
                                &llm_client,
                                &app_state,
                                &status_tx,
                                &server_data,
                                &protocol,
                                llm_event,
                            ).await {
                                Ok(llm_result) => {
                                    // Look for read response in actions
                                    let value = llm_result.raw_actions.iter()
                                        .find(|a| {
                                            a.get("type").and_then(|v| v.as_str()) == Some("respond_to_read") ||
                                            a.get("type").and_then(|v| v.as_str()) == Some("send_read_response")
                                        })
                                        .and_then(|a| a.get("value"))
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| {
                                            let s = s.trim_start_matches("0x");
                                            hex::decode(s).ok()
                                        })
                                        .unwrap_or_else(|| {
                                            // Default: return current stored value
                                            let guard = futures::executor::block_on(server_data.lock());
                                            guard.characteristics.get(&char_uuid_str)
                                                .map(|c| c.current_value.clone())
                                                .unwrap_or_default()
                                        });

                                    let _ = responder.send(ReadRequestResponse {
                                        value,
                                        response: RequestResponse::Success,
                                    });
                                }
                                Err(e) => {
                                    error!("LLM call failed for read request: {}", e);
                                    let _ = responder.send(ReadRequestResponse {
                                        value: Vec::new(),
                                        response: RequestResponse::UnlikelyError,
                                    });
                                }
                            }

                            // Back to Idle
                            server_data.lock().await.state = ConnectionState::Idle;
                        }
                        ConnectionState::Processing => {
                            // Queue the event
                            server_data.lock().await.queued_events.push(
                                PeripheralEvent::ReadRequest { request, offset, responder }
                            );
                        }
                        ConnectionState::Accumulating => {
                            // Also queue
                            server_data.lock().await.queued_events.push(
                                PeripheralEvent::ReadRequest { request, offset, responder }
                            );
                        }
                    }
                }
                PeripheralEvent::WriteRequest { request, value, offset, responder } => {
                    let char_uuid_str = request.characteristic.to_string();
                    let value_hex = hex::encode(&value);

                    debug!("BLE write request on characteristic {} with {} bytes at offset {}",
                        char_uuid_str, value.len(), offset);
                    let _ = status_tx.send(format!(
                        "[DEBUG] BLE write request on {} ({} bytes)",
                        char_uuid_str,
                        value.len()
                    ));
                    trace!("BLE write data (hex): {}", value_hex);
                    let _ = status_tx.send(format!("[TRACE] BLE write data (hex): {}", value_hex));

                    // Check current state
                    let current_state = {
                        let guard = server_data.lock().await;
                        guard.state.clone()
                    };

                    match current_state {
                        ConnectionState::Idle => {
                            // Update state to Processing
                            server_data.lock().await.state = ConnectionState::Processing;

                            // Update stored value
                            {
                                let mut guard = server_data.lock().await;
                                if let Some(char_data) = guard.characteristics.get_mut(&char_uuid_str) {
                                    char_data.current_value = value.clone();
                                }
                            }

                            // Create write request event
                            let llm_event = Event::new(
                                &BLUETOOTH_WRITE_REQUEST_EVENT,
                                serde_json::json!({
                                    "characteristic_uuid": char_uuid_str,
                                    "value": value_hex,
                                    "offset": offset,
                                })
                            );

                            // Call LLM
                            match Self::call_llm_for_event(
                                &server_id,
                                &llm_client,
                                &app_state,
                                &status_tx,
                                &server_data,
                                &protocol,
                                llm_event,
                            ).await {
                                Ok(_) => {
                                    let _ = responder.send(WriteRequestResponse {
                                        response: RequestResponse::Success,
                                    });
                                }
                                Err(e) => {
                                    error!("LLM call failed for write request: {}", e);
                                    let _ = responder.send(WriteRequestResponse {
                                        response: RequestResponse::UnlikelyError,
                                    });
                                }
                            }

                            // Back to Idle
                            server_data.lock().await.state = ConnectionState::Idle;
                        }
                        ConnectionState::Processing => {
                            // Queue the event
                            server_data.lock().await.queued_events.push(
                                PeripheralEvent::WriteRequest { request, value, offset, responder }
                            );
                        }
                        ConnectionState::Accumulating => {
                            // Also queue
                            server_data.lock().await.queued_events.push(
                                PeripheralEvent::WriteRequest { request, value, offset, responder }
                            );
                        }
                    }
                }
                PeripheralEvent::CharacteristicSubscriptionUpdate { request, subscribed } => {
                    let char_uuid_str = request.characteristic.to_string();
                    if subscribed {
                        info!("Client subscribed to notifications on {}", char_uuid_str);
                        let _ = status_tx.send(format!("[INFO] Client subscribed to notifications on {}", char_uuid_str));
                    } else {
                        info!("Client unsubscribed from notifications on {}", char_uuid_str);
                        let _ = status_tx.send(format!("[INFO] Client unsubscribed from notifications on {}", char_uuid_str));
                    }

                    let llm_event = Event::new(
                        &BLUETOOTH_SUBSCRIBE_EVENT,
                        serde_json::json!({
                            "characteristic_uuid": char_uuid_str,
                            "subscribed": subscribed,
                        })
                    );

                    let _ = Self::call_llm_for_event(
                        &server_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &server_data,
                        &protocol,
                        llm_event,
                    ).await;
                }
            }
        }
    }

    /// Call LLM with an event and execute resulting actions
    #[cfg(feature = "bluetooth-ble")]
    async fn call_llm_for_event(
        server_id: &crate::state::ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        server_data: &Arc<Mutex<ServerData>>,
        protocol: &Arc<BluetoothBleProtocol>,
        event: Event,
    ) -> Result<crate::llm::actions::executor::ExecutionResult> {
        let _memory = server_data.lock().await.memory.clone();

        let llm_result = call_llm(
            llm_client,
            app_state,
            *server_id,
            None, // No connection_id for server-level events
            &event,
            protocol.as_ref(),
        ).await?;

        // Execute returned actions (except read/write responses which are handled inline)
        for action in &llm_result.raw_actions {
            let action_type = action.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match action_type {
                "respond_to_read" | "send_read_response" | "respond_to_write" | "send_write_response" => {
                    // These are handled inline in the event match arms
                    continue;
                }
                _ => {
                    if let Err(e) = Self::execute_action(
                        server_data,
                        "NetGet-BLE",
                        action.clone(),
                        status_tx,
                    ).await {
                        error!("Failed to execute action: {}", e);
                    }
                }
            }
        }

        Ok(llm_result)
    }
}

#[cfg(not(feature = "bluetooth-ble"))]
impl BluetoothBle {
    pub async fn spawn_with_llm_actions(
        _device_name: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _instruction: String,
    ) -> Result<std::net::SocketAddr> {
        anyhow::bail!("Bluetooth server support not enabled - compile with --features bluetooth-ble")
    }
}
