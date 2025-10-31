//! Tor consensus document builder for test networks
//!
//! Generates minimal valid consensus documents pointing to our test relay.

use super::helpers::RelayKeys;
use chrono::Utc;
use sha2::{Digest, Sha256};
use anyhow::Result;

/// Build a minimal Tor consensus document for testing
pub fn build_consensus(relay: &RelayKeys) -> Result<String> {
    let now = Utc::now();
    let valid_after = now - chrono::Duration::minutes(5);
    let fresh_until = now + chrono::Duration::hours(1);
    let valid_until = now + chrono::Duration::hours(3);

    // Format timestamps for Tor consensus (YYYY-MM-DD HH:MM:SS)
    let format_time = |t: chrono::DateTime<Utc>| t.format("%Y-%m-%d %H:%M:%S").to_string();

    // Calculate microdescriptor digest
    let microdesc = build_microdescriptor(relay)?;
    let microdesc_digest = calculate_microdescriptor_digest(&microdesc);

    // Build consensus document
    let consensus = format!(
        r#"network-status-version 3
vote-status consensus
consensus-method 31
valid-after {}
fresh-until {}
valid-until {}
voting-delay 300 300
client-versions 0.4.7.0
server-versions 0.4.7.0
known-flags Authority Exit Fast Guard HSDir Running Stable V2Dir Valid
params CircuitPriorityHalflifeMsec=30000 DoSCircuitCreationEnabled=1

dir-source test-authority 0000000000000000000000000000000000000000 {} 9030 9090
contact test@example.com
vote-digest 0000000000000000000000000000000000000000

r netget-relay {} {} {} {} {} 0
s Exit Fast Running Stable Valid
v Tor 0.4.7.0
pr Cons=1-2 Desc=1-2 Link=1-5 LinkAuth=3 Microdesc=1-2 Relay=1-2
w Bandwidth=1000
p accept 1-65535
m {}
"#,
        format_time(valid_after),
        format_time(fresh_until),
        format_time(valid_until),
        relay.address,
        relay.identity_fingerprint[..8].to_uppercase(),  // Truncated base64 identity
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAA",                 // Descriptor digest (base64)
        format_time(now),
        relay.address,
        relay.or_port,
        microdesc_digest,
    );

    Ok(consensus)
}

/// Build a microdescriptor for the relay
fn build_microdescriptor(relay: &RelayKeys) -> Result<String> {
    // Generate a dummy RSA onion key (required by Tor spec, even though ntor is used)
    let rsa_key_pem = r#"-----BEGIN RSA PUBLIC KEY-----
MIGJAoGBALmFdvtFdGLAQh/TpAz7fCT8jVD3kl4nHDGpfQCz7jDf5H0Yl5U9/bLc
V6xQq6VKf0rJpXqWvGq0lFNmQDKXqGq5/J4bOzMdJGEkdwKlNDKQv6YgF0KcQqPe
fNd9kl7p7C8xGjBfNl8Jt2YH2Lq6xQ7K8VkPx3gJz0Jl5F7l9kLdAgMBAAE=
-----END RSA PUBLIC KEY-----"#;

    let microdesc = format!(
        r#"onion-key
{}
ntor-onion-key {}
id ed25519 {}
"#,
        rsa_key_pem, relay.ntor_onion_key, relay.ed25519_identity
    );

    Ok(microdesc)
}

/// Calculate SHA256 digest of microdescriptor for consensus
fn calculate_microdescriptor_digest(microdesc: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(microdesc.as_bytes());
    let digest = hasher.finalize();
    base64::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_generation() {
        let relay = RelayKeys {
            identity_fingerprint: "0123456789abcdef0123456789abcdef01234567".to_string(),
            ed25519_identity: base64::encode(&[1u8; 32]),
            ntor_onion_key: base64::encode(&[2u8; 32]),
            address: "127.0.0.1".to_string(),
            or_port: 9001,
        };

        let consensus = build_consensus(&relay).unwrap();

        // Verify consensus contains required fields
        assert!(consensus.contains("network-status-version 3"));
        assert!(consensus.contains("vote-status consensus"));
        assert!(consensus.contains(&relay.address));
        // Note: directory-footer and signature are added by the action handler
    }

    #[test]
    fn test_microdescriptor_generation() {
        let relay = RelayKeys {
            identity_fingerprint: "0123456789abcdef0123456789abcdef01234567".to_string(),
            ed25519_identity: base64::encode(&[1u8; 32]),
            ntor_onion_key: base64::encode(&[2u8; 32]),
            address: "127.0.0.1".to_string(),
            or_port: 9001,
        };

        let microdesc = build_microdescriptor(&relay).unwrap();

        // Verify microdescriptor contains required fields
        assert!(microdesc.contains("onion-key"));
        assert!(microdesc.contains("ntor-onion-key"));
        assert!(microdesc.contains("id ed25519"));
        assert!(microdesc.contains(&relay.ntor_onion_key));
    }
}
