//! ISO 7816-4 APDU (Application Protocol Data Unit) implementation
//!
//! This module implements APDU command/response handling for smart cards.
//!
//! ## APDU Command Format
//!
//! ```text
//! | CLA (1) | INS (1) | P1 (1) | P2 (1) | [Lc (1) | Data (Lc)] | [Le (1)] |
//! ```
//!
//! ## APDU Response Format
//!
//! ```text
//! | [Data] | SW1 (1) | SW2 (1) |
//! ```

#[cfg(feature = "usb-smartcard")]
use anyhow::{bail, Context, Result};
#[cfg(feature = "usb-smartcard")]
use tracing::debug;

/// ISO 7816-4 instruction codes
#[cfg(feature = "usb-smartcard")]
pub mod ins {
    pub const SELECT: u8 = 0xA4;
    pub const READ_BINARY: u8 = 0xB0;
    pub const UPDATE_BINARY: u8 = 0xD6;
    pub const VERIFY: u8 = 0x20;
    pub const GET_RESPONSE: u8 = 0xC0;
    pub const GET_DATA: u8 = 0xCA;
    pub const PUT_DATA: u8 = 0xDA;
    pub const INTERNAL_AUTHENTICATE: u8 = 0x88;
}

/// ISO 7816-4 status words
#[cfg(feature = "usb-smartcard")]
pub mod sw {
    pub const SUCCESS: u16 = 0x9000;
    pub const BYTES_REMAINING: u8 = 0x61; // SW1, SW2 = number of bytes
    pub const WARNING: u16 = 0x6200;
    pub const FILE_NOT_FOUND: u16 = 0x6A82;
    pub const WRONG_LENGTH: u16 = 0x6700;
    pub const SECURITY_NOT_SATISFIED: u16 = 0x6982;
    pub const AUTH_METHOD_BLOCKED: u16 = 0x6983;
    pub const WRONG_DATA: u16 = 0x6A80;
    pub const FUNCTION_NOT_SUPPORTED: u16 = 0x6A81;
    pub const INCORRECT_P1_P2: u16 = 0x6A86;
    pub const INS_NOT_SUPPORTED: u16 = 0x6D00;
    pub const CLA_NOT_SUPPORTED: u16 = 0x6E00;
}

/// APDU Command
#[cfg(feature = "usb-smartcard")]
#[derive(Debug, Clone)]
pub struct ApduCommand {
    pub cla: u8,
    pub ins: u8,
    pub p1: u8,
    pub p2: u8,
    pub data: Vec<u8>,
    pub le: Option<u8>,
}

#[cfg(feature = "usb-smartcard")]
impl ApduCommand {
    /// Parse APDU command from bytes
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 {
            bail!("APDU too short: {} bytes", bytes.len());
        }

        let cla = bytes[0];
        let ins = bytes[1];
        let p1 = bytes[2];
        let p2 = bytes[3];

        let (data, le) = if bytes.len() == 4 {
            // Case 1: No data, no Le
            (Vec::new(), None)
        } else if bytes.len() == 5 {
            // Case 2: No data, Le present
            (Vec::new(), Some(bytes[4]))
        } else {
            let lc = bytes[4] as usize;
            if bytes.len() >= 5 + lc {
                let data = bytes[5..5 + lc].to_vec();
                let le = if bytes.len() > 5 + lc {
                    Some(bytes[5 + lc])
                } else {
                    None
                };
                (data, le)
            } else {
                bail!("APDU data length mismatch");
            }
        };

        Ok(Self {
            cla,
            ins,
            p1,
            p2,
            data,
            le,
        })
    }

    /// Get instruction name for logging
    pub fn ins_name(&self) -> &'static str {
        match self.ins {
            ins::SELECT => "SELECT",
            ins::READ_BINARY => "READ_BINARY",
            ins::UPDATE_BINARY => "UPDATE_BINARY",
            ins::VERIFY => "VERIFY",
            ins::GET_RESPONSE => "GET_RESPONSE",
            ins::GET_DATA => "GET_DATA",
            ins::PUT_DATA => "PUT_DATA",
            ins::INTERNAL_AUTHENTICATE => "INTERNAL_AUTHENTICATE",
            _ => "UNKNOWN",
        }
    }
}

/// APDU Response
#[cfg(feature = "usb-smartcard")]
pub struct ApduResponse {
    pub data: Vec<u8>,
    pub sw: u16,
}

#[cfg(feature = "usb-smartcard")]
impl ApduResponse {
    pub fn success(data: Vec<u8>) -> Self {
        Self {
            data,
            sw: sw::SUCCESS,
        }
    }

    pub fn error(sw: u16) -> Self {
        Self {
            data: Vec::new(),
            sw,
        }
    }

    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = self.data;
        bytes.extend_from_slice(&self.sw.to_be_bytes());
        bytes
    }
}

/// Simple file system for smart card
#[cfg(feature = "usb-smartcard")]
pub struct SmartCardFileSystem {
    /// Currently selected file
    current_file: Option<u16>,
    /// File contents (file_id -> data)
    files: std::collections::HashMap<u16, Vec<u8>>,
}

#[cfg(feature = "usb-smartcard")]
impl SmartCardFileSystem {
    pub fn new() -> Self {
        let mut fs = Self {
            current_file: None,
            files: std::collections::HashMap::new(),
        };

        // Create some default files for demo
        // File 0x3F00: Master File (MF) - root directory
        fs.files.insert(0x3F00, vec![0x62, 0x00]); // Empty DF

        // File 0x2F00: Elementary File (EF) - test data
        fs.files
            .insert(0x2F00, b"Hello from NetGet Smart Card!".to_vec());

        fs
    }

    pub fn select_file(&mut self, file_id: u16) -> Result<Vec<u8>> {
        if self.files.contains_key(&file_id) {
            self.current_file = Some(file_id);
            debug!("Selected file: {:#06x}", file_id);
            // Return FCI (File Control Information)
            Ok(vec![0x6F, 0x00]) // Minimal FCI
        } else {
            bail!("File not found: {:#06x}", file_id);
        }
    }

    pub fn read_binary(&self, offset: u16, length: u8) -> Result<Vec<u8>> {
        let file_id = self.current_file.context("No file selected")?;
        let data = self.files.get(&file_id).context("File not found")?;

        let offset = offset as usize;
        let length = length as usize;

        if offset >= data.len() {
            Ok(Vec::new())
        } else {
            let end = (offset + length).min(data.len());
            Ok(data[offset..end].to_vec())
        }
    }

    pub fn update_binary(&mut self, offset: u16, data: &[u8]) -> Result<()> {
        let file_id = self.current_file.context("No file selected")?;
        let file_data = self.files.get_mut(&file_id).context("File not found")?;

        let offset = offset as usize;
        if offset + data.len() > file_data.len() {
            file_data.resize(offset + data.len(), 0);
        }

        file_data[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }
}

#[cfg(feature = "usb-smartcard")]
impl Default for SmartCardFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// APDU handler
#[cfg(feature = "usb-smartcard")]
pub struct ApduHandler {
    fs: SmartCardFileSystem,
    pin_verified: bool,
    key_store: Option<crate::server::usb::smartcard::crypto::SmartCardKeyStore>,
}

#[cfg(feature = "usb-smartcard")]
impl ApduHandler {
    pub fn new() -> Self {
        Self::new_with_crypto(false)
    }

    pub fn new_with_crypto(enable_crypto: bool) -> Self {
        let key_store = if enable_crypto {
            let mut store = crate::server::usb::smartcard::crypto::SmartCardKeyStore::new();
            // Pre-generate a demo key (key ref 0x9A - PIV authentication key)
            if let Err(e) = store.generate_key(0x9A, 2048) {
                debug!("Failed to generate demo key: {}", e);
            }
            Some(store)
        } else {
            None
        };

        Self {
            fs: SmartCardFileSystem::new(),
            pin_verified: false,
            key_store,
        }
    }

    /// Process APDU command
    pub fn process_command(&mut self, bytes: &[u8]) -> Vec<u8> {
        let cmd = match ApduCommand::parse(bytes) {
            Ok(cmd) => cmd,
            Err(e) => {
                debug!("Failed to parse APDU: {}", e);
                return ApduResponse::error(sw::WRONG_DATA).to_bytes();
            }
        };

        debug!(
            "APDU: {} (CLA={:#04x}, P1={:#04x}, P2={:#04x}, Lc={}, Le={:?})",
            cmd.ins_name(),
            cmd.cla,
            cmd.p1,
            cmd.p2,
            cmd.data.len(),
            cmd.le
        );

        let response = match cmd.ins {
            ins::SELECT => self.handle_select(&cmd),
            ins::READ_BINARY => self.handle_read_binary(&cmd),
            ins::UPDATE_BINARY => self.handle_update_binary(&cmd),
            ins::VERIFY => self.handle_verify(&cmd),
            ins::GET_DATA => self.handle_get_data(&cmd),
            ins::INTERNAL_AUTHENTICATE => self.handle_internal_authenticate(&cmd),
            _ => {
                debug!("Unsupported instruction: {:#04x}", cmd.ins);
                ApduResponse::error(sw::INS_NOT_SUPPORTED)
            }
        };

        response.to_bytes()
    }

    fn handle_select(&mut self, cmd: &ApduCommand) -> ApduResponse {
        // SELECT command: P1=selection method, P2=file control info
        // Data = file ID (2 bytes typically)

        if cmd.data.len() < 2 {
            return ApduResponse::error(sw::WRONG_LENGTH);
        }

        let file_id = u16::from_be_bytes([cmd.data[0], cmd.data[1]]);

        match self.fs.select_file(file_id) {
            Ok(fci) => ApduResponse::success(fci),
            Err(_) => ApduResponse::error(sw::FILE_NOT_FOUND),
        }
    }

    fn handle_read_binary(&self, cmd: &ApduCommand) -> ApduResponse {
        let offset = u16::from_be_bytes([cmd.p1, cmd.p2]);
        let length = cmd.le.unwrap_or(0);

        match self.fs.read_binary(offset, length) {
            Ok(data) => ApduResponse::success(data),
            Err(_) => ApduResponse::error(sw::FILE_NOT_FOUND),
        }
    }

    fn handle_update_binary(&mut self, cmd: &ApduCommand) -> ApduResponse {
        if !self.pin_verified {
            return ApduResponse::error(sw::SECURITY_NOT_SATISFIED);
        }

        let offset = u16::from_be_bytes([cmd.p1, cmd.p2]);

        match self.fs.update_binary(offset, &cmd.data) {
            Ok(_) => ApduResponse::success(Vec::new()),
            Err(_) => ApduResponse::error(sw::WRONG_DATA),
        }
    }

    fn handle_verify(&mut self, cmd: &ApduCommand) -> ApduResponse {
        // Simplified PIN verification (P2 = reference, data = PIN)
        // For demo, accept PIN "123456"
        let pin = String::from_utf8_lossy(&cmd.data);

        if pin == "123456" {
            self.pin_verified = true;
            debug!("PIN verified successfully");
            ApduResponse::success(Vec::new())
        } else {
            debug!("PIN verification failed");
            ApduResponse::error(sw::AUTH_METHOD_BLOCKED)
        }
    }

    fn handle_get_data(&self, cmd: &ApduCommand) -> ApduResponse {
        // GET_DATA: retrieve data objects
        // P1P2 = data object identifier
        let _data_id = u16::from_be_bytes([cmd.p1, cmd.p2]);

        // Return dummy data for demo
        ApduResponse::success(vec![0x01, 0x02, 0x03, 0x04])
    }

    fn handle_internal_authenticate(&self, cmd: &ApduCommand) -> ApduResponse {
        // INTERNAL_AUTHENTICATE: Sign data with private key
        // P2 = key reference
        if !self.pin_verified {
            debug!("INTERNAL_AUTHENTICATE rejected: PIN not verified");
            return ApduResponse::error(sw::SECURITY_NOT_SATISFIED);
        }

        let key_store = match &self.key_store {
            Some(store) => store,
            None => {
                debug!("INTERNAL_AUTHENTICATE rejected: Crypto not enabled");
                return ApduResponse::error(sw::FUNCTION_NOT_SUPPORTED);
            }
        };

        let key_ref = cmd.p2;
        let key_pair = match key_store.get_key(key_ref) {
            Some(kp) => kp,
            None => {
                debug!("INTERNAL_AUTHENTICATE: Key {:#04x} not found", key_ref);
                return ApduResponse::error(sw::WRONG_DATA);
            }
        };

        // Sign the challenge data
        match key_pair.sign(&cmd.data) {
            Ok(signature) => {
                debug!(
                    "INTERNAL_AUTHENTICATE: Signed {} bytes with key {:#04x}, signature {} bytes",
                    cmd.data.len(),
                    key_ref,
                    signature.len()
                );
                ApduResponse::success(signature)
            }
            Err(e) => {
                debug!("INTERNAL_AUTHENTICATE failed: {}", e);
                ApduResponse::error(sw::WRONG_DATA)
            }
        }
    }
}

#[cfg(feature = "usb-smartcard")]
impl Default for ApduHandler {
    fn default() -> Self {
        Self::new()
    }
}
