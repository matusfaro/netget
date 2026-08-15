//! Bluetooth Low Energy (BLE) GATT server implementation
//!
//! Cross-platform peripheral/server mode using ble-peripheral-rust
//! Platforms: Windows (WinRT), macOS (CoreBluetooth), Linux (BlueZ)

pub mod actions;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_error, console_info, console_trace};
use actions::{
    BluetoothBleProtocol, BLUETOOTH_BLE_STARTED_EVENT, BLUETOOTH_READ_REQUEST_EVENT,
    BLUETOOTH_STATE_CHANGED_EVENT, BLUETOOTH_SUBSCRIBE_EVENT, BLUETOOTH_WRITE_REQUEST_EVENT,
};

#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::characteristic::Characteristic;
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::peripheral_event::{
    PeripheralEvent, ReadRequestResponse, RequestResponse, WriteRequestResponse,
};
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::properties::{AttributePermission, CharacteristicProperty};
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::gatt::service::Service;
#[cfg(feature = "bluetooth-ble")]
use ble_peripheral_rust::{Peripheral, PeripheralImpl};
#[cfg(feature = "bluetooth-ble")]
use uuid::Uuid;

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-characteristic data for tracking pending requests
#[derive(Debug)]
struct CharacteristicData {
    #[allow(dead_code)]
    uuid: String,
    #[allow(dead_code)]
    properties: Vec<String>,
    #[allow(dead_code)]
    permissions: Vec<String>,
    current_value: Vec<u8>,
}

/// Server data for BLE peripheral
struct ServerData {
    /// The process-wide shared radio, or `None` in the radio-free test loop.
    hub: Option<Arc<BleHub>>,
    /// This server's own slice of the shared event stream, used to register characteristic
    /// routes with the hub as services are added. `None` in the radio-free test loop.
    event_tx: Option<mpsc::Sender<PeripheralEvent>>,
    state: ConnectionState,
    memory: String,
    characteristics: HashMap<String, CharacteristicData>,
    queued_events: Vec<PeripheralEvent>,
}

/// Parse a BLE UUID, expanding the 16- and 32-bit shorthands.
///
/// Bluetooth assigns short UUIDs (`180D` for Heart Rate, `2A37` for its
/// measurement characteristic) that stand for the full 128-bit value
/// `0000XXXX-0000-1000-8000-00805F9B34FB`. Every example in this protocol's
/// actions and CLAUDE.md uses that shorthand, and the docs stated it was
/// "expanded to" the long form — but nothing expanded it, and
/// `Uuid::parse_str("180D")` fails, so a model copying the protocol's own
/// documented example got `Invalid service UUID`.
///
/// Accepts a 4-hex-digit (16-bit) or 8-hex-digit (32-bit) shorthand, or any
/// form `Uuid::parse_str` already understands.
///
/// `pub` so `tests/` can exercise it directly — CLAUDE.md forbids unit-test
/// modules in `src/`, so an internal helper has to be reachable to be tested.
#[cfg(feature = "bluetooth-ble")]
pub fn parse_ble_uuid(s: &str) -> Result<Uuid> {
    let t = s.trim();
    let is_hex = |v: &str| v.chars().all(|c| c.is_ascii_hexdigit());

    if (t.len() == 4 || t.len() == 8) && is_hex(t) {
        // Left-pad the 16-bit form to 32 bits, then splice into the BLE base UUID.
        let short = format!("{:0>8}", t.to_ascii_lowercase());
        let full = format!("{short}-0000-1000-8000-00805f9b34fb");
        return Uuid::parse_str(&full)
            .with_context(|| format!("Invalid BLE short UUID {t:?} (expanded to {full})"));
    }

    Uuid::parse_str(t).with_context(|| {
        format!(
            "Invalid UUID {t:?}. Use a 16-bit shorthand like \"180D\", a 32-bit one, \
             or a full 128-bit UUID."
        )
    })
}

/// Process-wide shared BLE radio.
///
/// `ble-peripheral-rust`'s CoreBluetooth backend funnels *every* `Peripheral` through a single
/// process-global manager thread guarded by its own `static PERIPHERAL_THREAD: OnceCell<()>`
/// (`peripheral_manager.rs:50`). Only the **first** `Peripheral::new()` in a process actually
/// spawns that thread and wires up its command channel; every later `Peripheral::new()` builds a
/// fresh `manager_tx` whose receiver is dropped on the spot, so all of its commands — including
/// `is_powered()` — fail silently. The second BLE server start therefore saw `is_powered()`
/// return `false` forever and bailed after the 20×500ms wait with a bogus "adapter failed to
/// power on after 10 seconds", while the adapter was fine (IMPROVEMENTS item 2).
///
/// CoreBluetooth is genuinely one-manager-per-process anyway (one GATT database, one advertising
/// state, one radio), so the correct model is a single shared `Peripheral` reused across every
/// BLE server start. That is what `BleHub` is. It is created exactly once, on the first start,
/// via [`BLE_HUB`]; the adapter power-on wait happens once there, so subsequent starts reuse the
/// live radio with no wait and no hang.
///
/// The single shared radio also means a single shared event stream: `Peripheral::new` takes one
/// `PeripheralEvent` sender for the whole process. The hub owns that stream and a dispatcher task
/// hands each event to its [`BleRouter`], which fans it out to the owning server by characteristic
/// UUID, falling back to the most-recently-started live server, and broadcasts adapter
/// `StateUpdate`s to all of them.
#[cfg(feature = "bluetooth-ble")]
struct BleHub {
    /// The one real radio. Its mutating methods take `&mut self`, so calls are serialised here.
    peripheral: Mutex<Peripheral>,
    /// Where the single event stream is fanned out to per-server channels.
    router: BleRouter,
}

/// Routes one shared radio's events to the right per-server channel.
///
/// Split out of [`BleHub`] so it is exercisable without a Bluetooth adapter: it holds no
/// `Peripheral`, only channels, so a test can register stub servers, dispatch events, and assert
/// which server received what. It is `pub` for the same reason `run_event_loop_without_radio` and
/// `parse_ble_uuid` are — the project forbids `#[cfg(test)]` modules in `src/`.
#[cfg(feature = "bluetooth-ble")]
#[derive(Default)]
pub struct BleRouter {
    /// characteristic UUID (lowercased) -> the event channel of the server that added it.
    routes: std::sync::Mutex<HashMap<String, mpsc::Sender<PeripheralEvent>>>,
    /// Every started server's event channel, for events with no characteristic (StateUpdate)
    /// and as the routing fallback.
    servers: std::sync::Mutex<Vec<mpsc::Sender<PeripheralEvent>>>,
}

#[cfg(feature = "bluetooth-ble")]
static BLE_HUB: tokio::sync::OnceCell<Arc<BleHub>> = tokio::sync::OnceCell::const_new();

#[cfg(feature = "bluetooth-ble")]
impl BleRouter {
    /// A router with no registered servers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a freshly started server so adapter-level events and unrouted requests can reach
    /// it.
    pub fn register_server(&self, tx: mpsc::Sender<PeripheralEvent>) {
        self.servers.lock().unwrap().push(tx);
    }

    /// Point a characteristic's future read/write/subscribe events at the server that owns it.
    pub fn register_characteristic(&self, char_uuid: &str, tx: mpsc::Sender<PeripheralEvent>) {
        self.routes
            .lock()
            .unwrap()
            .insert(char_uuid.to_lowercase(), tx);
    }

    /// Choose where a characteristic event goes: its registered owner, else the newest live
    /// server. Closed channels (stopped servers) are skipped so a dead server never captures
    /// traffic.
    fn route_target(&self, char_uuid_lc: &str) -> Option<mpsc::Sender<PeripheralEvent>> {
        if let Some(tx) = self.routes.lock().unwrap().get(char_uuid_lc) {
            if !tx.is_closed() {
                return Some(tx.clone());
            }
        }
        self.servers
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|tx| !tx.is_closed())
            .cloned()
    }

    /// Every still-open server channel.
    fn live_servers(&self) -> Vec<mpsc::Sender<PeripheralEvent>> {
        self.servers
            .lock()
            .unwrap()
            .iter()
            .filter(|tx| !tx.is_closed())
            .cloned()
            .collect()
    }

    /// Fan one radio event out to the owning server(s). Never holds a std lock across an await —
    /// senders are cloned out first.
    pub async fn dispatch(&self, event: PeripheralEvent) {
        match event {
            // No characteristic: an adapter state change concerns every server on the radio.
            // `PeripheralEvent` is not `Clone` (read/write carry a `oneshot` responder), but
            // `StateUpdate` is trivially rebuildable, so it can be fanned out.
            PeripheralEvent::StateUpdate { is_powered } => {
                for tx in self.live_servers() {
                    let _ = tx.send(PeripheralEvent::StateUpdate { is_powered }).await;
                }
            }
            // Everything else names a characteristic and carries at most one responder, so it
            // goes to exactly one server.
            ev => {
                let char_uuid_lc = match &ev {
                    PeripheralEvent::ReadRequest { request, .. }
                    | PeripheralEvent::WriteRequest { request, .. }
                    | PeripheralEvent::CharacteristicSubscriptionUpdate { request, .. } => {
                        request.characteristic.to_string().to_lowercase()
                    }
                    PeripheralEvent::StateUpdate { .. } => unreachable!(),
                };
                match self.route_target(&char_uuid_lc) {
                    Some(tx) => {
                        let _ = tx.send(ev).await;
                    }
                    None => {
                        // No live server owns this characteristic. A read/write responder is
                        // dropped here, which the central surfaces as a failed operation — the
                        // honest outcome when nothing is behind the GATT entry.
                        warn!(
                            "BLE event for characteristic {} has no live server to handle it; \
                             dropping",
                            char_uuid_lc
                        );
                    }
                }
            }
        }
    }
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
        // Acquire the process-wide shared radio. It is created (and the adapter powered on)
        // exactly once, on the first BLE server start; every later start reuses it with no
        // wait. Creating a fresh `Peripheral` per start is what IMPROVEMENTS item 2 describes:
        // the crate's global manager singleton leaves the second peripheral's command channel
        // dead, so `is_powered()` never returns true and the start falsely times out. See
        // [`BleHub`].
        let hub = Self::shared_hub(&status_tx)
            .await
            .context("Failed to bring up the shared BLE radio")?;

        info!("Bluetooth server created on shared radio, adapter powered on");
        Log::new(Some(&status_tx)).info(format!(
            "Bluetooth server created for device '{}'",
            device_name
        ));

        // This server's slice of the shared event stream. The hub's dispatcher forwards this
        // server's characteristic events here; adapter state changes are broadcast here too.
        let (event_tx, event_rx) = mpsc::channel::<PeripheralEvent>(256);
        hub.router.register_server(event_tx.clone());

        // Create server data
        let server_data = Arc::new(Mutex::new(ServerData {
            hub: Some(hub.clone()),
            event_tx: Some(event_tx.clone()),
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
            }),
        );

        info!("Calling LLM for initial Bluetooth server configuration");

        // Bringing the adapter up must not require the model to be reachable: the LLM answers
        // traffic, it does not open the radio. This call used to propagate with `?`, so an
        // Ollama outage made `spawn()` return `Err` and the server never started.
        //
        // Unlike NFC and the USB smart card reader there is no useful default here — the
        // configuration *is* the services and the advertisement — so a failure leaves a
        // powered adapter advertising nothing. That is a much better outcome than a server
        // that will not start, but it is not a working one, so it is logged at ERROR on both
        // channels saying exactly that. The individual actions are non-fatal for the same
        // reason `executor::execute_actions` does not abort a batch on one bad action:
        // dropping the rest would suppress the services that were fine.
        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            None, // No connection_id for server-level actions
            &started_event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(llm_result) => {
                // Execute initial actions (add services, start advertising, etc.)
                for action in llm_result.raw_actions {
                    debug!(
                        "Executing initial Bluetooth action: {:?}",
                        action.get("type")
                    );
                    if let Err(e) =
                        Self::execute_action(&server_data, &device_name, action, &status_tx).await
                    {
                        Log::new(Some(&status_tx))
                            .error(format!("Initial Bluetooth action failed: {e}"));
                    }
                }
            }
            Err(e) => {
                error!(
                    "Bluetooth startup configuration failed ({}); the adapter for '{}' is \
                     powered but has no services and is NOT advertising",
                    e, device_name
                );
                Log::new(Some(&status_tx)).error(format!(
                    "Bluetooth startup configuration failed: {e}. The adapter is up but \
                     no services were added and it is not advertising."
                ));
            }
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
            )
            .await;
        });

        // BLE speaks to a radio, not a socket, so there is no endpoint to report.
        //
        // This used to return `127.0.0.1:{5900 + server_id % 100}` "for display purposes".
        // `server_startup::is_bound_addr` only rejects port 0, so that address was recorded as
        // `local_addr` and the TUI, `server_status` and every log reader were told the BLE
        // server was listening on a loopback port that nothing had bound — and on 5900, which
        // is VNC's port, so it could also collide with a real server in the display. Port 0 is
        // this codebase's "binds no listening socket" placeholder and is recognised as such.
        Ok(std::net::SocketAddr::from((
            std::net::Ipv4Addr::UNSPECIFIED,
            0,
        )))
    }

    /// Get (creating once) the process-wide shared BLE radio.
    ///
    /// The first call builds the single `Peripheral`, waits for the adapter to power on, and
    /// spawns the dispatcher task that fans radio events out to per-server channels. Every later
    /// call returns the same `Arc<BleHub>` immediately. On failure the `OnceCell` stays empty so
    /// a subsequent start retries rather than caching the failure.
    #[cfg(feature = "bluetooth-ble")]
    async fn shared_hub(status_tx: &mpsc::UnboundedSender<String>) -> Result<Arc<BleHub>> {
        let status_tx = status_tx.clone();
        BLE_HUB
            .get_or_try_init(move || async move {
                info!("Creating shared BLE peripheral (first BLE server in this process)");

                // The single process-wide event stream. `Peripheral::new` bakes this sender into
                // the crate's global manager; the hub dispatcher owns the receiver.
                let (event_tx, mut event_rx) = mpsc::channel::<PeripheralEvent>(256);
                let mut peripheral = Peripheral::new(event_tx)
                    .await
                    .context("Failed to create BLE peripheral")?;

                // Wait for the adapter to power on — once, here, not per server start.
                let mut retries = 0;
                while !peripheral.is_powered().await.unwrap_or(false) {
                    if retries == 0 {
                        Log::new(Some(&status_tx))
                            .warn("Bluetooth adapter is not powered on, waiting...");
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    retries += 1;
                    if retries > 20 {
                        anyhow::bail!("Bluetooth adapter failed to power on after 10 seconds");
                    }
                }

                info!("Bluetooth adapter powered on (shared radio)");
                Log::new(Some(&status_tx)).info("Bluetooth adapter powered on");

                let hub = Arc::new(BleHub {
                    peripheral: Mutex::new(peripheral),
                    router: BleRouter::new(),
                });

                // Single dispatcher for the whole process: route each radio event to the server
                // that owns the characteristic (or broadcast, for adapter state).
                let dispatch_hub = hub.clone();
                tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        dispatch_hub.router.dispatch(event).await;
                    }
                    warn!("BLE shared radio event stream ended; no further events will be routed");
                });

                Ok::<Arc<BleHub>, anyhow::Error>(hub)
            })
            .await
            .cloned()
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
            "add_service" => Self::execute_add_service(server_data, action, status_tx).await,
            "start_advertising" => {
                Self::execute_start_advertising(server_data, device_name, action, status_tx).await
            }
            "stop_advertising" => Self::execute_stop_advertising(server_data, status_tx).await,
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

        let uuid = parse_ble_uuid(uuid_str).context("Invalid service UUID")?;

        let chars_json = action["characteristics"]
            .as_array()
            .context("add_service requires 'characteristics' array")?;

        let mut characteristics = Vec::new();
        let mut char_uuids: Vec<String> = Vec::new();
        let mut server_data_guard = server_data.lock().await;

        for char_json in chars_json {
            let char_uuid_str = char_json["uuid"]
                .as_str()
                .context("characteristic requires 'uuid' field")?;
            let char_uuid = parse_ble_uuid(char_uuid_str).context("Invalid characteristic UUID")?;

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
            let perms_json = char_json["permissions"].as_array().unwrap_or(&empty_perms);
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
                    properties: props_json
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                    permissions: perms_json
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                    current_value: initial_value.clone(),
                },
            );

            // `mut` is only used by the Apple-specific guard below.
            #[allow(unused_mut)]
            let mut cached_value = if initial_value.is_empty() {
                None
            } else {
                Some(initial_value)
            };

            // CoreBluetooth (macOS/iOS) raises NSInvalidArgumentException from
            // -[CBMutableCharacteristic initWithType:properties:value:permissions:] when a
            // cached value is combined with anything other than read-only properties and
            // permissions. That Objective-C exception crosses the FFI boundary and aborts the
            // whole process ("fatal runtime error: Rust cannot catch foreign exceptions"), so it
            // cannot be caught or reported - it must be avoided. ble-peripheral-rust documents
            // the same constraint in its CoreBluetooth backend (peripheral_manager.rs:205).
            //
            // Dropping the cached value costs nothing: reads are served through the
            // bluetooth_read_request -> respond_to_read event path, never from this cache.
            #[cfg(target_vendor = "apple")]
            if cached_value.is_some() {
                let read_only = properties
                    .iter()
                    .all(|p| matches!(p, CharacteristicProperty::Read))
                    && permissions
                        .iter()
                        .all(|p| matches!(p, AttributePermission::Readable));
                if !read_only {
                    warn!(
                        "CoreBluetooth: ignoring initial_value on characteristic {} - a cached \
                         value is only legal on a read-only characteristic. Reads are answered \
                         via bluetooth_read_request instead.",
                        char_uuid_str
                    );
                    Log::new(Some(&status_tx)).warn(format!(
                        "BLE {}: initial_value ignored (macOS allows a cached value only \
                         on read-only characteristics); reads are answered by the LLM",
                        char_uuid_str
                    ));
                    cached_value = None;
                }
            }

            characteristics.push(Characteristic {
                uuid: char_uuid,
                properties,
                permissions,
                value: cached_value,
                descriptors: Vec::new(), // TODO: support descriptors if needed
            });
            char_uuids.push(char_uuid_str.to_string());
        }

        // Capture the shared radio and this server's event channel, then drop the ServerData
        // guard: the radio call below awaits I/O and must not be made holding that lock (and
        // never with the two locks nested, to keep a consistent order).
        let hub = server_data_guard.hub.clone();
        let event_tx = server_data_guard.event_tx.clone();
        drop(server_data_guard);

        // Point this characteristic's future read/write/subscribe events at this server.
        if let (Some(hub), Some(tx)) = (hub.as_ref(), event_tx.as_ref()) {
            for cu in &char_uuids {
                hub.router.register_characteristic(cu, tx.clone());
            }
        }

        let service = Service {
            uuid,
            primary,
            characteristics,
        };

        if let Some(hub) = hub.as_ref() {
            hub.peripheral
                .lock()
                .await
                .add_service(&service)
                .await
                .context("Failed to add service to peripheral")?;

            Log::new(Some(&status_tx)).info(format!(
                "Added BLE service {} with {} characteristics",
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
        let name = action["device_name"].as_str().unwrap_or(device_name);

        // Parse service UUIDs if provided
        let service_uuids: Vec<Uuid> = action["service_uuids"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| parse_ble_uuid(s).ok())
                    .collect()
            })
            .unwrap_or_else(Vec::new);

        let hub = server_data.lock().await.hub.clone();
        if let Some(hub) = hub.as_ref() {
            hub.peripheral
                .lock()
                .await
                .start_advertising(name, &service_uuids)
                .await
                .context("Failed to start advertising")?;

            console_info!(
                status_tx,
                "Started BLE advertising as '{}' with {} service(s)",
                name,
                service_uuids.len()
            );
        }

        Ok(())
    }

    /// Stop BLE advertising
    #[cfg(feature = "bluetooth-ble")]
    async fn execute_stop_advertising(
        server_data: &Arc<Mutex<ServerData>>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let hub = server_data.lock().await.hub.clone();
        if let Some(hub) = hub.as_ref() {
            hub.peripheral
                .lock()
                .await
                .stop_advertising()
                .await
                .context("Failed to stop advertising")?;

            Log::new(Some(&status_tx)).info("Stopped BLE advertising");
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

        let char_uuid = parse_ble_uuid(char_uuid_str).context("Invalid characteristic UUID")?;
        let value_str = value_str.trim_start_matches("0x");
        let value = hex::decode(value_str).context("Value must be hex-encoded")?;

        // Update stored value and capture the shared radio, then drop the guard before I/O.
        let hub = {
            let mut server_data_guard = server_data.lock().await;
            if let Some(char_data) = server_data_guard.characteristics.get_mut(char_uuid_str) {
                char_data.current_value = value.clone();
            }
            server_data_guard.hub.clone()
        };

        if let Some(hub) = hub.as_ref() {
            hub.peripheral
                .lock()
                .await
                .update_characteristic(char_uuid, value.clone())
                .await
                .context("Failed to send notification")?;

            Log::new(Some(&status_tx)).debug(format!(
                "Sent BLE notification on {} ({} bytes)",
                char_uuid_str,
                value.len()
            ));
        }

        Ok(())
    }

    /// Run the GATT event loop over an externally supplied event stream, with no radio.
    ///
    /// This is the same [`Self::event_loop`] `spawn_with_llm_actions` runs; the only difference
    /// is that the `ServerData` it builds holds no `Peripheral`, so the actions that would
    /// transmit are no-ops while the request/response paths are byte-for-byte the ones a real
    /// central drives.
    ///
    /// It exists so `tests/` can exercise the ATT error paths. The project forbids
    /// `#[cfg(test)]` modules in `src/`, and the alternative — a Bluetooth adapter plus a second
    /// radio acting as a central — is why every test in `tests/server/bluetooth_ble/e2e_test.rs`
    /// is `#[ignore]`d. `parse_ble_uuid` above is `pub` for the same reason.
    #[cfg(feature = "bluetooth-ble")]
    pub async fn run_event_loop_without_radio(
        event_rx: mpsc::Receiver<PeripheralEvent>,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        let server_data = Arc::new(Mutex::new(ServerData {
            hub: None,
            event_tx: None,
            state: ConnectionState::Idle,
            memory: String::new(),
            characteristics: HashMap::new(),
            queued_events: Vec::new(),
        }));

        Self::event_loop(
            event_rx,
            server_id,
            llm_client,
            app_state,
            status_tx,
            server_data,
            Arc::new(BluetoothBleProtocol::new()),
        )
        .await;
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
                    Log::new(Some(&status_tx))
                        .info(format!("Bluetooth state update: powered = {}", is_powered));

                    // Create event for LLM
                    let llm_event = Event::new(
                        &BLUETOOTH_STATE_CHANGED_EVENT,
                        serde_json::json!({
                            "state": if is_powered { "powered_on" } else { "powered_off" },
                        }),
                    );

                    // Call LLM with state change. There is no peer waiting on an adapter
                    // state change, so silence towards the radio is the only possible
                    // behaviour - but the failure must still be visible. This used to be
                    // `let _ = ...`, which discarded the error with no log on either channel.
                    if let Err(e) = Self::call_llm_for_event(
                        &server_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &server_data,
                        &protocol,
                        llm_event,
                    )
                    .await
                    {
                        console_error!(
                            status_tx,
                            "BLE adapter state change (powered = {}) was not handled: the \
                             handler failed ({}). Nothing was reconfigured.",
                            is_powered,
                            e
                        );
                    }
                }
                PeripheralEvent::ReadRequest {
                    request,
                    offset,
                    responder,
                } => {
                    let char_uuid_str = request.characteristic.to_string();

                    Log::new(Some(&status_tx)).debug(format!(
                        "BLE read request on {} (offset: {})",
                        char_uuid_str, offset
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
                                }),
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
                            )
                            .await
                            {
                                Ok(llm_result) => {
                                    // Look for a read response the model produced.
                                    let llm_value = llm_result
                                        .raw_actions
                                        .iter()
                                        .find(|a| {
                                            a.get("type").and_then(|v| v.as_str())
                                                == Some("respond_to_read")
                                                || a.get("type").and_then(|v| v.as_str())
                                                    == Some("send_read_response")
                                        })
                                        .and_then(|a| a.get("value"))
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| {
                                            let s = s.trim_start_matches("0x");
                                            hex::decode(s).ok()
                                        });

                                    // If the model gave no value, fall back to the
                                    // characteristic's last stored value. This used to be a
                                    // `futures::executor::block_on(server_data.lock())` inside a
                                    // synchronous `unwrap_or_else` closure — a blocking lock
                                    // acquired on a tokio worker thread. That is the exact
                                    // antipattern the root CLAUDE.md documents: on a runtime
                                    // thread it can panic ("Cannot block the current thread from
                                    // within a runtime"), and the panic is swallowed by the
                                    // `tokio::spawn` running this loop, so the server would look
                                    // healthy while the read died. We are already on the async
                                    // path here, so the lock is simply `.await`ed.
                                    let value = match llm_value {
                                        Some(v) => v,
                                        None => {
                                            let guard = server_data.lock().await;
                                            guard
                                                .characteristics
                                                .get(&char_uuid_str)
                                                .map(|c| c.current_value.clone())
                                                .unwrap_or_default()
                                        }
                                    };

                                    let _ = responder.send(ReadRequestResponse {
                                        value,
                                        response: RequestResponse::Success,
                                    });
                                }
                                Err(e) => {
                                    // Fail closed, and say so on both channels. ATT has no
                                    // "try again later"; `UnlikelyError` (0x0E) is the generic
                                    // "the server could not do it" the central will surface as
                                    // a failed read. What must not happen is the fallback in
                                    // the Ok branch above — answering Success with the last
                                    // cached value, which is a claim about the characteristic
                                    // that nothing here is in a position to make.
                                    console_error!(
                                        status_tx,
                                        "BLE read of {} could not be answered: the handler \
                                         failed ({}). Replying ATT Unlikely Error (0x0E); no \
                                         characteristic value is being invented.",
                                        char_uuid_str,
                                        e
                                    );
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
                                PeripheralEvent::ReadRequest {
                                    request,
                                    offset,
                                    responder,
                                },
                            );
                        }
                        ConnectionState::Accumulating => {
                            // Also queue
                            server_data.lock().await.queued_events.push(
                                PeripheralEvent::ReadRequest {
                                    request,
                                    offset,
                                    responder,
                                },
                            );
                        }
                    }
                }
                PeripheralEvent::WriteRequest {
                    request,
                    value,
                    offset,
                    responder,
                } => {
                    let char_uuid_str = request.characteristic.to_string();
                    let value_hex = hex::encode(&value);

                    Log::new(Some(&status_tx)).debug(format!(
                        "BLE write request on {} ({} bytes)",
                        char_uuid_str,
                        value.len()
                    ));
                    console_trace!(status_tx, "BLE write data (hex): {}", value_hex);

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
                                if let Some(char_data) =
                                    guard.characteristics.get_mut(&char_uuid_str)
                                {
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
                                }),
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
                            )
                            .await
                            {
                                Ok(_) => {
                                    let _ = responder.send(WriteRequestResponse {
                                        response: RequestResponse::Success,
                                    });
                                }
                                Err(e) => {
                                    // Fail closed: the central must be told the write did not
                                    // take effect. An ATT Write Response is an acknowledgement,
                                    // so answering Success here would tell the peer its value
                                    // was accepted by a handler that never ran.
                                    console_error!(
                                        status_tx,
                                        "BLE write to {} could not be answered: the handler \
                                         failed ({}). Replying ATT Unlikely Error (0x0E); the \
                                         write is NOT acknowledged.",
                                        char_uuid_str,
                                        e
                                    );
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
                                PeripheralEvent::WriteRequest {
                                    request,
                                    value,
                                    offset,
                                    responder,
                                },
                            );
                        }
                        ConnectionState::Accumulating => {
                            // Also queue
                            server_data.lock().await.queued_events.push(
                                PeripheralEvent::WriteRequest {
                                    request,
                                    value,
                                    offset,
                                    responder,
                                },
                            );
                        }
                    }
                }
                PeripheralEvent::CharacteristicSubscriptionUpdate {
                    request,
                    subscribed,
                } => {
                    let char_uuid_str = request.characteristic.to_string();
                    if subscribed {
                        console_info!(
                            status_tx,
                            "Client subscribed to notifications on {}",
                            char_uuid_str
                        );
                    } else {
                        console_info!(
                            status_tx,
                            "Client unsubscribed from notifications on {}",
                            char_uuid_str
                        );
                    }

                    let llm_event = Event::new(
                        &BLUETOOTH_SUBSCRIBE_EVENT,
                        serde_json::json!({
                            "characteristic_uuid": char_uuid_str,
                            "subscribed": subscribed,
                        }),
                    );

                    // A CCCD subscription update is reported after the fact - the stack has
                    // already acknowledged the descriptor write and there is no responder here,
                    // so there is nothing to answer. What there is to do is say the
                    // subscription will not be served, because the notifications the central is
                    // now waiting for were never set up. This used to be `let _ = ...`.
                    if let Err(e) = Self::call_llm_for_event(
                        &server_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &server_data,
                        &protocol,
                        llm_event,
                    )
                    .await
                    {
                        console_error!(
                            status_tx,
                            "BLE subscription change on {} (subscribed = {}) was not handled: \
                             the handler failed ({}). No notifications will be sent for it.",
                            char_uuid_str,
                            subscribed,
                            e
                        );
                    }
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
        )
        .await?;

        // Execute returned actions (except read/write responses which are handled inline)
        for action in &llm_result.raw_actions {
            let action_type = action.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match action_type {
                "respond_to_read"
                | "send_read_response"
                | "respond_to_write"
                | "send_write_response" => {
                    // These are handled inline in the event match arms
                    continue;
                }
                _ => {
                    if let Err(e) =
                        Self::execute_action(server_data, "NetGet-BLE", action.clone(), status_tx)
                            .await
                    {
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
        anyhow::bail!(
            "Bluetooth server support not enabled - compile with --features bluetooth-ble"
        )
    }
}
