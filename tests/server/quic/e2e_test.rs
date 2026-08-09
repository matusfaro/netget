//! End-to-end tests for QUIC protocol implementation
//!
//! These tests spawn a real NetGet instance with QUIC server
//! and use quinn client to test QUIC functionality.

#![cfg(all(test, feature = "quic"))]

use super::super::helpers::{self, E2EResult, NetGetConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// A prompt saying "quic" must reach this protocol.
///
/// The protocol used to be called `http3` while serving raw QUIC streams. After the
/// rename the lookup path is the thing most likely to silently break, so assert it
/// directly instead of only through `base_stack` in the mocks below. `http3` must
/// *not* resolve here: NetGet has no HTTP/3 server, and resolving it to a raw QUIC
/// socket is the same silent mis-resolution `ftp` -> TCP used to cause.
#[test]
fn quic_keyword_resolves_to_this_protocol() {
    use netget::protocol::server_registry::registry;

    assert_eq!(
        registry().parse_from_str("quic"),
        Some("QUIC".to_string()),
        "\"quic\" must resolve to the QUIC protocol"
    );
    assert_eq!(
        registry().parse_from_str("listen on port 4433 via quic"),
        Some("QUIC".to_string()),
        "a natural-language prompt naming quic must resolve to the QUIC protocol"
    );
    assert_ne!(
        registry().parse_from_str("http3"),
        Some("QUIC".to_string()),
        "\"http3\" must not resolve to the raw QUIC server - it is not an HTTP/3 server"
    );
}

/// Test QUIC echo server - send data and receive it back
#[tokio::test]
async fn test_quic_echo() -> E2EResult<()> {
    let config = NetGetConfig::new("Start a QUIC server on port 0")
        .with_log_level("debug")
        .with_mock(|mock| {
            mock
                // IMPORTANT: Event-specific mocks MUST come first
                // The mock system uses the FIRST matching rule
                // Mock 1: Connection opened - just acknowledge
                .on_event("quic_connection_opened")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Connection opened"
                    }
                ]))
                .and()
                // Mock 2: Stream opened - just acknowledge
                .on_event("quic_stream_opened")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Stream opened"
                    }
                ]))
                .and()
                // Mock 3: LLM receives data and echoes it back
                .on_event("quic_data_received")
                .and_event_data_contains("data", "Hello")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_quic_data",
                        "data": "Hello, QUIC!"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: Server startup (catch-all for user input, MUST come LAST)
                .on_custom(|ctx| !ctx.instruction.contains("Event ID:"))
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "QUIC",
                        "instruction": "Echo back all data received"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
    let port = server.port;

    println!("✓ QUIC server started on port {}", port);

    // Install rustls crypto provider for client
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Configure QUIC client to skip certificate validation (self-signed cert)
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
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
            .expect("Failed to create QUIC client config"),
    ));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .expect("Failed to create client endpoint");
    endpoint.set_default_client_config(client_config);

    // Connect to QUIC server
    let connecting = endpoint
        .connect(format!("127.0.0.1:{}", port).parse().unwrap(), "localhost")
        .expect("Failed to start connection");

    let connection = timeout(Duration::from_secs(10), connecting)
        .await
        .expect("Connection timeout")
        .expect("Failed to complete connection");

    println!("✓ Connected to QUIC server");

    // Open bidirectional stream
    let (mut send, mut recv) = timeout(Duration::from_secs(10), connection.open_bi())
        .await
        .expect("Stream open timeout")
        .expect("Failed to open stream");

    // Send test data
    let test_data = b"Hello, QUIC!";
    send.write_all(test_data)
        .await
        .expect("Failed to send data");
    send.finish().expect("Failed to finish stream");

    println!("✓ Sent data to QUIC server");

    // Read response from LLM
    let response = timeout(Duration::from_secs(5), recv.read_to_end(1024))
        .await
        .expect("Read timeout")
        .expect("Failed to read response");

    println!("✓ Received response: {} bytes", response.len());

    // Verify echo
    assert_eq!(response, test_data.to_vec(), "Expected echo of sent data");

    // Cleanup
    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    // Verify mock expectations were met
    server.verify_mocks().await?;

    server.stop().await?;

    Ok(())
}

/// Test QUIC custom response - send command and receive specific response
#[tokio::test]
async fn test_quic_custom_response() -> E2EResult<()> {
    let config = NetGetConfig::new("Start a QUIC server on port 0")
        .with_log_level("debug")
        .with_mock(|mock| {
            mock
                // IMPORTANT: Event-specific mocks MUST come first
                // The mock system uses the FIRST matching rule
                // Mock 1: Connection opened - just acknowledge
                .on_event("quic_connection_opened")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Connection opened"
                    }
                ]))
                .and()
                // Mock 2: Stream opened - just acknowledge
                .on_event("quic_stream_opened")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Stream opened"
                    }
                ]))
                .and()
                // Mock 3: LLM receives PING and responds with PONG
                .on_event("quic_data_received")
                .and_event_data_contains("data", "PING")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_quic_data",
                        "data": "PONG"
                    },
                    {
                        "type": "close_this_stream"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: Server startup (catch-all for user input, MUST come LAST)
                .on_custom(|ctx| !ctx.instruction.contains("Event ID:"))
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "QUIC",
                        "instruction": "Respond to PING with PONG"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
    let port = server.port;

    println!("✓ QUIC server started on port {}", port);

    // Install rustls crypto provider for client
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Configure QUIC client (same as above)
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
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
            .expect("Failed to create QUIC client config"),
    ));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .expect("Failed to create client endpoint");
    endpoint.set_default_client_config(client_config);

    // Connect to QUIC server
    let connecting = endpoint
        .connect(format!("127.0.0.1:{}", port).parse().unwrap(), "localhost")
        .expect("Failed to start connection");

    let connection = timeout(Duration::from_secs(10), connecting)
        .await
        .expect("Connection timeout")
        .expect("Failed to complete connection");

    println!("✓ Connected to QUIC server");

    // Open bidirectional stream
    let (mut send, mut recv) = timeout(Duration::from_secs(10), connection.open_bi())
        .await
        .expect("Stream open timeout")
        .expect("Failed to open stream");

    // Send PING
    send.write_all(b"PING").await.expect("Failed to send data");
    send.finish().expect("Failed to finish stream");

    println!("✓ Sent PING to QUIC server");

    // Read PONG response from LLM
    let response = timeout(Duration::from_secs(5), recv.read_to_end(1024))
        .await
        .expect("Read timeout")
        .expect("Failed to read response");

    let response_str = String::from_utf8_lossy(&response);
    println!("✓ Received response: {}", response_str);

    assert_eq!(response_str, "PONG", "Expected PONG response");

    // Cleanup
    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    // Verify mock expectations were met
    server.verify_mocks().await?;

    server.stop().await?;

    Ok(())
}

/// Test QUIC multiple streams - verify stream multiplexing
#[tokio::test]
async fn test_quic_multiple_streams() -> E2EResult<()> {
    let config = NetGetConfig::new("Start a QUIC server on port 0")
        .with_log_level("debug")
        .with_mock(|mock| {
            mock
                // IMPORTANT: Event-specific mocks MUST come first
                // The mock system uses the FIRST matching rule
                // Mock 1: Connection opened - just acknowledge
                .on_event("quic_connection_opened")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Connection opened"
                    }
                ]))
                .and()
                // Mock 2: Stream opened - just acknowledge
                .on_event("quic_stream_opened")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Stream opened"
                    }
                ]))
                .and()
                // Mock 3: LLM receives data and echoes it back (matches any stream)
                .on_event("quic_data_received")
                .and_event_data_contains("data", "Stream")
                .respond_with_actions_from_event(|event_data| {
                    // Extract the data from the event and echo it back
                    let data = event_data["data"].as_str().unwrap_or("Stream");
                    serde_json::json!([
                        {
                            "type": "send_quic_data",
                            "data": data
                        }
                    ])
                })
                .expect_calls(3) // Expecting 3 streams
                .and()
                // Mock 4: Server startup (catch-all for user input, MUST come LAST)
                .on_custom(|ctx| !ctx.instruction.contains("Event ID:"))
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "QUIC",
                        "instruction": "Echo back all data on multiple streams"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
    let port = server.port;

    println!("✓ QUIC server started on port {}", port);

    // Install rustls crypto provider for client
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Configure QUIC client
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
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
            .expect("Failed to create QUIC client config"),
    ));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .expect("Failed to create client endpoint");
    endpoint.set_default_client_config(client_config);

    // Connect to QUIC server
    let connecting = endpoint
        .connect(format!("127.0.0.1:{}", port).parse().unwrap(), "localhost")
        .expect("Failed to start connection");

    let connection = timeout(Duration::from_secs(10), connecting)
        .await
        .expect("Connection timeout")
        .expect("Failed to complete connection");

    println!("✓ Connected to QUIC server");

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

            // Read response from LLM
            let response = timeout(Duration::from_secs(5), recv.read_to_end(1024))
                .await
                .expect("Read timeout")
                .expect("Failed to read");
            (test_data, String::from_utf8_lossy(&response).to_string())
        });
        handles.push(handle);
    }

    // Wait for all streams to complete and verify echoes
    for handle in handles {
        let (sent, received) = timeout(Duration::from_secs(15), handle)
            .await
            .expect("Stream timeout")
            .expect("Stream task failed");

        println!("✓ Stream test - sent: {}, received: {}", sent, received);
        assert_eq!(sent, received, "Expected echo on stream");
    }

    // Cleanup
    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    // Verify mock expectations were met
    server.verify_mocks().await?;

    server.stop().await?;

    Ok(())
}

/// Certificate verifier that skips all verification (for self-signed certs)
#[derive(Debug)]
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
