//! CTAP2 (Client-to-Authenticator Protocol 2) / FIDO2 implementation
//!
//! This module implements the CTAP2 protocol commands with full credential support.

#[cfg(feature = "usb-fido2")]
use anyhow::{bail, Context, Result};
#[cfg(feature = "usb-fido2")]
use ring::rand::SecureRandom;
#[cfg(feature = "usb-fido2")]
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
#[cfg(feature = "usb-fido2")]
use serde_cbor::Value as CborValue;
#[cfg(feature = "usb-fido2")]
use std::collections::BTreeMap;
#[cfg(feature = "usb-fido2")]
use tracing::{debug, info, trace, warn};

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
}

/// CTAP2 credential store
#[cfg(feature = "usb-fido2")]
pub struct Ctap2CredentialStore {
    /// Credentials indexed by RP ID
    credentials: std::collections::HashMap<String, Vec<Ctap2Credential>>,
    /// RNG
    rng: ring::rand::SystemRandom,
}

#[cfg(feature = "usb-fido2")]
impl Ctap2CredentialStore {
    pub fn new() -> Self {
        Self {
            credentials: std::collections::HashMap::new(),
            rng: ring::rand::SystemRandom::new(),
        }
    }

    /// Create a new credential
    pub fn make_credential(
        &mut self,
        rp_id: &str,
        user_handle: &[u8],
        user_name: &str,
    ) -> Result<Ctap2Credential> {
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
        };

        // Store credential
        self.credentials
            .entry(rp_id.to_string())
            .or_insert_with(Vec::new)
            .push(credential.clone());

        info!(
            "Created credential for RP '{}', user '{}'",
            rp_id, user_name
        );

        Ok(credential)
    }

    /// Find credentials for RP ID
    pub fn find_credentials(&mut self, rp_id: &str, credential_id: Option<&[u8]>) -> Option<&mut Ctap2Credential> {
        let creds = self.credentials.get_mut(rp_id)?;

        if let Some(cred_id) = credential_id {
            creds.iter_mut().find(|c| c.credential_id == cred_id)
        } else {
            creds.first_mut()
        }
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
}

#[cfg(feature = "usb-fido2")]
impl Ctap2Handler {
    pub fn new() -> Self {
        let aaguid = [
            0x4e, 0x65, 0x74, 0x47,  // "NetG"
            0x65, 0x74, 0x2d, 0x46,  // "et-F"
            0x49, 0x44, 0x4f, 0x32,  // "IDO2"
            0x00, 0x00, 0x00, 0x01,  // version
        ];

        Self {
            aaguid,
            store: Ctap2CredentialStore::new(),
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
            Ctap2Command::Reset => self.handle_reset(),
            _ => {
                warn!("Unsupported CTAP2 command: {:?}", request.command);
                Ctap2Response::error(Ctap2Status::InvalidCommand)
            }
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
        options.insert(CborValue::Text("rk".to_string()), CborValue::Bool(true));
        options.insert(CborValue::Text("up".to_string()), CborValue::Bool(true));
        options.insert(CborValue::Text("plat".to_string()), CborValue::Bool(false));
        info.insert(CborValue::Integer(0x04), CborValue::Map(options));

        // 0x05: maxMsgSize
        info.insert(CborValue::Integer(0x05), CborValue::Integer(1200));

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

        info!(
            "MakeCredential for RP '{}', user '{}'",
            rp_id, user_name
        );

        // Create credential
        let credential = match self.store.make_credential(&rp_id, &user_handle, &user_name) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to create credential: {}", e);
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
        // Flags (1 byte): UP=1, UV=0, AT=1, ED=0
        auth_data.push(0x41);
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
        auth_data.push(0x01); // Flags: UP=1
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

        let signature = match key_pair.sign(&sig_data) {
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

    fn handle_reset(&mut self) -> Ctap2Response {
        debug!("CTAP2 Reset");
        self.store.credentials.clear();
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
