//! Certificate cache for MITM proxy
//!
//! Generates and caches per-domain leaf certificates signed by a CA certificate.
//! This allows the proxy to present valid-looking certificates for any domain
//! when performing TLS Man-in-the-Middle interception.

use anyhow::{Context, Result};
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::collections::HashMap;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;
use tracing::{debug, info, trace};

/// Certificate cache entry
#[derive(Clone)]
struct CachedCert {
    /// The certificate
    cert: Certificate,
    /// The private key
    key_pair: KeyPair,
    /// When this certificate was generated
    generated_at: std::time::Instant,
}

/// Certificate cache for dynamically generated leaf certificates
pub struct CertificateCache {
    /// Root CA certificate (used to sign leaf certificates)
    ca_cert: Arc<Certificate>,
    /// Root CA private key
    ca_key_pair: Arc<KeyPair>,
    /// Cache of per-domain certificates (domain -> certificate)
    cache: Arc<RwLock<HashMap<String, CachedCert>>>,
    /// Certificate TTL in seconds (default: 24 hours)
    cert_ttl_secs: u64,
}

impl CertificateCache {
    /// Create a new certificate cache with a CA certificate
    pub fn new(ca_cert: Certificate, ca_key_pair: KeyPair) -> Self {
        Self {
            ca_cert: Arc::new(ca_cert),
            ca_key_pair: Arc::new(ca_key_pair),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cert_ttl_secs: 24 * 60 * 60, // 24 hours
        }
    }

    /// Get or generate a certificate for a specific domain
    ///
    /// Returns both the certificate and key pair, either from cache or freshly generated.
    pub async fn get_or_generate(&self, domain: &str) -> Result<(Certificate, KeyPair)> {
        // Normalize domain (lowercase, trim)
        let domain_normalized = domain.to_lowercase().trim().to_string();

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&domain_normalized) {
                // Check if certificate is still valid (not expired)
                let age = cached.generated_at.elapsed().as_secs();
                if age < self.cert_ttl_secs {
                    trace!(
                        "Certificate cache HIT for domain '{}' (age: {}s)",
                        domain_normalized,
                        age
                    );
                    return Ok((cached.cert.clone(), cached.key_pair.clone()));
                } else {
                    debug!(
                        "Certificate cache EXPIRED for domain '{}' (age: {}s > {}s)",
                        domain_normalized, age, self.cert_ttl_secs
                    );
                }
            } else {
                debug!("Certificate cache MISS for domain '{}'", domain_normalized);
            }
        }

        // Generate new certificate
        info!("Generating new leaf certificate for domain '{}'", domain_normalized);
        let (cert, key_pair) = self.generate_leaf_cert(&domain_normalized)?;

        // Cache it
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                domain_normalized.clone(),
                CachedCert {
                    cert: cert.clone(),
                    key_pair: key_pair.clone(),
                    generated_at: std::time::Instant::now(),
                },
            );
            debug!(
                "Cached certificate for domain '{}' (cache size: {})",
                domain_normalized,
                cache.len()
            );
        }

        Ok((cert, key_pair))
    }

    /// Generate a leaf certificate for a specific domain, signed by the CA
    fn generate_leaf_cert(&self, domain: &str) -> Result<(Certificate, KeyPair)> {
        let mut params = CertificateParams::default();

        // Set distinguished name with the domain as CN
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, domain);
        dn.push(DnType::OrganizationName, "NetGet MITM Proxy");
        params.distinguished_name = dn;

        // Add Subject Alternative Names (both the domain and wildcard)
        params.subject_alt_names = vec![
            SanType::DnsName(domain.to_string().try_into()
                .context("Invalid domain name")?),
        ];

        // If domain doesn't start with wildcard, also add wildcard version
        if !domain.starts_with("*.") && !domain.starts_with("www.") {
            // Add wildcard for subdomains (e.g., for example.com, add *.example.com)
            let wildcard_domain = format!("*.{}", domain);
            if let Ok(wildcard_san) = wildcard_domain.try_into() {
                params.subject_alt_names.push(SanType::DnsName(wildcard_san));
            }
        }

        // Add www variant if applicable
        if !domain.starts_with("www.") {
            let www_domain = format!("www.{}", domain);
            if let Ok(www_san) = www_domain.try_into() {
                params.subject_alt_names.push(SanType::DnsName(www_san));
            }
        }

        // Set validity period (24 hours to match cache TTL)
        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(1);

        // Mark as NOT a CA (this is a leaf certificate)
        params.is_ca = rcgen::IsCa::NoCa;

        // Generate key pair for this certificate
        let key_pair = KeyPair::generate().context("Failed to generate key pair for leaf certificate")?;

        // Sign this certificate with the CA
        let cert = params
            .signed_by(&key_pair, &self.ca_cert, &self.ca_key_pair)
            .context("Failed to sign leaf certificate with CA")?;

        info!(
            "Successfully generated leaf certificate for domain '{}' (valid for 24h, {} SANs)",
            domain,
            params.subject_alt_names.len()
        );
        trace!("Leaf certificate SANs: {:?}", params.subject_alt_names);

        Ok((cert, key_pair))
    }

    /// Get the CA certificate (for exporting to users)
    pub fn get_ca_cert(&self) -> &Certificate {
        &self.ca_cert
    }

    /// Get the CA key pair
    pub fn get_ca_key_pair(&self) -> &KeyPair {
        &self.ca_key_pair
    }

    /// Clear expired certificates from the cache
    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        let initial_size = cache.len();

        cache.retain(|domain, cached| {
            let age = cached.generated_at.elapsed().as_secs();
            if age >= self.cert_ttl_secs {
                debug!("Removing expired certificate for domain '{}' (age: {}s)", domain, age);
                false
            } else {
                true
            }
        });

        let removed = initial_size - cache.len();
        if removed > 0 {
            info!("Cleaned up {} expired certificates from cache (remaining: {})", removed, cache.len());
        }
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let total_certs = cache.len();

        let mut expired_count = 0;
        for cached in cache.values() {
            let age = cached.generated_at.elapsed().as_secs();
            if age >= self.cert_ttl_secs {
                expired_count += 1;
            }
        }

        CacheStats {
            total_certificates: total_certs,
            expired_certificates: expired_count,
            valid_certificates: total_certs - expired_count,
        }
    }

    /// Convert certificate and key pair to rustls format
    pub fn to_rustls_cert(
        cert: &Certificate,
        key_pair: &KeyPair,
    ) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
        // Get the certificate DER
        let cert_der = cert.der();
        let cert_der_owned = CertificateDer::from(cert_der.to_vec());

        // Get the private key DER from the KeyPair
        let key_der_vec = key_pair.serialize_der();
        let key_der_owned = PrivateKeyDer::try_from(key_der_vec)
            .map_err(|e| anyhow::anyhow!("Failed to parse private key DER: {}", e))?;

        Ok((vec![cert_der_owned], key_der_owned))
    }
}

/// Certificate cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_certificates: usize,
    pub expired_certificates: usize,
    pub valid_certificates: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cert_cache_generation() {
        // Generate a CA certificate
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params.distinguished_name.push(DnType::CommonName, "Test CA");
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        // Create cache
        let cache = CertificateCache::new(ca_cert, ca_key);

        // Generate certificate for example.com
        let (cert1, _key1) = cache.get_or_generate("example.com").await.unwrap();

        // Verify certificate has correct CN
        assert!(cert1.pem().contains("example.com"));

        // Second request should hit cache
        let (cert2, _key2) = cache.get_or_generate("example.com").await.unwrap();
        assert_eq!(cert1.pem(), cert2.pem(), "Certificate should be cached");

        // Different domain should generate new cert
        let (cert3, _key3) = cache.get_or_generate("different.com").await.unwrap();
        assert_ne!(cert1.pem(), cert3.pem(), "Different domain should have different cert");

        // Verify cache stats
        let stats = cache.get_stats().await;
        assert_eq!(stats.total_certificates, 2, "Should have 2 certificates in cache");
    }

    #[tokio::test]
    async fn test_cert_cache_normalization() {
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params.distinguished_name.push(DnType::CommonName, "Test CA");
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let cache = CertificateCache::new(ca_cert, ca_key);

        // These should all result in the same cached certificate
        let (cert1, _) = cache.get_or_generate("Example.COM").await.unwrap();
        let (cert2, _) = cache.get_or_generate("example.com").await.unwrap();
        let (cert3, _) = cache.get_or_generate("  example.com  ").await.unwrap();

        assert_eq!(cert1.pem(), cert2.pem());
        assert_eq!(cert2.pem(), cert3.pem());

        let stats = cache.get_stats().await;
        assert_eq!(stats.total_certificates, 1, "Should have only 1 certificate (normalized)");
    }
}
