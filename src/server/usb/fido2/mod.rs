//! USB FIDO2/U2F Security Key server implementation
//!
//! This module implements a virtual FIDO2/U2F security key using the USB/IP protocol.
//! The device appears as a USB HID device implementing the CTAPHID protocol.
//!
//! ## Implementation Status
//!
//! **FUNCTIONAL IMPLEMENTATION** - Core U2F and FIDO2 GetInfo commands working.
//! See src/server/usb/fido2/CLAUDE.md for detailed implementation plan.
//!
//! ## Architecture
//!
//! ```
//! ┌────────────┐    USB/IP    ┌──────────┐    CTAPHID    ┌─────────────┐
//! │   Browser  │ ◄───────────► │ USB/IP   │ ◄────────────► │  NetGet     │
//! │  (Chrome)  │     HID       │ Client   │    64-byte    │  + FIDO2    │
//! └────────────┘               └──────────┘    packets    └─────────────┘
//! ```
//!
//! ## Components
//!
//! - **CTAPHID** (`ctaphid.rs`): HID transport protocol with packet fragmentation
//! - **U2F** (`u2f.rs`): CTAP1 commands (REGISTER, AUTHENTICATE, VERSION)
//! - **CTAP2** (`ctap2.rs`): FIDO2 commands (MakeCredential, GetAssertion, GetInfo)
//!
//! ## Supported Features
//!
//! - ✅ CTAPHID transport layer (packet fragmentation, channel management)
//! - ✅ U2F_VERSION command
//! - ✅ U2F_REGISTER command (full implementation with ECDSA P-256)
//! - ✅ U2F_AUTHENTICATE command (full implementation)
//! - ✅ CTAP2 GetInfo command
//! - ✅ CTAP2 MakeCredential (full credential creation with COSE keys)
//! - ✅ CTAP2 GetAssertion (full assertion with ECDSA signatures)
//! - ✅ Credential storage and management
//!
//! ## Limitations
//!
//! - No persistent credential storage (in-memory only, LLM controls persistence)
//! - No PIN/UV support
//! - No resident key support
//! - Simplified attestation (no proper certificate chain)
//! - No LLM integration for user presence yet

pub mod actions;
pub mod ctaphid;
pub mod u2f;
pub mod ctap2;

#[cfg(feature = "usb-fido2")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-fido2")]
use std::net::SocketAddr;
#[cfg(feature = "usb-fido2")]
use std::sync::Arc;
#[cfg(feature = "usb-fido2")]
use tokio::sync::mpsc;
#[cfg(feature = "usb-fido2")]
use tracing::{debug, error, info, warn};

#[cfg(feature = "usb-fido2")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "usb-fido2")]
use crate::state::app_state::AppState;

#[cfg(feature = "usb-fido2")]
use crate::server::usb::descriptors::{build_fido2_hid_config_descriptor, FIDO_HID_REPORT_DESCRIPTOR};
#[cfg(feature = "usb-fido2")]
use crate::server::usb::common::UsbSpeed;

#[cfg(feature = "usb-fido2")]
use ctaphid::{CtapHidCommand, CtapHidHandler, CtapHidPacket};
#[cfg(feature = "usb-fido2")]
use u2f::U2fHandler;
#[cfg(feature = "usb-fido2")]
use ctap2::Ctap2Handler;

/// USB FIDO2 Security Key server
#[cfg(feature = "usb-fido2")]
pub struct UsbFido2Server;

/// FIDO2 USB/IP HID handler
#[cfg(feature = "usb-fido2")]
struct Fido2HidHandler {
    /// CTAPHID protocol handler
    ctaphid: CtapHidHandler,
    /// U2F command handler
    u2f: U2fHandler,
    /// CTAP2 command handler
    ctap2: Ctap2Handler,
    /// Pending response packets
    response_packets: Vec<Vec<u8>>,
}

#[cfg(feature = "usb-fido2")]
impl Fido2HidHandler {
    fn new() -> Self {
        Self {
            ctaphid: CtapHidHandler::new(),
            u2f: U2fHandler::new(),
            ctap2: Ctap2Handler::new(),
            response_packets: Vec::new(),
        }
    }

    /// Process CTAPHID command
    fn process_ctaphid_command(&mut self, cid: u32, cmd: CtapHidCommand, data: &[u8]) -> Vec<Vec<u8>> {
        debug!("CTAPHID command: {:?}, cid={:#010x}, data_len={}", cmd, cid, data.len());

        let response_data = match cmd {
            CtapHidCommand::Init => {
                // INIT command: allocate new channel
                // Request: 8-byte nonce
                // Response: 8-byte nonce + 4-byte CID + protocol version + capabilities
                if data.len() < 8 {
                    return vec![CtapHidPacket::build_error(cid, ctaphid::CtapHidError::InvalidLen)];
                }

                let new_cid = self.ctaphid.allocate_channel();
                let nonce = &data[..8];

                let mut response = Vec::new();
                response.extend_from_slice(nonce); // Echo nonce
                response.extend_from_slice(&new_cid.to_be_bytes()); // New CID
                response.push(2); // Protocol version (CTAP 2.0)
                response.push(0); // Major device version
                response.push(0); // Minor device version
                response.push(0); // Build device version
                response.push(0x01); // Capabilities (WINK support)

                info!("CTAPHID INIT: allocated CID {:#010x}", new_cid);
                response
            }

            CtapHidCommand::Ping => {
                // PING command: echo data back
                debug!("CTAPHID PING: {} bytes", data.len());
                data.to_vec()
            }

            CtapHidCommand::Msg => {
                // MSG command: U2F/CTAP1 message
                debug!("CTAPHID MSG (U2F): processing {} bytes", data.len());
                self.u2f.process_command(data)
            }

            CtapHidCommand::Cbor => {
                // CBOR command: CTAP2 message
                debug!("CTAPHID CBOR (CTAP2): processing {} bytes", data.len());
                self.ctap2.process_command(data)
            }

            CtapHidCommand::Wink => {
                // WINK command: user presence test (no response data)
                debug!("CTAPHID WINK");
                Vec::new()
            }

            CtapHidCommand::Cancel => {
                // CANCEL command: cancel pending request
                debug!("CTAPHID CANCEL");
                Vec::new()
            }

            _ => {
                warn!("Unsupported CTAPHID command: {:?}", cmd);
                return vec![CtapHidPacket::build_error(cid, ctaphid::CtapHidError::InvalidCmd)];
            }
        };

        // Fragment response into HID packets
        self.ctaphid.fragment_response(cid, cmd, &response_data)
    }
}

#[cfg(feature = "usb-fido2")]
impl usbip::UsbInterfaceHandler for Fido2HidHandler {
    fn handle_urb(
        &mut self,
        setup: &usbip::SetupPacket,
    ) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::server::usb::common::{descriptor_type, hid_request, request, request_type};

        debug!(
            "FIDO2 control request: type={:#04x}, request={:#04x}, value={:#06x}",
            setup.request_type, setup.request, setup.value
        );

        match (setup.request_type, setup.request) {
            // Get HID Report Descriptor
            (request_type::DEVICE_TO_HOST | request_type::STANDARD | request_type::INTERFACE, request::GET_DESCRIPTOR) => {
                let desc_type = (setup.value >> 8) as u8;
                if desc_type == descriptor_type::HID_REPORT {
                    debug!("GET_DESCRIPTOR: HID Report ({}bytes)", FIDO_HID_REPORT_DESCRIPTOR.len());
                    Ok(FIDO_HID_REPORT_DESCRIPTOR.to_vec())
                } else {
                    warn!("Unsupported descriptor type: {:#04x}", desc_type);
                    Err("Unsupported descriptor".into())
                }
            }

            // Get/Set Idle
            (request_type::DEVICE_TO_HOST | request_type::CLASS | request_type::INTERFACE, hid_request::GET_IDLE) => {
                debug!("GET_IDLE");
                Ok(vec![0])
            }
            (request_type::HOST_TO_DEVICE | request_type::CLASS | request_type::INTERFACE, hid_request::SET_IDLE) => {
                debug!("SET_IDLE");
                Ok(vec![])
            }

            // Get/Set Protocol
            (request_type::DEVICE_TO_HOST | request_type::CLASS | request_type::INTERFACE, hid_request::GET_PROTOCOL) => {
                debug!("GET_PROTOCOL");
                Ok(vec![0]) // Report protocol
            }
            (request_type::HOST_TO_DEVICE | request_type::CLASS | request_type::INTERFACE, hid_request::SET_PROTOCOL) => {
                debug!("SET_PROTOCOL");
                Ok(vec![])
            }

            _ => {
                warn!(
                    "Unsupported FIDO2 control request: type={:#04x}, request={:#04x}",
                    setup.request_type, setup.request
                );
                Err("Unsupported control request".into())
            }
        }
    }

    fn handle_data_out(
        &mut self,
        ep: u8,
        data: &[u8],
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!("FIDO2 OUT: ep={:#04x}, {} bytes", ep, data.len());

        // Process CTAPHID packet
        match self.ctaphid.process_packet(data) {
            Ok(Some(message)) => {
                // Complete message received - process command
                let response_packets = self.process_ctaphid_command(
                    message.cid,
                    message.cmd,
                    &message.into_data(),
                );
                self.response_packets = response_packets;
            }
            Ok(None) => {
                // Continuation packet - waiting for more
                debug!("CTAPHID: waiting for continuation packets");
            }
            Err(e) => {
                warn!("CTAPHID packet error: {}", e);
                // Send error response
                self.response_packets = vec![CtapHidPacket::build_error(
                    0xffffffff,
                    ctaphid::CtapHidError::InvalidSeq,
                )];
            }
        }

        Ok(())
    }

    fn handle_data_in(
        &mut self,
        ep: u8,
    ) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        // Send next response packet if available
        if let Some(packet) = self.response_packets.first().cloned() {
            self.response_packets.remove(0);
            debug!("FIDO2 IN: ep={:#04x}, sending {} bytes", ep, packet.len());
            Ok(packet)
        } else {
            // No data to send
            Ok(vec![])
        }
    }

    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(feature = "usb-fido2")]
impl UsbFido2Server {
    /// Spawn the USB FIDO2 server with LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _support_u2f: Option<bool>,
        _support_fido2: Option<bool>,
    ) -> Result<SocketAddr> {
        info!("Starting USB FIDO2/U2F Security Key server on {}", listen_addr);

        // Create TCP listener for USB/IP protocol
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        // Create USB device descriptor for FIDO2 security key
        let device_desc = usbip::UsbDeviceDescriptor {
            bcd_usb: 0x0200,  // USB 2.0
            device_class: 0x00,  // Defined in interface
            device_sub_class: 0,
            device_protocol: 0,
            max_packet_size: 64,
            vendor_id: 0x1050,  // Yubico VID (for compatibility)
            product_id: 0x0407,  // FIDO U2F Security Key
            bcd_device: 0x0100,  // Device version 1.0
            manufacturer_string: 1,
            product_string: 2,
            serial_number_string: 3,
            num_configurations: 1,
        };

        // Create FIDO2 HID handler
        let handler = Box::new(Fido2HidHandler::new());
        let handler_arc = Arc::new(std::sync::Mutex::new(handler as Box<dyn usbip::UsbInterfaceHandler + Send>));

        // Create USB interface
        let mut interfaces = std::collections::HashMap::new();
        interfaces.insert(0, handler_arc);

        // Create USB device
        let device = usbip::UsbDevice::new(
            "usb-fido2".to_string(),
            device_desc,
            vec![build_fido2_hid_config_descriptor()],
            interfaces,
            UsbSpeed::High as u32,
        );

        let device_arc = Arc::new(std::sync::Mutex::new(device));

        info!("USB FIDO2 server ready on {}", local_addr);
        let _ = status_tx.send(format!(
            "USB FIDO2/U2F Security Key ready on {} (connect with 'usbip attach')",
            local_addr
        ));

        // Spawn USB/IP server task
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        info!("USB/IP connection from {}", peer_addr);
                        let device_clone = device_arc.clone();

                        tokio::spawn(async move {
                            if let Err(e) = usbip::handle_usbip_connection(stream, device_clone).await {
                                error!("USB/IP connection error: {}", e);
                            } else {
                                info!("USB/IP connection closed: {}", peer_addr);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept USB/IP connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
