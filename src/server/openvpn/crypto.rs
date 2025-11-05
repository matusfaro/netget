//! OpenVPN data channel encryption
//!
//! Supports AES-GCM and ChaCha20-Poly1305 for data channel encryption.

use anyhow::{Result, Context};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use chacha20poly1305::{ChaCha20Poly1305, Key as ChaChaKey};

/// Cipher suite for data channel encryption
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherSuite {
    Aes256Gcm,
    ChaCha20Poly1305,
}

/// Data channel cipher for encrypting/decrypting VPN packets
pub struct DataChannelCipher {
    suite: CipherSuite,
    aes_cipher: Option<Aes256Gcm>,
    chacha_cipher: Option<ChaCha20Poly1305>,
}

impl DataChannelCipher {
    /// Create new cipher with AES-256-GCM
    pub fn new_aes256gcm(key: &[u8; 32]) -> Result<Self> {
        let cipher = Aes256Gcm::new_from_slice(key)
            .context("Failed to create AES-256-GCM cipher")?;

        Ok(DataChannelCipher {
            suite: CipherSuite::Aes256Gcm,
            aes_cipher: Some(cipher),
            chacha_cipher: None,
        })
    }

    /// Create new cipher with ChaCha20-Poly1305
    pub fn new_chacha20poly1305(key: &[u8; 32]) -> Result<Self> {
        let chacha_key = ChaChaKey::from_slice(key);
        let cipher = ChaCha20Poly1305::new(chacha_key);

        Ok(DataChannelCipher {
            suite: CipherSuite::ChaCha20Poly1305,
            aes_cipher: None,
            chacha_cipher: Some(cipher),
        })
    }

    /// Encrypt data channel packet
    ///
    /// packet_id is used as the nonce (IV) for the encryption.
    /// In OpenVPN, packet ID serves as replay protection and nonce.
    pub fn encrypt(&self, packet_id: u32, plaintext: &[u8], additional_data: &[u8]) -> Result<Vec<u8>> {
        match self.suite {
            CipherSuite::Aes256Gcm => {
                let cipher = self.aes_cipher.as_ref()
                    .context("AES cipher not initialized")?;

                // Create 12-byte nonce from packet_id
                let mut nonce_bytes = [0u8; 12];
                nonce_bytes[8..12].copy_from_slice(&packet_id.to_be_bytes());
                let nonce = Nonce::from_slice(&nonce_bytes);

                let payload = Payload {
                    msg: plaintext,
                    aad: additional_data,
                };

                cipher
                    .encrypt(nonce, payload)
                    .map_err(|e| anyhow::anyhow!("AES-GCM encryption failed: {}", e))
            }
            CipherSuite::ChaCha20Poly1305 => {
                let cipher = self.chacha_cipher.as_ref()
                    .context("ChaCha20-Poly1305 cipher not initialized")?;

                // Create 12-byte nonce from packet_id
                let mut nonce_bytes = [0u8; 12];
                nonce_bytes[8..12].copy_from_slice(&packet_id.to_be_bytes());
                let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

                let payload = Payload {
                    msg: plaintext,
                    aad: additional_data,
                };

                cipher
                    .encrypt(nonce, payload)
                    .map_err(|e| anyhow::anyhow!("ChaCha20-Poly1305 encryption failed: {}", e))
            }
        }
    }

    /// Decrypt data channel packet
    pub fn decrypt(&self, packet_id: u32, ciphertext: &[u8], additional_data: &[u8]) -> Result<Vec<u8>> {
        match self.suite {
            CipherSuite::Aes256Gcm => {
                let cipher = self.aes_cipher.as_ref()
                    .context("AES cipher not initialized")?;

                // Create 12-byte nonce from packet_id
                let mut nonce_bytes = [0u8; 12];
                nonce_bytes[8..12].copy_from_slice(&packet_id.to_be_bytes());
                let nonce = Nonce::from_slice(&nonce_bytes);

                let payload = Payload {
                    msg: ciphertext,
                    aad: additional_data,
                };

                cipher
                    .decrypt(nonce, payload)
                    .map_err(|e| anyhow::anyhow!("AES-GCM decryption failed: {}", e))
            }
            CipherSuite::ChaCha20Poly1305 => {
                let cipher = self.chacha_cipher.as_ref()
                    .context("ChaCha20-Poly1305 cipher not initialized")?;

                // Create 12-byte nonce from packet_id
                let mut nonce_bytes = [0u8; 12];
                nonce_bytes[8..12].copy_from_slice(&packet_id.to_be_bytes());
                let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

                let payload = Payload {
                    msg: ciphertext,
                    aad: additional_data,
                };

                cipher
                    .decrypt(nonce, payload)
                    .map_err(|e| anyhow::anyhow!("ChaCha20-Poly1305 decryption failed: {}", e))
            }
        }
    }
}

/// Derive OpenVPN data channel keys from TLS master secret
///
/// This implements OpenVPN's key derivation using PRF (Pseudo-Random Function).
/// In a full implementation, this would use the TLS PRF with proper labels.
/// For MVP, we use a simplified HKDF-based approach.
pub fn derive_data_keys(master_secret: &[u8], client_random: &[u8], server_random: &[u8]) -> Result<DataChannelKeys> {
    use hkdf::Hkdf;
    use sha2::Sha256;

    // Combine randoms for salt
    let mut salt = Vec::with_capacity(client_random.len() + server_random.len());
    salt.extend_from_slice(client_random);
    salt.extend_from_slice(server_random);

    // Create HKDF
    let hk = Hkdf::<Sha256>::new(Some(&salt), master_secret);

    // Derive keys (32 bytes for cipher, 32 bytes for HMAC for each direction)
    let mut okm = [0u8; 128];
    hk.expand(b"OpenVPN data channel keys", &mut okm)
        .map_err(|e| anyhow::anyhow!("HKDF expansion failed: {}", e))?;

    let mut client_encrypt_key = [0u8; 32];
    let mut client_hmac_key = [0u8; 32];
    let mut server_encrypt_key = [0u8; 32];
    let mut server_hmac_key = [0u8; 32];

    client_encrypt_key.copy_from_slice(&okm[0..32]);
    client_hmac_key.copy_from_slice(&okm[32..64]);
    server_encrypt_key.copy_from_slice(&okm[64..96]);
    server_hmac_key.copy_from_slice(&okm[96..128]);

    Ok(DataChannelKeys {
        client_encrypt_key,
        client_hmac_key,
        server_encrypt_key,
        server_hmac_key,
    })
}

/// Data channel keys for both directions
#[derive(Debug, Clone)]
pub struct DataChannelKeys {
    pub client_encrypt_key: [u8; 32],
    pub client_hmac_key: [u8; 32],
    pub server_encrypt_key: [u8; 32],
    pub server_hmac_key: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes256gcm_roundtrip() {
        let key = [0x42u8; 32];
        let cipher = DataChannelCipher::new_aes256gcm(&key).unwrap();

        let plaintext = b"Hello, OpenVPN!";
        let aad = b"additional data";
        let packet_id = 12345;

        let ciphertext = cipher.encrypt(packet_id, plaintext, aad).unwrap();
        let decrypted = cipher.decrypt(packet_id, &ciphertext, aad).unwrap();

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_chacha20poly1305_roundtrip() {
        let key = [0x42u8; 32];
        let cipher = DataChannelCipher::new_chacha20poly1305(&key).unwrap();

        let plaintext = b"Hello, OpenVPN!";
        let aad = b"additional data";
        let packet_id = 12345;

        let ciphertext = cipher.encrypt(packet_id, plaintext, aad).unwrap();
        let decrypted = cipher.decrypt(packet_id, &ciphertext, aad).unwrap();

        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_key_derivation() {
        let master_secret = b"master secret for testing";
        let client_random = b"client random data";
        let server_random = b"server random data";

        let keys = derive_data_keys(master_secret, client_random, server_random).unwrap();

        // Keys should be different
        assert_ne!(keys.client_encrypt_key, keys.server_encrypt_key);
        assert_ne!(keys.client_hmac_key, keys.server_hmac_key);
    }
}
