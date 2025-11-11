//! HTTP/3 client implementation using QUIC
pub mod actions;

pub use actions::Http3ClientProtocol;

use anyhow::{Context, Result};
use bytes::Bytes;
use http::Request;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::http3::actions::HTTP3_CLIENT_RESPONSE_RECEIVED_EVENT;

/// HTTP/3 client that makes requests to remote HTTP/3 servers over QUIC
pub struct Http3Client;

impl Http3Client {
    /// Connect to an HTTP/3 server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("HTTP/3 client {} initializing for {}", client_id, remote_addr);

        // Parse remote address
        let remote_sock_addr: SocketAddr = remote_addr.parse()
            .context("Invalid remote address format, expected host:port")?;

        // Store base URL and connection info in protocol_data
        let base_url = format!("https://{}", remote_addr);

        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "base_url".to_string(),
                serde_json::json!(base_url),
            );
            client.set_protocol_field(
                "remote_addr".to_string(),
                serde_json::json!(remote_addr),
            );
            client.set_protocol_field(
                "quic_initialized".to_string(),
                serde_json::json!(true),
            );
        }).await;

        // Update status to connected
        app_state.update_client_status(client_id, ClientStatus::Connected).await;

        console_info!(status_tx, "[CLIENT] HTTP/3 client {} ready for {} (QUIC transport)");
        console_info!(status_tx, "__UPDATE_UI__");

        info!("HTTP/3 client {} initialized successfully", client_id);

        // Spawn background monitoring task
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("HTTP/3 client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return the remote address
        Ok(remote_sock_addr)
    }

    /// Make an HTTP/3 request over QUIC
    #[allow(clippy::too_many_arguments)]
    pub async fn make_request(
        client_id: ClientId,
        method: String,
        path: String,
        headers: Option<serde_json::Map<String, serde_json::Value>>,
        body: Option<String>,
        priority: Option<u8>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get connection info from client
        let (base_url, remote_addr) = app_state.with_client_mut(client_id, |client| {
            let base_url = client.get_protocol_field("base_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let remote_addr = client.get_protocol_field("remote_addr")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (base_url, remote_addr)
        }).await.context("Client not found")?;

        let base_url = base_url.context("No base URL found")?;
        let remote_addr_str = remote_addr.context("No remote address found")?;
        let remote_sock_addr: SocketAddr = remote_addr_str.parse()?;

        // Build full URL
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.clone()
        } else {
            format!("{}{}", base_url, path)
        };

        info!("HTTP/3 client {} making request: {} {} (priority: {:?})",
              client_id, method, url, priority);

        // Create QUIC endpoint
        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;

        // Configure TLS (accept invalid certs for now - can be made configurable)
        let mut rustls_client_config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        // Set ALPN to h3
        rustls_client_config.alpn_protocols = vec![b"h3".to_vec()];

        // Convert to quinn client config
        let client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(rustls_client_config)?
        ));

        endpoint.set_default_client_config(client_config);

        // Extract host from URL for SNI
        let url_parsed = url::Url::parse(&url)?;
        let host = url_parsed.host_str().context("No host in URL")?;

        info!("HTTP/3 client {} connecting to {} via QUIC", client_id, remote_sock_addr);

        // Connect via QUIC
        let connection = endpoint.connect(remote_sock_addr, host)
            .context("Failed to create QUIC connection")?
            .await
            .context("Failed to establish QUIC connection")?;

        info!("HTTP/3 client {} established QUIC connection", client_id);

        // Create H3 connection
        let quinn_connection = h3_quinn::Connection::new(connection);
        let (mut h3_conn, mut send_request) = h3::client::new(quinn_connection)
            .await
            .context("Failed to create HTTP/3 connection")?;

        info!("HTTP/3 client {} created HTTP/3 session", client_id);

        // Build HTTP request
        let mut req_builder = Request::builder()
            .uri(&url)
            .method(method.as_str());

        // Add headers
        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                if let Some(val_str) = value.as_str() {
                    req_builder = req_builder.header(&key, val_str);
                }
            }
        }

        // Build request body
        let req_body = body.unwrap_or_default();
        let request = req_builder.body(()).context("Failed to build request")?;

        // Send request
        let mut stream = send_request.send_request(request)
            .await
            .context("Failed to send HTTP/3 request")?;

        // Send body if present
        if !req_body.is_empty() {
            stream.send_data(Bytes::from(req_body))
                .await
                .context("Failed to send request body")?;
        }

        stream.finish().await.context("Failed to finish sending request")?;

        info!("HTTP/3 client {} sent request, waiting for response", client_id);

        // Receive response
        let response = stream.recv_response()
            .await
            .context("Failed to receive response")?;

        let status = response.status();
        let status_code = status.as_u16();

        // Get headers
        let mut resp_headers = serde_json::Map::new();
        for (name, value) in response.headers() {
            if let Ok(val_str) = value.to_str() {
                resp_headers.insert(name.to_string(), serde_json::json!(val_str));
            }
        }

        // Read response body
        let mut body_bytes = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await? {
            use bytes::Buf;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
            body_bytes.extend_from_slice(chunk.chunk());
            chunk.advance(chunk.remaining());
        }
        let body_text = String::from_utf8_lossy(&body_bytes).to_string();

        info!("HTTP/3 client {} received response: {} ({})", client_id, status_code, status);

        // Call LLM with response
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::http3::actions::Http3ClientProtocol::new());
            let event = Event::new(
                &HTTP3_CLIENT_RESPONSE_RECEIVED_EVENT,
                serde_json::json!({
                    "status_code": status_code,
                    "status_text": status.to_string(),
                    "headers": resp_headers,
                    "body": body_text,
                    "stream_id": 0u64, // TODO: Get actual stream ID if available
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions: _, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for HTTP/3 client {}: {}", client_id, e);
                }
            }
        }

        // Close connection gracefully
        h3_conn.shutdown(0).await?;
        endpoint.close(0u32.into(), b"done");

        Ok(())
    }
}

/// Skip server certificate verification (for testing)
/// TODO: Make this configurable
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer,
        _intermediates: &[rustls::pki_types::CertificateDer],
        _server_name: &rustls::pki_types::ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
