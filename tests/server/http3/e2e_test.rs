//! End-to-end tests for HTTP3 protocol implementation
//!
//! These tests spawn a real NetGet instance with HTTP3 server
//! and use quinn client to test HTTP3 functionality.

#![cfg(all(test, feature = "http3"))]

use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

mod helpers {
    pub use crate::server::helpers::*;
}
use helpers::{spawn_test_server, TestServerHandle};

/// Test HTTP3 echo server - send data and receive it back
#[tokio::test]
async fn test_http3_echo() {
    let prompt = "listen on port {AVAILABLE_PORT} via http3. When you receive data on any stream, echo it back exactly as received.";

    let handle = spawn_test_server(prompt).await;
    let port = handle.port;

    // Configure HTTP3 client to skip certificate validation (self-signed cert)
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    // CRITICAL: Accept invalid certificates (self-signed)
    client_crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));

    client_crypto.alpn_protocols = vec![b"h3".to_vec()];

    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::Http3ClientConfig::try_from(client_crypto)
            .expect("Failed to create HTTP3 client config"),
    ));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .expect("Failed to create client endpoint");
    endpoint.set_default_client_config(client_config);

    // Connect to HTTP3 server
    let connection = timeout(
        Duration::from_secs(10),
        endpoint.connect(
            format!("127.0.0.1:{}", port).parse().unwrap(),
            "localhost",
        ),
    )
    .await
    .expect("Connection timeout")
    .expect("Failed to connect")
    .await
    .expect("Failed to complete connection");

    // Open bidirectional stream
    let (mut send, mut recv) = timeout(
        Duration::from_secs(10),
        connection.open_bi(),
    )
    .await
    .expect("Stream open timeout")
    .expect("Failed to open stream");

    // Send test data
    let test_data = b"Hello, HTTP3!";
    send.write_all(test_data)
        .await
        .expect("Failed to send data");
    send.finish().expect("Failed to finish stream");

    // Read echo response
    let response = timeout(
        Duration::from_secs(10),
        recv.read_to_end(1024),
    )
    .await
    .expect("Read timeout")
    .expect("Failed to read response");

    // Verify echo
    assert_eq!(response, test_data, "Expected echo of sent data");

    // Cleanup
    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    handle.stop().await;
}

/// Test HTTP3 custom response - send command and receive specific response
#[tokio::test]
async fn test_http3_custom_response() {
    let prompt = "listen on port {AVAILABLE_PORT} via http3. When you receive 'PING' on a stream, respond with 'PONG' and close the stream.";

    let handle = spawn_test_server(prompt).await;
    let port = handle.port;

    // Configure HTTP3 client (same as above)
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    client_crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));

    client_crypto.alpn_protocols = vec![b"h3".to_vec()];

    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::Http3ClientConfig::try_from(client_crypto)
            .expect("Failed to create HTTP3 client config"),
    ));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .expect("Failed to create client endpoint");
    endpoint.set_default_client_config(client_config);

    // Connect to HTTP3 server
    let connection = timeout(
        Duration::from_secs(10),
        endpoint.connect(
            format!("127.0.0.1:{}", port).parse().unwrap(),
            "localhost",
        ),
    )
    .await
    .expect("Connection timeout")
    .expect("Failed to connect")
    .await
    .expect("Failed to complete connection");

    // Open bidirectional stream
    let (mut send, mut recv) = timeout(
        Duration::from_secs(10),
        connection.open_bi(),
    )
    .await
    .expect("Stream open timeout")
    .expect("Failed to open stream");

    // Send PING
    send.write_all(b"PING")
        .await
        .expect("Failed to send data");
    send.finish().expect("Failed to finish stream");

    // Read PONG response
    let response = timeout(
        Duration::from_secs(10),
        recv.read_to_end(1024),
    )
    .await
    .expect("Read timeout")
    .expect("Failed to read response");

    let response_str = String::from_utf8_lossy(&response);
    assert!(
        response_str.contains("PONG"),
        "Expected response to contain 'PONG', got: {}",
        response_str
    );

    // Cleanup
    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    handle.stop().await;
}

/// Test HTTP3 multiple streams - verify stream multiplexing
#[tokio::test]
async fn test_http3_multiple_streams() {
    let prompt = "listen on port {AVAILABLE_PORT} via http3. Echo back all data received on each stream. Handle multiple streams concurrently.";

    let handle = spawn_test_server(prompt).await;
    let port = handle.port;

    // Configure HTTP3 client
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    client_crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));

    client_crypto.alpn_protocols = vec![b"h3".to_vec()];

    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::Http3ClientConfig::try_from(client_crypto)
            .expect("Failed to create HTTP3 client config"),
    ));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .expect("Failed to create client endpoint");
    endpoint.set_default_client_config(client_config);

    // Connect to HTTP3 server
    let connection = timeout(
        Duration::from_secs(10),
        endpoint.connect(
            format!("127.0.0.1:{}", port).parse().unwrap(),
            "localhost",
        ),
    )
    .await
    .expect("Connection timeout")
    .expect("Failed to connect")
    .await
    .expect("Failed to complete connection");

    // Open 3 streams concurrently
    let mut handles = vec![];
    for i in 0..3 {
        let conn = connection.clone();
        let handle = tokio::spawn(async move {
            let (mut send, mut recv) = conn.open_bi().await.expect("Failed to open stream");

            let test_data = format!("Stream {}", i);
            send.write_all(test_data.as_bytes())
                .await
                .expect("Failed to send");
            send.finish().expect("Failed to finish");

            let response = recv.read_to_end(1024).await.expect("Failed to read");
            (test_data, String::from_utf8_lossy(&response).to_string())
        });
        handles.push(handle);
    }

    // Wait for all streams to complete
    for handle in handles {
        let (sent, received) = timeout(Duration::from_secs(15), handle)
            .await
            .expect("Stream timeout")
            .expect("Stream task failed");
        assert_eq!(sent, received, "Expected echo on stream");
    }

    // Cleanup
    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    handle.stop().await;
}

/// Certificate verifier that skips all verification (for self-signed certs)
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
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
