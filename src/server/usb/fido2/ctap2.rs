//! CTAP2 (Client-to-Authenticator Protocol 2) / FIDO2 implementation
//!
//! This module implements the CTAP2 protocol commands:
//! - authenticatorMakeCredential (0x01): Create new credential
//! - authenticatorGetAssertion (0x02): Get authentication assertion
//! - authenticatorGetInfo (0x04): Get authenticator info
//! - authenticatorClientPIN (0x06): Client PIN management
//! - authenticatorReset (0x07): Reset authenticator
//! - authenticatorGetNextAssertion (0x08): Get next assertion from batch
//!
//! ## CTAP2 Message Format
//!
//! All CTAP2 messages are CBOR-encoded:
//! ```text
//! | Command (1) | CBOR Parameters (variable) |
//! ```
//!
//! ## Response Format
//!
//! ```text
//! | Status (1) | CBOR Response Data (variable) |
//! ```

#[cfg(feature = "usb-fido2")]
use anyhow::{bail, Context, Result};
#[cfg(feature = "usb-fido2")]
use serde_cbor::Value as CborValue;
#[cfg(feature = "usb-fido2")]
use std::collections::BTreeMap;
#[cfg(feature = "usb-fido2")]
use tracing::{debug, trace, warn};

/// CTAP2 command codes
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ctap2Command {
    MakeCredential = 0x01,
    GetAssertion = 0x02,
    GetInfo = 0x04,
    ClientPin = 0x06,
    Reset = 0x07,
    GetNextAssertion = 0x08,
}

#[cfg(feature = "usb-fido2")]
impl Ctap2Command {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::MakeCredential),
            0x02 => Some(Self::GetAssertion),
            0x04 => Some(Self::GetInfo),
            0x06 => Some(Self::ClientPin),
            0x07 => Some(Self::Reset),
            0x08 => Some(Self::GetNextAssertion),
            _ => None,
        }
    }
}

/// CTAP2 status codes
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ctap2Status {
    Success = 0x00,
    InvalidCommand = 0x01,
    InvalidParameter = 0x02,
    InvalidLength = 0x03,
    InvalidSeq = 0x04,
    Timeout = 0x05,
    ChannelBusy = 0x06,
    LockRequired = 0x0A,
    InvalidChannel = 0x0B,
    CborUnexpectedType = 0x11,
    InvalidCbor = 0x12,
    MissingParameter = 0x14,
    LimitExceeded = 0x15,
    UnsupportedExtension = 0x16,
    CredentialExcluded = 0x19,
    Processing = 0x21,
    InvalidCredential = 0x22,
    UserActionPending = 0x23,
    OperationPending = 0x24,
    NoOperations = 0x25,
    UnsupportedAlgorithm = 0x26,
    OperationDenied = 0x27,
    KeyStoreFull = 0x28,
    NotBusy = 0x29,
    NoOperationPending = 0x2A,
    UnsupportedOption = 0x2B,
    InvalidOption = 0x2C,
    KeepaliveCancel = 0x2D,
    NoCredentials = 0x2E,
    UserActionTimeout = 0x2F,
    NotAllowed = 0x30,
    PinInvalid = 0x31,
    PinBlocked = 0x32,
    PinAuthInvalid = 0x33,
    PinAuthBlocked = 0x34,
    PinNotSet = 0x35,
    PinRequired = 0x36,
    PinPolicyViolation = 0x37,
    PinTokenExpired = 0x38,
    RequestTooLarge = 0x39,
    ActionTimeout = 0x3A,
    UpRequired = 0x3B,
    UvBlocked = 0x3C,
    Other = 0x7F,
    SpecLast = 0xDF,
    ExtensionFirst = 0xE0,
    ExtensionLast = 0xEF,
    VendorFirst = 0xF0,
    VendorLast = 0xFF,
}

/// CTAP2 request
#[cfg(feature = "usb-fido2")]
pub struct Ctap2Request {
    pub command: Ctap2Command,
    pub cbor_params: Option<CborValue>,
}

#[cfg(feature = "usb-fido2")]
impl Ctap2Request {
    /// Parse CTAP2 request
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.is_empty() {
            bail!("Empty CTAP2 request");
        }

        let command = Ctap2Command::from_u8(data[0]).context("Invalid CTAP2 command")?;

        let cbor_params = if data.len() > 1 {
            Some(serde_cbor::from_slice(&data[1..])?)
        } else {
            None
        };

        Ok(Self {
            command,
            cbor_params,
        })
    }
}

/// CTAP2 response
#[cfg(feature = "usb-fido2")]
pub struct Ctap2Response {
    pub status: Ctap2Status,
    pub cbor_data: Option<CborValue>,
}

#[cfg(feature = "usb-fido2")]
impl Ctap2Response {
    pub fn success(cbor_data: CborValue) -> Self {
        Self {
            status: Ctap2Status::Success,
            cbor_data: Some(cbor_data),
        }
    }

    pub fn error(status: Ctap2Status) -> Self {
        Self {
            status,
            cbor_data: None,
        }
    }

    /// Encode response to bytes
    pub fn to_bytes(self) -> Vec<u8> {
        let mut response = vec![self.status as u8];

        if let Some(cbor) = self.cbor_data {
            if let Ok(encoded) = serde_cbor::to_vec(&cbor) {
                response.extend_from_slice(&encoded);
            }
        }

        response
    }
}

/// CTAP2 protocol handler
#[cfg(feature = "usb-fido2")]
pub struct Ctap2Handler {
    /// AAGUID (Authenticator Attestation GUID) - 16 bytes
    aaguid: [u8; 16],
}

#[cfg(feature = "usb-fido2")]
impl Ctap2Handler {
    pub fn new() -> Self {
        // Generate a simple AAGUID (in production, this should be persistent and unique)
        let aaguid = [
            0x4e, 0x65, 0x74, 0x47,  // "NetG"
            0x65, 0x74, 0x2d, 0x46,  // "et-F"
            0x49, 0x44, 0x4f, 0x32,  // "IDO2"
            0x00, 0x00, 0x00, 0x01,  // version
        ];

        Self { aaguid }
    }

    /// Process CTAP2 command
    pub fn process_command(&mut self, data: &[u8]) -> Vec<u8> {
        let request = match Ctap2Request::parse(data) {
            Ok(req) => req,
            Err(e) => {
                warn!("Failed to parse CTAP2 request: {}", e);
                return Ctap2Response::error(Ctap2Status::InvalidCbor).to_bytes();
            }
        };

        debug!("CTAP2 command: {:?}", request.command);

        let response = match request.command {
            Ctap2Command::GetInfo => self.handle_get_info(),
            Ctap2Command::MakeCredential => self.handle_make_credential(request.cbor_params),
            Ctap2Command::GetAssertion => self.handle_get_assertion(request.cbor_params),
            Ctap2Command::Reset => self.handle_reset(),
            _ => {
                warn!("Unsupported CTAP2 command: {:?}", request.command);
                Ctap2Response::error(Ctap2Status::InvalidCommand)
            }
        };

        response.to_bytes()
    }

    /// Handle authenticatorGetInfo command
    fn handle_get_info(&self) -> Ctap2Response {
        debug!("CTAP2 GetInfo");

        let mut info = BTreeMap::new();

        // 0x01: versions (array of strings)
        let versions = CborValue::Array(vec![
            CborValue::Text("U2F_V2".to_string()),
            CborValue::Text("FIDO_2_0".to_string()),
        ]);
        info.insert(CborValue::Integer(0x01), versions);

        // 0x02: extensions (array of strings) - optional
        // info.insert(CborValue::Integer(0x02), CborValue::Array(vec![]));

        // 0x03: aaguid (16 bytes)
        info.insert(
            CborValue::Integer(0x03),
            CborValue::Bytes(self.aaguid.to_vec()),
        );

        // 0x04: options (map of option_id => bool)
        let mut options = BTreeMap::new();
        options.insert(CborValue::Text("rk".to_string()), CborValue::Bool(true));  // Resident key support
        options.insert(CborValue::Text("up".to_string()), CborValue::Bool(true));  // User presence
        options.insert(CborValue::Text("plat".to_string()), CborValue::Bool(false)); // Not a platform authenticator
        info.insert(CborValue::Integer(0x04), CborValue::Map(options));

        // 0x05: maxMsgSize (integer) - optional
        info.insert(CborValue::Integer(0x05), CborValue::Integer(1200));

        // 0x06: pinProtocols (array of integers) - optional
        // info.insert(CborValue::Integer(0x06), CborValue::Array(vec![CborValue::Integer(1)]));

        Ctap2Response::success(CborValue::Map(info))
    }

    /// Handle authenticatorMakeCredential command
    fn handle_make_credential(&mut self, params: Option<CborValue>) -> Ctap2Response {
        debug!("CTAP2 MakeCredential");

        // For a full implementation, we would:
        // 1. Parse CBOR parameters (clientDataHash, rp, user, pubKeyCredParams, etc.)
        // 2. Generate new key pair
        // 3. Create credential ID
        // 4. Store credential
        // 5. Generate attestation statement
        // 6. Return CBOR response

        // For now, return NotAllowed (needs user interaction)
        Ctap2Response::error(Ctap2Status::NotAllowed)
    }

    /// Handle authenticatorGetAssertion command
    fn handle_get_assertion(&mut self, params: Option<CborValue>) -> Ctap2Response {
        debug!("CTAP2 GetAssertion");

        // For a full implementation, we would:
        // 1. Parse CBOR parameters (rpId, clientDataHash, allowList, etc.)
        // 2. Find matching credentials
        // 3. Generate assertion (signature)
        // 4. Return CBOR response

        // For now, return NoCredentials
        Ctap2Response::error(Ctap2Status::NoCredentials)
    }

    /// Handle authenticatorReset command
    fn handle_reset(&mut self) -> Ctap2Response {
        debug!("CTAP2 Reset");

        // For a full implementation, we would clear all credentials
        // For now, just return success
        let response = BTreeMap::new();
        Ctap2Response::success(CborValue::Map(response))
    }
}

#[cfg(feature = "usb-fido2")]
impl Default for Ctap2Handler {
    fn default() -> Self {
        Self::new()
    }
}
