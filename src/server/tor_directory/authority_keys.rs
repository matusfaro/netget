//! Tor Directory Authority Key Management
//!
//! This module handles generation and management of cryptographic keys for a Tor directory authority.
//! Implements the key types required by the Tor directory protocol:
//!
//! - **Authority Identity Key** (long-term): Ed25519 key that identifies the directory authority
//! - **Authority Signing Key** (medium-term): Ed25519 key used to sign consensus documents
//!
//! The identity key should be kept secure and rarely rotated, while the signing key
//! can be rotated monthly for security best practices.

use anyhow::Result;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// Authority keypair consisting of identity key (long-term) and signing key (medium-term)
#[derive(Clone)]
pub struct AuthorityKeys {
    /// Long-term identity key for the authority
    pub identity_key: SigningKey,
    /// Medium-term signing key (rotates monthly)
    pub signing_key: SigningKey,
}

impl AuthorityKeys {
    /// Generate new authority keys with random Ed25519 keypairs
    ///
    /// In production, these would be generated once and stored securely.
    /// For testing, we generate them on each run.
    pub fn generate() -> Result<Self> {
        use rand::rngs::OsRng;

        let mut csprng = OsRng;
        let identity_key = SigningKey::generate(&mut csprng);
        let signing_key = SigningKey::generate(&mut csprng);

        Ok(Self {
            identity_key,
            signing_key,
        })
    }

    /// Get the authority identity public key
    pub fn identity_public_key(&self) -> VerifyingKey {
        self.identity_key.verifying_key()
    }

    /// Get the authority signing public key
    pub fn signing_public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Calculate the v3 identity fingerprint (SHA-256 of identity public key, first 20 bytes)
    ///
    /// This is used in the DirAuthority line in torrc:
    /// `DirAuthority nickname ... v3ident=<HEX_FINGERPRINT>`
    pub fn v3_identity_fingerprint(&self) -> String {
        let binding = self.identity_public_key();
        let pubkey_bytes = binding.as_bytes();
        let mut hasher = Sha256::new();
        hasher.update(pubkey_bytes);
        let hash = hasher.finalize();

        // Take first 20 bytes and convert to hex
        hex::encode(&hash[..20])
    }

    /// Calculate the authority fingerprint (SHA-1 equivalent for compatibility)
    ///
    /// For Tor v3 authorities, this is typically the same as the v3 identity fingerprint.
    /// This is used in the final FINGERPRINT field in DirAuthority line.
    pub fn authority_fingerprint(&self) -> String {
        // For v3 authorities, we use the same as v3_identity_fingerprint
        self.v3_identity_fingerprint()
    }

    /// Sign data with the signing key
    ///
    /// This is used to sign consensus documents.
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    /// Get signing key bytes for consensus signature block
    pub fn signing_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Get identity key bytes for certificate generation
    pub fn identity_key_bytes(&self) -> [u8; 32] {
        self.identity_key.to_bytes()
    }
}
