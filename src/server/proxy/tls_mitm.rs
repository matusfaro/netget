//! TLS MITM (Man-in-the-Middle) orchestration for HTTPS proxy
//!
//! Handles the complete MITM flow:
//! 1. Accept TLS connection from client (using dynamically generated cert)
//! 2. Connect to upstream server with TLS
//! 3. Proxy decrypted HTTP traffic through LLM filtering
//! 4. Re-encrypt and forward to both sides

use super::cert_cache::CertificateCache;
use super::filter::{FullRequestInfo, ProxyFilterConfig, RequestAction, ResponseAction};
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::ProxyProtocol;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use anyhow::{Context, Result};
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{debug, error, info, trace, warn};

use super::actions::PROXY_HTTP_REQUEST_EVENT;

/// Perform full TLS MITM interception
///
/// This function:
/// 1. Sends "200 Connection Established" to client
/// 2. Performs TLS handshake with client (using generated cert for domain)
/// 3. Connects to upstream server with TLS
/// 4. Proxies HTTP requests/responses through LLM filtering
pub async fn perform_mitm(
    mut client_stream: TcpStream,
    dest_host: &str,
    dest_port: u16,
    peer_addr: SocketAddr,
    _connection_id: ConnectionId,
    server_id: ServerId,
    cert_cache: Arc<CertificateCache>,
    _config: ProxyFilterConfig,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<ProxyProtocol>,
) -> Result<()> {
    let start_time = std::time::Instant::now();

    info!(
        "Starting MITM for {}:{} from {}",
        dest_host, dest_port, peer_addr
    );
    let _ = status_tx.send(format!(
        "[INFO] MITM mode: intercepting {}:{} from {}",
        dest_host, dest_port, peer_addr
    ));

    // Step 1: Send "200 Connection Established" to client BEFORE TLS handshake
    client_stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .context("Failed to send 200 to client")?;

    trace!("Sent 200 Connection Established to client");

    // Step 2: Generate or retrieve leaf certificate for this domain
    let (leaf_cert, leaf_key) = cert_cache
        .get_or_generate(dest_host)
        .await
        .context("Failed to generate leaf certificate")?;

    debug!("Generated/retrieved certificate for domain '{}'", dest_host);

    // Step 3: Create TLS server config with the leaf certificate
    let (cert_chain, private_key) = CertificateCache::to_rustls_cert(&leaf_cert, &leaf_key)?;

    // Install crypto provider (ring) if not already installed
    let _ = rustls::crypto::ring::default_provider().install_default();

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .context("Failed to create server TLS config")?;

    let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

    // Step 4: Perform TLS handshake with client
    let mut client_tls_stream = tls_acceptor
        .accept(client_stream)
        .await
        .context("TLS handshake with client failed")?;

    info!("TLS handshake with client completed for {}", dest_host);
    let _ = status_tx.send(format!(
        "[INFO] Client TLS handshake complete for {}",
        dest_host
    ));

    // Step 5: Connect to upstream server
    let dest_addr = format!("{}:{}", dest_host, dest_port);
    let upstream_tcp = TcpStream::connect(&dest_addr)
        .await
        .context(format!("Failed to connect to upstream {}", dest_addr))?;

    debug!("Connected to upstream server {}", dest_addr);

    // Step 6: Create TLS client config for upstream connection
    let root_store = rustls::RootCertStore::from_iter(
        webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .cloned()
    );

    let client_config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let tls_connector = TlsConnector::from(Arc::new(client_config));

    // Step 7: Perform TLS handshake with upstream server
    let server_name = ServerName::try_from(dest_host.to_string())
        .context("Invalid server name for TLS")?;

    let mut upstream_tls_stream = tls_connector
        .connect(server_name, upstream_tcp)
        .await
        .context("TLS handshake with upstream server failed")?;

    info!("TLS handshake with upstream server completed for {}", dest_host);
    let _ = status_tx.send(format!(
        "[INFO] Upstream TLS handshake complete for {}",
        dest_host
    ));

    // Step 8: Proxy HTTP traffic through LLM
    // Now we have two TLS streams: client_tls_stream (client) and upstream_tls_stream (upstream)
    // We read HTTP requests from client, optionally modify via LLM, forward to upstream
    // We read HTTP responses from upstream, optionally modify via LLM, send to client

    // For now, implement simple bidirectional proxying
    // TODO: Parse HTTP requests/responses and apply LLM filtering

    let _ = status_tx.send(format!(
        "[INFO] Starting bidirectional HTTPS proxy for {}:{}",
        dest_host, dest_port
    ));

    // Read first HTTP request from client
    let mut request_buffer = vec![0u8; 8192];
    let request_len = client_tls_stream
        .read(&mut request_buffer)
        .await
        .context("Failed to read HTTP request from client")?;

    if request_len == 0 {
        warn!("Client closed connection immediately after TLS handshake");
        return Ok(());
    }

    let request_data = &request_buffer[..request_len];
    let request_str = String::from_utf8_lossy(request_data);

    trace!("Received HTTP request over TLS ({} bytes):\n{}", request_len,
        if request_str.len() > 500 {
            format!("{}...", &request_str[..500])
        } else {
            request_str.to_string()
        }
    );

    // Parse HTTP request
    let first_line = request_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() >= 3 {
        let method = parts[0];
        let path = parts[1];

        info!("MITM: {} {} via HTTPS to {}", method, path, dest_host);
        let _ = status_tx.send(format!(
            "[INFO] MITM request: {} {} -> {}",
            method, path, dest_host
        ));

        // Parse headers
        let mut headers = HashMap::new();
        let mut body_start = 0;

        for (i, line) in request_str.lines().enumerate() {
            if i == 0 {
                continue; // Skip request line
            }
            if line.is_empty() {
                // End of headers
                body_start = request_str[..request_str.find("\r\n\r\n").unwrap_or(request_str.len())].len() + 4;
                break;
            }
            if let Some(colon_pos) = line.find(':') {
                let name = line[..colon_pos].trim().to_string();
                let value = line[colon_pos + 1..].trim().to_string();
                headers.insert(name, value);
            }
        }

        let body = if body_start < request_data.len() {
            &request_data[body_start..]
        } else {
            &[]
        };

        // Build request info for LLM
        let request_info = FullRequestInfo {
            method: method.to_string(),
            url: format!("https://{}{}", dest_host, path),
            path: path.to_string(),
            host: dest_host.to_string(),
            headers: headers.clone(),
            body: body.to_vec(),
            client_addr: peer_addr.to_string(),
        };

        // Consult LLM (simplified - just check if we should pass through)
        let action = consult_llm_for_request(
            &request_info,
            server_id,
            &llm_client,
            &app_state,
            &protocol,
            &status_tx,
        )
        .await
        .unwrap_or_else(|e| {
            error!("LLM consultation failed: {}", e);
            RequestAction::Pass // Default to pass on error
        });

        match action {
            RequestAction::Pass => {
                // Forward request to upstream as-is
                upstream_tls_stream
                    .write_all(request_data)
                    .await
                    .context("Failed to forward request to upstream")?;

                trace!("Forwarded request to upstream server");
            }
            RequestAction::Block { status, body } => {
                // Return error response to client
                let response = format!(
                    "HTTP/1.1 {} Blocked\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );

                client_tls_stream
                    .write_all(response.as_bytes())
                    .await
                    .context("Failed to send blocked response to client")?;

                info!("Blocked MITM request with status {}", status);
                return Ok(());
            }
            RequestAction::Modify { .. } => {
                // TODO: Implement request modification
                warn!("Request modification not yet implemented for MITM, passing through");
                upstream_tls_stream
                    .write_all(request_data)
                    .await
                    .context("Failed to forward request to upstream")?;
            }
        }

        // Read response from upstream
        let mut response_buffer = vec![0u8; 8192];
        let response_len = upstream_tls_stream
            .read(&mut response_buffer)
            .await
            .context("Failed to read response from upstream")?;

        if response_len == 0 {
            warn!("Upstream server closed connection");
            return Ok(());
        }

        let response_data = &response_buffer[..response_len];
        trace!("Received response from upstream ({} bytes)", response_len);

        // TODO: Parse response and consult LLM for response filtering
        // For now, just forward the response

        client_tls_stream
            .write_all(response_data)
            .await
            .context("Failed to send response to client")?;

        trace!("Sent response to client");

        // After first request/response, switch to bidirectional copy
        // This handles keep-alive connections and additional requests
        let (mut client_read, mut client_write) = tokio::io::split(client_tls_stream);
        let (mut upstream_read, mut upstream_write) = tokio::io::split(upstream_tls_stream);

        let client_to_upstream = tokio::io::copy(&mut client_read, &mut upstream_write);
        let upstream_to_client = tokio::io::copy(&mut upstream_read, &mut client_write);

        let (up_bytes, down_bytes) = tokio::join!(client_to_upstream, upstream_to_client);
        let up_bytes = up_bytes.unwrap_or(0) + request_len as u64;
        let down_bytes = down_bytes.unwrap_or(0) + response_len as u64;

        let duration = start_time.elapsed();

        debug!(
            "[ACCESS] {} MITM {}:{} -> {} bytes ({} up, {} down) in {:?}",
            peer_addr, dest_host, dest_port,
            up_bytes + down_bytes, up_bytes, down_bytes, duration
        );

        let _ = status_tx.send(format!(
            "[DEBUG] [ACCESS] {} MITM {}:{} -> {} bytes total",
            peer_addr, dest_host, dest_port,
            up_bytes + down_bytes
        ));

    } else {
        warn!("Invalid HTTP request in MITM mode: {}", first_line);
        return Err(anyhow::anyhow!("Invalid HTTP request"));
    }

    Ok(())
}

/// Consult LLM about an HTTP request in MITM mode
async fn consult_llm_for_request(
    request_info: &FullRequestInfo,
    server_id: ServerId,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    protocol: &Arc<ProxyProtocol>,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<RequestAction> {
    use crate::llm::actions::protocol_trait::{ActionResult, Server};
    use serde_json::json;

    let _ = status_tx.send("[DEBUG] Consulting LLM about MITM request...".to_string());

    // Create HTTP request event
    let event = Event::new(
        &PROXY_HTTP_REQUEST_EVENT,
        json!({
            "method": request_info.method,
            "url": request_info.url,
            "host": request_info.host,
            "path": request_info.path,
        }),
    );

    let execution_result = call_llm(
        llm_client,
        app_state,
        server_id,
        None, // TODO: Add connection_id
        &event,
        protocol.as_ref() as &dyn Server,
    )
    .await
    .context("LLM request failed")?;

    // Extract request action from protocol results
    for result in execution_result.protocol_results {
        if let ActionResult::Output(bytes) = result {
            // Deserialize the RequestAction
            let action: RequestAction = serde_json::from_slice(&bytes)
                .context("Failed to deserialize RequestAction")?;
            return Ok(action);
        }
    }

    // Default to pass if no explicit action found
    Ok(RequestAction::Pass)
}
