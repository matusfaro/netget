//! Transport for beacon advertisements.
//!
//! A beacon *is* its advertising payload, so the only question that matters for a platform is
//! whether the OS lets an application set manufacturer-specific and service advertising data.
//!
//! - **Linux** can: BlueZ exposes `org.bluez.LEAdvertisement1` (with `ManufacturerData` and
//!   `ServiceData` properties) and registers it through `org.bluez.LEAdvertisingManager1`. The
//!   `bluer` crate — already in this tree's dependency graph, as `ble-peripheral-rust` uses it
//!   for its own Linux backend — speaks exactly that D-Bus API, so it is reused here rather
//!   than adding a second D-Bus stack.
//! - **macOS cannot, at all.** `-[CBPeripheralManager startAdvertising:]` documents exactly two
//!   honoured keys, `CBAdvertisementDataLocalNameKey` and `CBAdvertisementDataServiceUUIDsKey`;
//!   every other key "is ignored". Writing our own CoreBluetooth bindings would change nothing,
//!   because the restriction is in CoreBluetooth and not in any Rust wrapper.
//! - **Windows**: `BluetoothLEAdvertisementPublisher` *can* carry manufacturer data, but not
//!   service data, and none of this has been written or tested. It refuses like macOS.
//!
//! Refusing is the whole point of the non-Linux half of this file. The protocol used to start
//! "successfully" on macOS and sit in `Running` while emitting an advertisement no beacon
//! scanner could recognise — a server that lies about being up.

use crate::server::bluetooth_ble_beacon::payload::BeaconFrame;
use anyhow::Result;

/// Message shown when the host OS cannot express a beacon advertisement.
///
/// A single `const` so the runtime error, the protocol description and the tests all quote the
/// same text.
pub const UNSUPPORTED_PLATFORM_MESSAGE: &str = concat!(
    "BLE beacons require setting manufacturer-specific or service advertising data, which ",
    "only Linux/BlueZ can do. On macOS, CBPeripheralManager.startAdvertising: honours only ",
    "CBAdvertisementDataLocalNameKey and CBAdvertisementDataServiceUUIDsKey and documents ",
    "every other key as ignored, so an iBeacon or Eddystone payload cannot be emitted at all; ",
    "Windows advertising is not implemented here. Run bluetooth-ble-beacon on Linux with ",
    "bluetoothd running, or use the bluetooth-ble GATT server instead."
);

/// Handle to whatever is currently being broadcast.
///
/// Dropping this must stop the advertisement. On Linux that falls out of `bluer`'s
/// `AdvertisementHandle`, which unregisters from `LEAdvertisingManager1` on drop, so the
/// advertisement dies with the server rather than outliving it in `bluetoothd`.
pub struct BeaconAdvertiser {
    #[cfg(target_os = "linux")]
    inner: linux::LinuxAdvertiser,
    /// What is currently on air, if anything.
    current: Option<BeaconFrame>,
    /// Device name requested at startup; only advertised when the frame leaves room.
    device_name: String,
}

impl BeaconAdvertiser {
    /// Open the adapter and prepare to advertise.
    ///
    /// Returns `Err` — before any server is reported as running — when the platform cannot set
    /// an advertising payload, when there is no adapter, or when `bluetoothd` is unreachable.
    #[cfg(target_os = "linux")]
    pub async fn open(device_name: String, adapter: Option<String>) -> Result<Self> {
        Ok(Self {
            inner: linux::LinuxAdvertiser::open(adapter.as_deref()).await?,
            current: None,
            device_name,
        })
    }

    /// Non-Linux: refuse, with the reason.
    #[cfg(not(target_os = "linux"))]
    pub async fn open(_device_name: String, _adapter: Option<String>) -> Result<Self> {
        anyhow::bail!(UNSUPPORTED_PLATFORM_MESSAGE)
    }

    /// The adapter this advertiser is bound to (`hci0`, …).
    pub fn adapter_name(&self) -> &str {
        #[cfg(target_os = "linux")]
        {
            self.inner.adapter_name()
        }
        #[cfg(not(target_os = "linux"))]
        {
            "none"
        }
    }

    /// Replace whatever is on air with `frame`.
    ///
    /// Registering a second advertisement while the first is live would leave BlueZ rotating
    /// between two payloads, so the previous one is dropped first.
    #[cfg(target_os = "linux")]
    pub async fn start(&mut self, frame: BeaconFrame) -> Result<()> {
        let local_name = frame.fit_local_name(&self.device_name).map(str::to_string);
        self.inner.advertise(&frame, local_name).await?;
        self.current = Some(frame);
        Ok(())
    }

    /// Non-Linux: unreachable in practice, because `open` already refused.
    #[cfg(not(target_os = "linux"))]
    pub async fn start(&mut self, _frame: BeaconFrame) -> Result<()> {
        anyhow::bail!(UNSUPPORTED_PLATFORM_MESSAGE)
    }

    /// Stop advertising. Idempotent.
    pub async fn stop(&mut self) {
        #[cfg(target_os = "linux")]
        self.inner.clear();
        self.current = None;
    }

    /// What is currently being broadcast, if anything.
    pub fn current(&self) -> Option<&BeaconFrame> {
        self.current.as_ref()
    }

    /// The device name this advertiser was started with.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::BeaconFrame;
    use anyhow::{Context, Result};
    use bluer::adv::{Advertisement, AdvertisementHandle, Type};
    use std::collections::{BTreeMap, BTreeSet};
    use tracing::{debug, info};

    /// Live BlueZ session, adapter, and the registered advertisement.
    pub struct LinuxAdvertiser {
        /// Held for its lifetime: dropping the session tears down the D-Bus connection the
        /// advertisement is registered on.
        _session: bluer::Session,
        adapter: bluer::Adapter,
        adapter_name: String,
        /// `Some` while an advertisement is registered; dropping it unregisters.
        handle: Option<AdvertisementHandle>,
    }

    impl LinuxAdvertiser {
        pub async fn open(adapter: Option<&str>) -> Result<Self> {
            let session = bluer::Session::new().await.context(
                "Failed to connect to BlueZ over D-Bus. Is bluetoothd running \
                 (systemctl start bluetooth)?",
            )?;

            let adapter = match adapter {
                Some(name) => session
                    .adapter(name)
                    .with_context(|| format!("No Bluetooth adapter named {name:?}"))?,
                None => session
                    .default_adapter()
                    .await
                    .context("No Bluetooth adapter found")?,
            };

            adapter
                .set_powered(true)
                .await
                .context("Failed to power on the Bluetooth adapter")?;

            let adapter_name = adapter.name().to_string();
            info!("BLE beacon bound to adapter {}", adapter_name);

            Ok(Self {
                _session: session,
                adapter,
                adapter_name,
                handle: None,
            })
        }

        pub fn adapter_name(&self) -> &str {
            &self.adapter_name
        }

        /// Register `frame` as a broadcast advertisement, replacing any previous one.
        pub async fn advertise(
            &mut self,
            frame: &BeaconFrame,
            local_name: Option<String>,
        ) -> Result<()> {
            // Unregister first: two live advertisements make BlueZ rotate between payloads.
            self.handle = None;

            let mut manufacturer_data = BTreeMap::new();
            if let Some((company, data)) = frame.manufacturer_data() {
                manufacturer_data.insert(company, data);
            }

            let mut service_data = BTreeMap::new();
            let mut service_uuids = BTreeSet::new();
            if let Some((uuid16, data)) = frame.service_data() {
                let uuid = uuid16_to_uuid(uuid16);
                service_data.insert(uuid, data);
                service_uuids.insert(uuid);
            }

            let advertisement = Advertisement {
                // Broadcast is ADV_NONCONN_IND: a beacon accepts no connections, and BlueZ
                // rejects `discoverable` on this type, so it is left unset.
                advertisement_type: Type::Broadcast,
                manufacturer_data,
                service_data,
                service_uuids,
                local_name,
                ..Default::default()
            };

            debug!(
                "Registering BlueZ advertisement on {}: {}",
                self.adapter_name,
                frame.describe()
            );

            let handle = self
                .adapter
                .advertise(advertisement)
                .await
                .context("BlueZ refused to register the beacon advertisement")?;
            self.handle = Some(handle);
            Ok(())
        }

        /// Drop the registration, stopping the broadcast.
        pub fn clear(&mut self) {
            self.handle = None;
        }
    }

    /// Expand a 16-bit Bluetooth SIG UUID into its 128-bit form.
    ///
    /// BlueZ's D-Bus API is 128-bit only and narrows back to 16 bits itself when building the
    /// AD structure, so 0xFEAA must be sent as `0000feaa-0000-1000-8000-00805f9b34fb`.
    fn uuid16_to_uuid(uuid16: u16) -> uuid::Uuid {
        uuid::Uuid::from_fields(
            uuid16 as u32,
            0x0000,
            0x1000,
            &[0x80, 0x00, 0x00, 0x80, 0x5f, 0x9b, 0x34, 0xfb],
        )
    }
}
