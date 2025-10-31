//! TLS certificate management for DoT and DoH servers
//!
//! Provides LLM-controlled certificate generation with self-signed fallback.

use anyhow::{Context, Result};
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use tracing::{debug, info};
use time::{Duration, OffsetDateTime};

/// Certificate specification from LLM or defaults
#[derive(Debug, Clone)]
pub struct CertificateSpec {
    /// Common name (CN) for the certificate
    pub common_name: String,
    /// Subject Alternative Names (DNS names)
    pub san_dns_names: Vec<String>,
    /// Certificate validity period in days
    pub validity_days: i64,
    /// Organization name
    pub organization: Option<String>,
    /// Organizational unit
    pub organizational_unit: Option<String>,
}

impl Default for CertificateSpec {
    fn default() -> Self {
        Self {
            common_name: "netget-dns-server".to_string(),
            san_dns_names: vec![
                "localhost".to_string(),
                "*.local".to_string(),
            ],
            validity_days: 365,
            organization: Some("NetGet".to_string()),
            organizational_unit: Some("DNS Server".to_string()),
        }
    }
}

impl CertificateSpec {
    /// Create a new certificate spec from LLM parameters
    pub fn from_llm_params(
        common_name: Option<String>,
        san_dns_names: Option<Vec<String>>,
        validity_days: Option<i64>,
        organization: Option<String>,
        organizational_unit: Option<String>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            common_name: common_name.unwrap_or(defaults.common_name),
            san_dns_names: san_dns_names.unwrap_or(defaults.san_dns_names),
            validity_days: validity_days.unwrap_or(defaults.validity_days),
            organization: organization.or(defaults.organization),
            organizational_unit: organizational_unit.or(defaults.organizational_unit),
        }
    }
}

/// Generate a self-signed TLS certificate based on spec
/// Returns both the Certificate and its KeyPair
pub fn generate_self_signed_cert(spec: &CertificateSpec) -> Result<(Certificate, KeyPair)> {
    info!("Generating self-signed TLS certificate");
    debug!("Certificate spec: {:?}", spec);

    let mut params = CertificateParams::default();

    // Set distinguished name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, &spec.common_name);

    if let Some(ref org) = spec.organization {
        dn.push(DnType::OrganizationName, org);
    }

    if let Some(ref ou) = spec.organizational_unit {
        dn.push(DnType::OrganizationalUnitName, ou);
    }

    params.distinguished_name = dn;

    // Add Subject Alternative Names
    params.subject_alt_names = spec.san_dns_names
        .iter()
        .map(|name| SanType::DnsName(name.to_string().try_into().unwrap()))
        .collect();

    // Set validity period
    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + Duration::days(spec.validity_days);

    // Generate key pair and self-sign
    let key_pair = KeyPair::generate()
        .context("Failed to generate key pair")?;

    let cert = params.self_signed(&key_pair)
        .context("Failed to create self-signed certificate")?;

    info!("Successfully generated self-signed certificate for CN={}", spec.common_name);

    Ok((cert, key_pair))
}

/// Create a rustls ServerConfig from an rcgen Certificate and KeyPair
pub fn create_rustls_server_config(cert: &Certificate, key_pair: &KeyPair) -> Result<Arc<ServerConfig>> {
    // Get the certificate DER
    let cert_der = cert.der();
    let cert_der_owned = CertificateDer::from(cert_der.to_vec());

    // Get the private key DER from the KeyPair
    let key_der_vec = key_pair.serialize_der();
    let key_der_owned = PrivateKeyDer::try_from(key_der_vec)
        .map_err(|e| anyhow::anyhow!("Failed to parse private key DER: {}", e))?;

    // Build rustls ServerConfig
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der_owned], key_der_owned)
        .context("Failed to create rustls ServerConfig")?;

    Ok(Arc::new(config))
}

/// Generate a default self-signed certificate and create rustls ServerConfig
pub fn generate_default_tls_config() -> Result<Arc<ServerConfig>> {
    let spec = CertificateSpec::default();
    let (cert, key_pair) = generate_self_signed_cert(&spec)?;
    create_rustls_server_config(&cert, &key_pair)
}

/// Generate a custom TLS configuration from LLM-specified parameters
pub fn generate_custom_tls_config(
    common_name: Option<String>,
    san_dns_names: Option<Vec<String>>,
    validity_days: Option<i64>,
    organization: Option<String>,
    organizational_unit: Option<String>,
) -> Result<Arc<ServerConfig>> {
    let spec = CertificateSpec::from_llm_params(
        common_name,
        san_dns_names,
        validity_days,
        organization,
        organizational_unit,
    );

    let (cert, key_pair) = generate_self_signed_cert(&spec)?;
    create_rustls_server_config(&cert, &key_pair)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cert_spec() {
        let spec = CertificateSpec::default();
        assert_eq!(spec.common_name, "netget-dns-server");
        assert!(spec.san_dns_names.contains(&"localhost".to_string()));
        assert_eq!(spec.validity_days, 365);
    }

    #[test]
    fn test_generate_self_signed_cert() {
        let spec = CertificateSpec::default();
        let cert = generate_self_signed_cert(&spec);
        assert!(cert.is_ok());
    }

    #[test]
    fn test_generate_default_tls_config() {
        let config = generate_default_tls_config();
        assert!(config.is_ok());
    }

    #[test]
    fn test_generate_custom_tls_config() {
        let config = generate_custom_tls_config(
            Some("test.example.com".to_string()),
            Some(vec!["test.local".to_string(), "*.test.local".to_string()]),
            Some(30),
            Some("Test Org".to_string()),
            None,
        );
        assert!(config.is_ok());
    }
}
