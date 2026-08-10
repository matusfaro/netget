//! Smart Card cryptographic operations
//!
//! This module provides RSA key generation and signature operations for smart cards.

#[cfg(feature = "usb-smartcard")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-smartcard")]
use rsa::{
    pkcs1v15::{SigningKey, VerifyingKey},
    signature::{RandomizedSigner, SignatureEncoding, Verifier},
    RsaPrivateKey, RsaPublicKey,
};
#[cfg(feature = "usb-smartcard")]
use sha2::Sha256;
#[cfg(feature = "usb-smartcard")]
use tracing::{debug, info};

/// Smart card key pair
#[cfg(feature = "usb-smartcard")]
pub struct SmartCardKeyPair {
    /// Private key
    private_key: RsaPrivateKey,
    /// Public key
    public_key: RsaPublicKey,
    /// Key reference (0x00-0xFF)
    key_ref: u8,
}

#[cfg(feature = "usb-smartcard")]
impl SmartCardKeyPair {
    /// Generate a new RSA key pair
    pub fn generate(key_ref: u8, bits: usize) -> Result<Self> {
        let mut rng = rand::thread_rng();
        let private_key =
            RsaPrivateKey::new(&mut rng, bits).context("Failed to generate RSA key")?;
        let public_key = RsaPublicKey::from(&private_key);

        info!("Generated RSA-{} key pair (ref={:#04x})", bits, key_ref);

        Ok(Self {
            private_key,
            public_key,
            key_ref,
        })
    }

    /// Sign data with the private key
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::<Sha256>::new(self.private_key.clone());
        let signature = signing_key.sign_with_rng(&mut rng, data);

        debug!("Signed {} bytes with key {:#04x}", data.len(), self.key_ref);

        Ok(signature.to_vec())
    }

    /// Verify signature with the public key
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<bool> {
        let verifying_key = VerifyingKey::<Sha256>::new(self.public_key.clone());

        let sig = match rsa::pkcs1v15::Signature::try_from(signature) {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };

        match verifying_key.verify(data, &sig) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Get public key modulus (for PIV)
    pub fn public_key_modulus(&self) -> Vec<u8> {
        use rsa::traits::PublicKeyParts;
        self.public_key.n().to_bytes_be()
    }

    /// Get public key exponent (for PIV)
    pub fn public_key_exponent(&self) -> Vec<u8> {
        use rsa::traits::PublicKeyParts;
        self.public_key.e().to_bytes_be()
    }

    /// Get key reference
    pub fn key_ref(&self) -> u8 {
        self.key_ref
    }
}

/// Smart card key store
#[cfg(feature = "usb-smartcard")]
pub struct SmartCardKeyStore {
    /// Stored key pairs
    keys: std::collections::HashMap<u8, SmartCardKeyPair>,
}

#[cfg(feature = "usb-smartcard")]
impl SmartCardKeyStore {
    pub fn new() -> Self {
        Self {
            keys: std::collections::HashMap::new(),
        }
    }

    /// Generate and store a new key pair
    pub fn generate_key(&mut self, key_ref: u8, bits: usize) -> Result<()> {
        let key_pair = SmartCardKeyPair::generate(key_ref, bits)?;
        self.keys.insert(key_ref, key_pair);
        Ok(())
    }

    /// Get a key pair by reference
    pub fn get_key(&self, key_ref: u8) -> Option<&SmartCardKeyPair> {
        self.keys.get(&key_ref)
    }

    /// Delete a key pair
    pub fn delete_key(&mut self, key_ref: u8) -> bool {
        self.keys.remove(&key_ref).is_some()
    }

    /// List all key references
    pub fn list_keys(&self) -> Vec<u8> {
        self.keys.keys().copied().collect()
    }
}

#[cfg(feature = "usb-smartcard")]
impl Default for SmartCardKeyStore {
    fn default() -> Self {
        Self::new()
    }
}
