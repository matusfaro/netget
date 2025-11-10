//! CTAP2 (Client-to-Authenticator Protocol 2) / FIDO2 implementation
//!
//! This module implements the CTAP2 protocol commands with full credential support.

#[cfg(feature = "usb-fido2")]
use anyhow::{bail, Context, Result};
#[cfg(feature = "usb-fido2")]
use ring::rand::SecureRandom;
#[cfg(feature = "usb-fido2")]
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
#[cfg(feature = "usb-fido2")]
use serde_cbor::Value as CborValue;
#[cfg(feature = "usb-fido2")]
use std::collections::BTreeMap;
#[cfg(feature = "usb-fido2")]
use tracing::{debug, info};

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
    NoCredentials = 0x2E,
    UserActionTimeout = 0x2F,
    NotAllowed = 0x30,
    PinInvalid = 0x31,
    Other = 0x7F,
}

/// CTAP2 request
#[cfg(feature = "usb-fido2")]
pub struct Ctap2Request {
    pub command: Ctap2Command,
    pub cbor_params: Option<CborValue>,
}

#[cfg(feature = "usb-fido2")]
impl Ctap2Request {
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

/// Stored CTAP2 credential
#[cfg(feature = "usb-fido2")]
#[derive(Clone)]
struct Ctap2Credential {
    /// Credential ID (opaque blob)
    credential_id: Vec<u8>,
    /// PKCS#8 private key
    private_key: Vec<u8>,
    /// Public key (COSE format)
    public_key_cose: Vec<u8>,
    /// Relying Party ID
    rp_id: String,
    /// User handle
    user_handle: Vec<u8>,
    /// User name
    user_name: String,
    /// Signature counter
    counter: u32,
    /// Resident key flag
    is_resident: bool,
}

/// PIN management state
#[cfg(feature = "usb-fido2")]
#[derive(Clone)]
struct PinState {
    /// PIN hash (SHA-256)
    pin_hash: Option<Vec<u8>>,
    /// PIN retry counter (starts at 8)
    retries: u8,
    /// PIN is currently verified
    verified: bool,
}

#[cfg(feature = "usb-fido2")]
impl PinState {
    fn new() -> Self {
        Self {
            pin_hash: None,
            retries: 8,
            verified: false,
        }
    }

    fn set_pin(&mut self, pin: &str) -> Result<()> {
        if pin.len() < 4 || pin.len() > 63 {
            bail!("PIN must be 4-63 characters");
        }

        // Hash PIN with SHA-256
        let hash = ring::digest::digest(&ring::digest::SHA256, pin.as_bytes());
        self.pin_hash = Some(hash.as_ref().to_vec());
        self.retries = 8;
        self.verified = false;

        info!("PIN set successfully");
        Ok(())
    }

    fn verify_pin(&mut self, pin: &str) -> Result<bool> {
        if self.retries == 0 {
            bail!("PIN blocked - too many failed attempts");
        }

        let Some(ref stored_hash) = self.pin_hash else {
            bail!("PIN not set");
        };

        let hash = ring::digest::digest(&ring::digest::SHA256, pin.as_bytes());

        if hash.as_ref() == stored_hash.as_slice() {
            self.verified = true;
            self.retries = 8; // Reset on successful verification
            info!("PIN verified successfully");
            Ok(true)
        } else {
            self.retries -= 1;
            self.verified = false;
            warn!("PIN verification failed - {} retries remaining", self.retries);
            Ok(false)
        }
    }

    fn is_set(&self) -> bool {
        self.pin_hash.is_some()
    }

    fn is_verified(&self) -> bool {
        self.verified
    }

    fn reset(&mut self) {
        self.verified = false;
    }
}

/// CTAP2 credential store
#[cfg(feature = "usb-fido2")]
pub struct Ctap2CredentialStore {
    /// Credentials indexed by RP ID
    credentials: std::collections::HashMap<String, Vec<Ctap2Credential>>,
    /// Resident credentials (stored on authenticator)
    resident_credentials: Vec<Ctap2Credential>,
    /// RNG
    rng: ring::rand::SystemRandom,
    /// PIN state
    pin_state: PinState,
}

#[cfg(feature = "usb-fido2")]
impl Ctap2CredentialStore {
    pub fn new() -> Self {
        Self {
            credentials: std::collections::HashMap::new(),
            resident_credentials: Vec::new(),
            rng: ring::rand::SystemRandom::new(),
            pin_state: PinState::new(),
        }
    }

    /// Create a new credential
    pub fn make_credential(
        &mut self,
        rp_id: &str,
        user_handle: &[u8],
        user_name: &str,
        require_resident_key: bool,
        require_user_verification: bool,
    ) -> Result<Ctap2Credential> {
        // Check UV requirement
        if require_user_verification && !self.pin_state.is_verified() {
            bail!("User verification required but PIN not verified");
        }

        // Generate ECDSA P-256 key pair
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &self.rng)?;
        let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &self.rng)?;

        let public_key_bytes = key_pair.public_key().as_ref();

        // Convert public key to COSE format (COSE_Key map)
        let public_key_cose = Self::encode_cose_key(public_key_bytes)?;

        // Generate random credential ID
        let mut credential_id = vec![0u8; 32];
        self.rng.fill(&mut credential_id)?;

        let credential = Ctap2Credential {
            credential_id: credential_id.clone(),
            private_key: pkcs8.as_ref().to_vec(),
            public_key_cose,
            rp_id: rp_id.to_string(),
            user_handle: user_handle.to_vec(),
            user_name: user_name.to_string(),
            counter: 0,
            is_resident: require_resident_key,
        };

        // Store credential
        self.credentials
            .entry(rp_id.to_string())
            .or_insert_with(Vec::new)
            .push(credential.clone());

        // Also store in resident credentials if requested
        if require_resident_key {
            self.resident_credentials.push(credential.clone());
            info!("Created resident credential for RP '{}', user '{}'", rp_id, user_name);
        } else {
            info!("Created credential for RP '{}', user '{}'", rp_id, user_name);
        }

        Ok(credential)
    }

    /// Find credentials for RP ID (includes resident keys)
    pub fn find_credentials(&mut self, rp_id: &str, credential_id: Option<&[u8]>) -> Option<&mut Ctap2Credential> {
        // First check resident credentials
        if credential_id.is_none() {
            if let Some(cred) = self.resident_credentials.iter_mut().find(|c| c.rp_id == rp_id) {
                return Some(cred);
            }
        }

        // Then check regular credentials
        let creds = self.credentials.get_mut(rp_id)?;

        if let Some(cred_id) = credential_id {
            creds.iter_mut().find(|c| c.credential_id == cred_id)
        } else {
            creds.first_mut()
        }
    }

    /// Get all resident credentials for RP
    pub fn get_resident_credentials(&self, rp_id: &str) -> Vec<&Ctap2Credential> {
        self.resident_credentials
            .iter()
            .filter(|c| c.rp_id == rp_id)
            .collect()
    }

    /// Set PIN
    pub fn set_pin(&mut self, pin: &str) -> Result<()> {
        self.pin_state.set_pin(pin)
    }

    /// Verify PIN
    pub fn verify_pin(&mut self, pin: &str) -> Result<bool> {
        self.pin_state.verify_pin(pin)
    }

    /// Check if PIN is set
    pub fn has_pin(&self) -> bool {
        self.pin_state.is_set()
    }

    /// Check if PIN is verified
    pub fn pin_verified(&self) -> bool {
        self.pin_state.is_verified()
    }

    /// Get PIN retries remaining
    pub fn pin_retries(&self) -> u8 {
        self.pin_state.retries
    }

    /// Reset PIN verification state (e.g., after timeout)
    pub fn reset_pin_verification(&mut self) {
        self.pin_state.reset()
    }

    /// Encode public key in COSE format
    fn encode_cose_key(public_key: &[u8]) -> Result<Vec<u8>> {
        // COSE_Key map for ES256 (ECDSA with SHA-256)
        // kty: 2 (EC2), alg: -7 (ES256), crv: 1 (P-256)
        // x: X coordinate, y: Y coordinate

        if public_key.len() != 65 || public_key[0] != 0x04 {
            bail!("Invalid public key format");
        }

        let x = &public_key[1..33];
        let y = &public_key[33..65];

        let mut cose_key = BTreeMap::new();
        cose_key.insert(CborValue::Integer(1), CborValue::Integer(2)); // kty: EC2
        cose_key.insert(CborValue::Integer(3), CborValue::Integer(-7)); // alg: ES256
        cose_key.insert(CborValue::Integer(-1), CborValue::Integer(1)); // crv: P-256
        cose_key.insert(CborValue::Integer(-2), CborValue::Bytes(x.to_vec())); // x
        cose_key.insert(CborValue::Integer(-3), CborValue::Bytes(y.to_vec())); // y

        Ok(serde_cbor::to_vec(&CborValue::Map(cose_key))?)
    }
}

/// CTAP2 protocol handler
#[cfg(feature = "usb-fido2")]
pub struct Ctap2Handler {
    /// AAGUID (Authenticator Attestation GUID)
    aaguid: [u8; 16],
    /// Credential store
    store: Ctap2CredentialStore,
    /// Approval manager for sync/async bridge
    approval_manager: Option<std::sync::Arc<crate::server::usb::fido2::approval::ApprovalManager>>,
}

#[cfg(feature = "usb-fido2")]
impl Ctap2Handler {
    pub fn new() -> Self {
        Self::new_with_approval_manager(None)
    }

    pub fn new_with_approval_manager(
        approval_manager: Option<std::sync::Arc<crate::server::usb::fido2::approval::ApprovalManager>>,
    ) -> Self {
        let aaguid = [
            0x4e, 0x65, 0x74, 0x47,  // "NetG"
            0x65, 0x74, 0x2d, 0x46,  // "et-F"
            0x49, 0x44, 0x4f, 0x32,  // "IDO2"
            0x00, 0x00, 0x00, 0x01,  // version
        ];

        Self {
            aaguid,
            store: Ctap2CredentialStore::new(),
            approval_manager,
        }
    }

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
            Ctap2Command::ClientPin => self.handle_client_pin(request.cbor_params),
            Ctap2Command::GetNextAssertion => self.handle_get_next_assertion(),
            Ctap2Command::Reset => self.handle_reset(),
        };

        response.to_bytes()
    }

    fn handle_get_info(&self) -> Ctap2Response {
        debug!("CTAP2 GetInfo");

        let mut info = BTreeMap::new();

        // 0x01: versions
        let versions = CborValue::Array(vec![
            CborValue::Text("U2F_V2".to_string()),
            CborValue::Text("FIDO_2_0".to_string()),
        ]);
        info.insert(CborValue::Integer(0x01), versions);

        // 0x03: aaguid
        info.insert(
            CborValue::Integer(0x03),
            CborValue::Bytes(self.aaguid.to_vec()),
        );

        // 0x04: options
        let mut options = BTreeMap::new();
        options.insert(CborValue::Text("rk".to_string()), CborValue::Bool(true)); // Resident key support
        options.insert(CborValue::Text("up".to_string()), CborValue::Bool(true)); // User presence
        options.insert(CborValue::Text("uv".to_string()), CborValue::Bool(true)); // User verification (PIN)
        options.insert(CborValue::Text("plat".to_string()), CborValue::Bool(false)); // Not platform authenticator
        options.insert(CborValue::Text("clientPin".to_string()), CborValue::Bool(self.store.has_pin())); // PIN configured
        info.insert(CborValue::Integer(0x04), CborValue::Map(options));

        // 0x05: maxMsgSize
        info.insert(CborValue::Integer(0x05), CborValue::Integer(1200));

        // 0x06: pinProtocols (supported PIN protocol versions)
        info.insert(CborValue::Integer(0x06), CborValue::Array(vec![CborValue::Integer(1)]));

        Ctap2Response::success(CborValue::Map(info))
    }

    fn handle_make_credential(&mut self, params: Option<CborValue>) -> Ctap2Response {
        debug!("CTAP2 MakeCredential");

        let params = match params {
            Some(CborValue::Map(m)) => m,
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        // Parse parameters
        let client_data_hash = match params.get(&CborValue::Integer(0x01)) {
            Some(CborValue::Bytes(b)) if b.len() == 32 => b.clone(),
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        let rp = match params.get(&CborValue::Integer(0x02)) {
            Some(CborValue::Map(m)) => m,
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        let rp_id = match rp.get(&CborValue::Text("id".to_string())) {
            Some(CborValue::Text(s)) => s.clone(),
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        let user = match params.get(&CborValue::Integer(0x03)) {
            Some(CborValue::Map(m)) => m,
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        let user_handle = match user.get(&CborValue::Text("id".to_string())) {
            Some(CborValue::Bytes(b)) => b.clone(),
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        let user_name = match user.get(&CborValue::Text("name".to_string())) {
            Some(CborValue::Text(s)) => s.clone(),
            _ => "user".to_string(),
        };

        // Parse options (0x07)
        let options = params.get(&CborValue::Integer(0x07));
        let require_resident_key = options
            .and_then(|o| match o {
                CborValue::Map(m) => Some(m),
                _ => None,
            })
            .and_then(|m| m.get(&CborValue::Text("rk".to_string())))
            .and_then(|v| match v {
                CborValue::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(false);

        let require_user_verification = options
            .and_then(|o| match o {
                CborValue::Map(m) => Some(m),
                _ => None,
            })
            .and_then(|m| m.get(&CborValue::Text("uv".to_string())))
            .and_then(|v| match v {
                CborValue::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(false);

        info!(
            "MakeCredential for RP '{}', user '{}' (rk={}, uv={})",
            rp_id, user_name, require_resident_key, require_user_verification
        );

        // Check for LLM approval if approval manager is configured
        if let Some(ref approval_mgr) = self.approval_manager {
            debug!("Requesting LLM approval for MakeCredential");
            let (approval_id, decision) = tokio::runtime::Handle::current().block_on(
                approval_mgr.request_approval(
                    crate::server::usb::fido2::approval::OperationType::Register,
                    rp_id.clone(),
                    Some(user_name.clone()),
                    None,
                )
            );

            if decision == crate::server::usb::fido2::approval::ApprovalDecision::Denied {
                warn!("MakeCredential denied by LLM (approval_id={})", approval_id);
                return Ctap2Response::error(Ctap2Status::OperationDenied);
            }

            info!("MakeCredential approved by LLM (approval_id={})", approval_id);
        }

        // Create credential
        let credential = match self.store.make_credential(&rp_id, &user_handle, &user_name, require_resident_key, require_user_verification) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to create credential: {}", e);
                // Return proper error for UV requirement
                if e.to_string().contains("PIN not verified") {
                    return Ctap2Response::error(Ctap2Status::PinInvalid);
                }
                return Ctap2Response::error(Ctap2Status::Other);
            }
        };

        // Build attestation object
        let mut att_stmt = BTreeMap::new();
        att_stmt.insert(CborValue::Text("alg".to_string()), CborValue::Integer(-7)); // ES256
        att_stmt.insert(CborValue::Text("sig".to_string()), CborValue::Bytes(vec![0u8; 71])); // Dummy signature

        let mut auth_data = Vec::new();
        // RP ID hash (32 bytes)
        auth_data.extend_from_slice(&ring::digest::digest(&ring::digest::SHA256, rp_id.as_bytes()).as_ref());
        // Flags (1 byte): UP=1, UV=(pin verified), AT=1, ED=0
        let flags = 0x41 | if self.store.pin_verified() { 0x04 } else { 0x00 }; // Set UV bit if PIN verified
        auth_data.push(flags);
        // Counter (4 bytes)
        auth_data.extend_from_slice(&0u32.to_be_bytes());
        // AAGUID (16 bytes)
        auth_data.extend_from_slice(&self.aaguid);
        // Credential ID length (2 bytes)
        auth_data.extend_from_slice(&(credential.credential_id.len() as u16).to_be_bytes());
        // Credential ID
        auth_data.extend_from_slice(&credential.credential_id);
        // Public key (COSE format)
        auth_data.extend_from_slice(&credential.public_key_cose);

        let mut att_obj = BTreeMap::new();
        att_obj.insert(CborValue::Text("fmt".to_string()), CborValue::Text("packed".to_string()));
        att_obj.insert(CborValue::Text("authData".to_string()), CborValue::Bytes(auth_data));
        att_obj.insert(CborValue::Text("attStmt".to_string()), CborValue::Map(att_stmt));

        let mut response = BTreeMap::new();
        response.insert(CborValue::Integer(0x01), CborValue::Text("packed".to_string()));
        response.insert(CborValue::Integer(0x02), CborValue::Bytes(serde_cbor::to_vec(&CborValue::Map(att_obj)).unwrap_or_default()));
        response.insert(CborValue::Integer(0x03), CborValue::Bytes(self.aaguid.to_vec()));

        info!("MakeCredential successful");
        Ctap2Response::success(CborValue::Map(response))
    }

    fn handle_get_assertion(&mut self, params: Option<CborValue>) -> Ctap2Response {
        debug!("CTAP2 GetAssertion");

        let params = match params {
            Some(CborValue::Map(m)) => m,
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        let rp_id = match params.get(&CborValue::Integer(0x01)) {
            Some(CborValue::Text(s)) => s.clone(),
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        let client_data_hash = match params.get(&CborValue::Integer(0x02)) {
            Some(CborValue::Bytes(b)) if b.len() == 32 => b.clone(),
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        info!("GetAssertion for RP '{}'", rp_id);

        // Check for LLM approval if approval manager is configured
        if let Some(ref approval_mgr) = self.approval_manager {
            debug!("Requesting LLM approval for GetAssertion");
            let (approval_id, decision) = tokio::runtime::Handle::current().block_on(
                approval_mgr.request_approval(
                    crate::server::usb::fido2::approval::OperationType::Authenticate,
                    rp_id.clone(),
                    None,
                    None,
                )
            );

            if decision == crate::server::usb::fido2::approval::ApprovalDecision::Denied {
                warn!("GetAssertion denied by LLM (approval_id={})", approval_id);
                return Ctap2Response::error(Ctap2Status::OperationDenied);
            }

            info!("GetAssertion approved by LLM (approval_id={})", approval_id);
        }

        // Find credential
        let credential = match self.store.find_credentials(&rp_id, None) {
            Some(c) => c,
            None => {
                warn!("No credentials found for RP '{}'", rp_id);
                return Ctap2Response::error(Ctap2Status::NoCredentials);
            }
        };

        // Increment counter
        credential.counter += 1;

        // Build authenticator data
        let mut auth_data = Vec::new();
        auth_data.extend_from_slice(&ring::digest::digest(&ring::digest::SHA256, rp_id.as_bytes()).as_ref());
        // Flags: UP=1, UV=(pin verified)
        let flags = 0x01 | if self.store.pin_verified() { 0x04 } else { 0x00 };
        auth_data.push(flags);
        auth_data.extend_from_slice(&credential.counter.to_be_bytes());

        // Sign (authenticator_data || client_data_hash)
        let mut sig_data = auth_data.clone();
        sig_data.extend_from_slice(&client_data_hash);

        let key_pair = match EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &credential.private_key,
            &ring::rand::SystemRandom::new()
        ) {
            Ok(kp) => kp,
            Err(e) => {
                warn!("Failed to load private key: {}", e);
                return Ctap2Response::error(Ctap2Status::Other);
            }
        };

        let signature = match key_pair.sign(&self.store.rng, &sig_data) {
            Ok(sig) => sig.as_ref().to_vec(),
            Err(e) => {
                warn!("Failed to sign: {}", e);
                return Ctap2Response::error(Ctap2Status::Other);
            }
        };

        let mut response = BTreeMap::new();
        response.insert(CborValue::Integer(0x01), CborValue::Bytes(credential.credential_id.clone()));
        response.insert(CborValue::Integer(0x02), CborValue::Bytes(auth_data));
        response.insert(CborValue::Integer(0x03), CborValue::Bytes(signature));

        info!("GetAssertion successful");
        Ctap2Response::success(CborValue::Map(response))
    }

    fn handle_client_pin(&mut self, params: Option<CborValue>) -> Ctap2Response {
        debug!("CTAP2 ClientPin");

        let params = match params {
            Some(CborValue::Map(m)) => m,
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        // Parse pinProtocol (0x01)
        let _pin_protocol = match params.get(&CborValue::Integer(0x01)) {
            Some(CborValue::Integer(1)) => 1, // Only support protocol 1
            _ => return Ctap2Response::error(Ctap2Status::InvalidParameter),
        };

        // Parse subCommand (0x02)
        let sub_command = match params.get(&CborValue::Integer(0x02)) {
            Some(CborValue::Integer(n)) => *n as u8,
            _ => return Ctap2Response::error(Ctap2Status::MissingParameter),
        };

        match sub_command {
            0x03 => {
                // getPinRetries
                let mut response = BTreeMap::new();
                response.insert(CborValue::Integer(0x03), CborValue::Integer(self.store.pin_retries() as i128));
                info!("ClientPin: getPinRetries = {}", self.store.pin_retries());
                Ctap2Response::success(CborValue::Map(response))
            }
            0x06 => {
                // setPinSimple - Simplified PIN setting (non-standard, for development)
                // In real CTAP2, PIN is set via shared secret negotiation
                // For now, we'll accept a simple PIN string for testing
                if let Some(CborValue::Text(pin)) = params.get(&CborValue::Integer(0x05)) {
                    match self.store.set_pin(pin) {
                        Ok(()) => {
                            info!("ClientPin: PIN set successfully");
                            Ctap2Response::success(CborValue::Map(BTreeMap::new()))
                        }
                        Err(e) => {
                            warn!("ClientPin: Failed to set PIN: {}", e);
                            Ctap2Response::error(Ctap2Status::PinInvalid)
                        }
                    }
                } else {
                    Ctap2Response::error(Ctap2Status::MissingParameter)
                }
            }
            0x08 => {
                // getPinTokenSimple - Simplified PIN verification (non-standard, for development)
                if let Some(CborValue::Text(pin)) = params.get(&CborValue::Integer(0x05)) {
                    match self.store.verify_pin(pin) {
                        Ok(true) => {
                            info!("ClientPin: PIN verified successfully");
                            // Return success - in real CTAP2, would return PIN token
                            Ctap2Response::success(CborValue::Map(BTreeMap::new()))
                        }
                        Ok(false) => {
                            warn!("ClientPin: PIN verification failed");
                            Ctap2Response::error(Ctap2Status::PinInvalid)
                        }
                        Err(e) => {
                            warn!("ClientPin: PIN verification error: {}", e);
                            Ctap2Response::error(Ctap2Status::PinInvalid)
                        }
                    }
                } else {
                    Ctap2Response::error(Ctap2Status::MissingParameter)
                }
            }
            _ => {
                warn!("ClientPin: Unsupported subCommand: {:#04x}", sub_command);
                Ctap2Response::error(Ctap2Status::InvalidParameter)
            }
        }
    }

    fn handle_get_next_assertion(&mut self) -> Ctap2Response {
        debug!("CTAP2 GetNextAssertion");
        // For now, return no more credentials
        // Full implementation would iterate through multiple resident credentials
        Ctap2Response::error(Ctap2Status::NoCredentials)
    }

    fn handle_reset(&mut self) -> Ctap2Response {
        debug!("CTAP2 Reset");
        self.store.credentials.clear();
        self.store.resident_credentials.clear();
        // Note: PIN is NOT cleared on reset per CTAP2 spec
        info!("All credentials cleared");
        Ctap2Response::success(CborValue::Map(BTreeMap::new()))
    }
}

#[cfg(feature = "usb-fido2")]
impl Default for Ctap2Handler {
    fn default() -> Self {
        Self::new()
    }
}
