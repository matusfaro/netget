//! Consensus Document Signing for Tor Directory Authorities
//!
//! This module handles signing consensus documents according to the Tor directory protocol.
//! The signature format follows Tor spec section 3.4.1:
//!
//! ```text
//! directory-signature [algorithm] identity-key-digest signing-key-digest
//! -----BEGIN SIGNATURE-----
//! <base64-encoded signature>
//! -----END SIGNATURE-----
//! ```

use super::authority_keys::AuthorityKeys;
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// Sign a consensus document with authority keys
///
/// # Arguments
/// * `consensus_body` - The consensus document text (without signature block)
/// * `keys` - The authority keys to sign with
///
/// # Returns
/// Complete consensus document with signature block appended
pub fn sign_consensus(consensus_body: &str, keys: &AuthorityKeys) -> Result<String> {
    // Calculate fingerprints
    let identity_digest = keys.authority_fingerprint();
    let signing_key_digest = keys.v3_identity_fingerprint(); // Using same for simplicity

    // Sign the consensus body
    let signature = keys.sign(consensus_body.as_bytes());
    let signature_base64 = BASE64.encode(signature.to_bytes());

    // Format signature block according to Tor spec
    // Break base64 into 64-character lines
    let sig_lines = signature_base64
        .as_bytes()
        .chunks(64)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect::<Vec<_>>()
        .join("\n");

    // Build complete consensus with signature
    let signed_consensus = format!(
        "{}directory-signature sha256 {} {}\n-----BEGIN SIGNATURE-----\n{}\n-----END SIGNATURE-----\n",
        consensus_body,
        identity_digest,
        signing_key_digest,
        sig_lines
    );

    Ok(signed_consensus)
}

/// Build directory-footer section
///
/// This is required before the signature block
pub fn build_directory_footer() -> String {
    "directory-footer\n".to_string()
}

