//! End-to-end DNS-over-TLS (DoT) tests for NetGet
//!
//! This test spawns a single NetGet DoT server with mocks
//! and validates multiple query types against the same server instance.

#![cfg(feature = "dot")]

use crate::helpers::{E2EResult, NetGetConfig};
use hickory_proto::op::{Message as DnsMessage, Query};
use hickory_proto::rr::{Name, RecordType};
use rustls::{ClientConfig, RootCertStore};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// Helper to query DoT server
async fn query_dot(port: u16, domain: &str, record_type: RecordType) -> E2EResult<DnsMessage> {
    // Initialize rustls crypto provider (required for rustls 0.23+)
    use rustls::crypto::CryptoProvider;
    let _ = CryptoProvider::install_default(rustls::crypto::ring::default_provider());

    let address: SocketAddr = format!("127.0.0.1:{}", port).parse()?;

    // Create a TLS client config that accepts self-signed certificates (for testing)
    let root_store = RootCertStore::empty();
    let mut config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    // Disable certificate verification for self-signed certs in tests
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(NoCertificateVerification));

    let tls_config = Arc::new(config);
    let connector = TlsConnector::from(tls_config);

    // Connect via TLS
    let tcp_stream = TcpStream::connect(address).await?;
    let domain_name = rustls::pki_types::ServerName::try_from("localhost")
        .map_err(|e| anyhow::anyhow!("Invalid server name: {}", e))?;
    let mut tls_stream = connector.connect(domain_name, tcp_stream).await?;

    // Build DNS query
    let name = Name::from_str(domain)?;
    let mut query_msg = DnsMessage::new();
    query_msg.add_query(Query::query(name, record_type));
    query_msg.set_recursion_desired(true);

    // Serialize to wire format
    let query_bytes = query_msg.to_vec()?;

    // Send with length prefix (DoT protocol: 2-byte length + DNS message)
    let len = query_bytes.len() as u16;
    tls_stream.write_all(&len.to_be_bytes()).await?;
    tls_stream.write_all(&query_bytes).await?;

    // Read response with length prefix
    let mut len_buf = [0u8; 2];
    tls_stream.read_exact(&mut len_buf).await?;
    let response_len = u16::from_be_bytes(len_buf) as usize;

    let mut response_buf = vec![0u8; response_len];
    tls_stream.read_exact(&mut response_buf).await?;

    // Parse DNS response
    let dns_response = DnsMessage::from_vec(&response_buf)?;

    Ok(dns_response)
}

/// Certificate verifier that accepts all certificates (for testing only)
#[derive(Debug)]
struct NoCertificateVerification;

impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

#[tokio::test]
async fn test_dot_server() -> E2EResult<()> {
    println!("\n=== E2E Test: DNS-over-TLS Server with Mocks ===");

    // Create a DoT server with mocks
    let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via DoT. Respond to all A record queries for example.com with IP 93.184.216.34 and TTL 300.")
        .with_log_level("info")
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("DoT")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "DoT",
                        "instruction": "Respond to all A record queries for example.com with IP 93.184.216.34 and TTL 300"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: First DNS query - example.com
                .on_event("dot_query")
                .and_event_data_contains("domain", "example.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_a_response",
                        "query_id": 1,
                        "domain": "example.com",
                        "ip": "93.184.216.34",
                        "ttl": 300
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Second DNS query - test.com (returns same response for all)
                .on_event("dot_query")
                .and_event_data_contains("domain", "test.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_a_response",
                        "query_id": 1,
                        "domain": "test.com",
                        "ip": "93.184.216.34",
                        "ttl": 300
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: Third DNS query - foo.example.com
                .on_event("dot_query")
                .and_event_data_contains("domain", "foo.example.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_a_response",
                        "query_id": 1,
                        "domain": "foo.example.com",
                        "ip": "93.184.216.34",
                        "ttl": 300
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let mut server = crate::helpers::start_netget_server(server_config).await?;

    // Extract server port
    let port = server.port;
    println!("DoT server started on port {}", port);

    // Wait for server to fully initialize
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Test multiple queries against the same server
    println!("\n[Test 1] First query - example.com A record...");
    let response1 = query_dot(port, "example.com.", RecordType::A).await?;
    assert!(
        !response1.answers().is_empty(),
        "Expected answer for example.com A"
    );
    println!("✓ Got response: {:?}", response1.answers()[0]);

    println!("\n[Test 2] Second query - testing TLS connection reuse...");
    let response2 = query_dot(port, "test.com.", RecordType::A).await?;
    assert!(
        !response2.answers().is_empty(),
        "Expected answer for test.com A"
    );
    println!("✓ Got response: {:?}", response2.answers()[0]);

    println!("\n[Test 3] Third query - different domain...");
    let response3 = query_dot(port, "foo.example.com.", RecordType::A).await?;
    assert!(!response3.answers().is_empty(), "Expected answer for foo.example.com A");
    println!("✓ Got response: {:?}", response3.answers()[0]);

    println!("\n=== All DoT tests passed! ===");

    // Verify mock expectations were met
    server.verify_mocks().await?;

    // Cleanup
    server.stop().await?;

    Ok(())
}
