//! USB Smart Card Reader (CCID) server implementation
//!
//! This module implements a virtual smart card that connects to vpcd daemon.
//! This approach avoids implementing USB CCID by using the vsmartcard infrastructure.
//!
//! ## Implementation Status
//!
//! **FUNCTIONAL IMPLEMENTATION** - ISO 7816-4 APDU handling with RSA cryptography support.
//! See src/server/usb/smartcard/CLAUDE.md for full implementation plan.
//!
//! ## Architecture
//!
//! ```
//! ┌───────────┐    TCP     ┌──────┐    PC/SC    ┌─────────┐
//! │  NetGet   │ ◄─────────► vpcd  │ ◄──────────► pcscd   │
//! │ + APDU    │  port 35963│ drv  │             │ daemon  │
//! └───────────┘            └──────┘             └─────────┘
//! ```
//!
//! ## Setup Requirements
//!
//! 1. Install vpcd daemon:
//!    ```bash
//!    sudo apt-get install vpcd  # Ubuntu/Debian
//!    ```
//!
//! 2. Start vpcd daemon:
//!    ```bash
//!    vpcd --tcp-port 35963
//!    ```
//!
//! 3. Start NetGet with smart card protocol:
//!    ```bash
//!    netget --protocol usb-smartcard
//!    ```
//!
//! ## Supported Features
//!
//! - ✅ ISO 7816-4 APDU parsing
//! - ✅ Basic file system (SELECT, READ_BINARY, UPDATE_BINARY)
//! - ✅ PIN verification (VERIFY command)
//! - ✅ RSA-2048 key generation and storage
//! - ✅ INTERNAL_AUTHENTICATE (RSA-SHA256 signatures)
//! - ✅ vpcd protocol integration
//! - ⚠️  No persistent storage (in-memory only)
//! - ⚠️  No card applications (PIV/OpenPGP) yet

pub mod actions;
pub mod apdu;
pub mod crypto;

#[cfg(feature = "usb-smartcard")]
use anyhow::Result;
#[cfg(feature = "usb-smartcard")]
use std::net::SocketAddr;
#[cfg(feature = "usb-smartcard")]
use std::sync::Arc;
#[cfg(feature = "usb-smartcard")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(feature = "usb-smartcard")]
use tokio::net::TcpStream;
#[cfg(feature = "usb-smartcard")]
use tokio::sync::mpsc;
#[cfg(feature = "usb-smartcard")]
use tracing::{debug, error, info, warn};

#[cfg(feature = "usb-smartcard")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "usb-smartcard")]
use crate::state::app_state::AppState;

#[cfg(feature = "usb-smartcard")]
use apdu::ApduHandler;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// USB Smart Card server (using vpcd approach)
#[cfg(feature = "usb-smartcard")]
pub struct UsbSmartCardServer;

/// Default Answer To Reset (ATR) for a generic smart card
/// This identifies the card to the PC/SC system
#[cfg(feature = "usb-smartcard")]
const DEFAULT_ATR: &[u8] = &[
    0x3B, // TS: Direct convention
    0x9F, // T0: Y1=9, K=15
    0x95, // TA1: Fi=372, Di=4
    0x81, // TB1: Programming voltage
    0x31, // TC1: Extra guard time
    0xFE, // TD1: Y2=F, T=14 (T=14)
    0x5D, // TA2: Specific mode, cannot change
    0x00, // TB2
    0x31, // TC2
    0x80, // TD2
    0x31, // TA3
    0x80, // TB3
    0x65, // TC3
    0xB0, // TD3
    0x05, 0x01, 0x02, 0x03, 0x04, 0x05, // Historical bytes
    0x29, // TCK: Checksum
];

#[cfg(feature = "usb-smartcard")]
impl UsbSmartCardServer {
    /// Spawn the USB Smart Card server
    ///
    /// This implementation creates a simplified vpcd client that:
    /// 1. Connects to vpcd daemon (localhost:35963 default)
    /// 2. Sends ATR (Answer To Reset)
    /// 3. Processes APDU commands
    /// 4. Returns APDU responses
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _card_type: Option<String>,
        vpcd_host: Option<String>,
        vpcd_port: Option<u16>,
    ) -> Result<SocketAddr> {
        let vpcd_host = vpcd_host.unwrap_or_else(|| "localhost".to_string());
        let vpcd_port = vpcd_port.unwrap_or(35963);
        let vpcd_addr = format!("{}:{}", vpcd_host, vpcd_port);

        info!("USB Smart Card server connecting to vpcd at {}", vpcd_addr);

        // Connect to vpcd daemon
        let stream = match TcpStream::connect(&vpcd_addr).await {
            Ok(stream) => {
                info!("Connected to vpcd daemon at {}", vpcd_addr);
                stream
            }
            Err(e) => {
                error!("Failed to connect to vpcd at {}: {}", vpcd_addr, e);
                console_error!(status_tx, "Smart Card ERROR: vpcd daemon not running at {}");
                return Err(e.into());
            }
        };

        let local_addr = stream.local_addr()?;

        console_info!(status_tx, "USB Smart Card connected to vpcd at {} (ISO 7816-4 APDU ready)");

        // Spawn handler task
        tokio::spawn(async move {
            if let Err(e) = Self::handle_vpcd_connection(stream, status_tx).await {
                error!("Smart card connection error: {}", e);
            }
        });

        Ok(local_addr)
    }

    /// Handle vpcd connection and APDU processing
    async fn handle_vpcd_connection(
        mut stream: TcpStream,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Enable crypto for RSA signing support
        let mut apdu_handler = ApduHandler::new_with_crypto(true);

        // Send ATR (Answer To Reset)
        // vpcd protocol: send ATR length (u16 big-endian) + ATR bytes
        let atr_len = DEFAULT_ATR.len() as u16;
        stream.write_u16(atr_len).await?;
        stream.write_all(DEFAULT_ATR).await?;
        stream.flush().await?;

        console_info!(status_tx, "Smart Card: ATR sent, ready for APDU commands");

        // Main loop: receive APDU commands, send responses
        let mut buffer = vec![0u8; 4096];

        loop {
            // Read APDU length (u16 big-endian)
            let apdu_len = match stream.read_u16().await {
                Ok(len) => len as usize,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    info!("vpcd connection closed");
                    break;
                }
                Err(e) => {
                    error!("Failed to read APDU length: {}", e);
                    break;
                }
            };

            if apdu_len == 0 || apdu_len > buffer.len() {
                warn!("Invalid APDU length: {}", apdu_len);
                continue;
            }

            // Read APDU data
            stream.read_exact(&mut buffer[..apdu_len]).await?;
            let apdu_cmd = &buffer[..apdu_len];

            debug!("Received APDU command: {} bytes", apdu_len);

            // Process APDU command
            let apdu_response = apdu_handler.process_command(apdu_cmd);

            // Send response length (u16 big-endian) + response bytes
            let response_len = apdu_response.len() as u16;
            stream.write_u16(response_len).await?;
            stream.write_all(&apdu_response).await?;
            stream.flush().await?;


            // Notify about command processing
            console_debug!(status_tx, "Smart Card: Processed APDU ({} bytes in, {} bytes out)");
        }

        info!("Smart card vpcd connection closed");
        Ok(())
    }
}
