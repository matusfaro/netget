//! HTTP/HTTPS Proxy server implementation using MITM with LLM control
//!
//! This module implements a sophisticated proxy server with:
//! - Full MITM capabilities with certificate generation/loading
//! - Pass-through mode for HTTPS (no decryption, allow/block only)
//! - LLM-controlled filtering and modification of requests/responses
//! - Regex-based filtering for selective interception

pub mod actions;
pub mod cert_cache;
pub mod filter;
pub mod tls_mitm;

use crate::server::connection::ConnectionId;
use anyhow::{Context, Result};
use cert_cache::CertificateCache;
use filter::{
    CertificateMode, FullRequestInfo, HttpsConnectionAction, HttpsConnectionInfo,
    ProxyFilterConfig, RequestAction,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::{ActionResult, Server};
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::ProxyProtocol;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use actions::{PROXY_HTTPS_CONNECT_EVENT, PROXY_HTTP_REQUEST_EVENT};

use crate::console_debug;
use rcgen::{Certificate, CertificateParams, KeyPair};
use regex::Regex;
use serde_json::json;

/// HTTP/HTTPS Proxy server that intercepts and forwards requests via LLM
pub struct ProxyServer;

impl ProxyServer {
    /// Spawn HTTP Proxy server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let _ = status_tx.send("[INFO] @@@ spawn_with_llm_actions CALLED @@@".to_string());
        info!("Proxy server (action-based) starting on {}", listen_addr);
        let _ = status_tx.send(format!("[INFO] @@@ Proxy starting on {} @@@", listen_addr));

        // Get or initialize proxy filter configuration
        let mut config = app_state
            .get_proxy_filter_config(server_id)
            .await
            .unwrap_or_else(|| {
                info!("No proxy filter config found, using defaults (MITM with cert generation)");
                ProxyFilterConfig::default()
            });

        // Apply startup parameters if provided
        if let Some(ref params) = startup_params {
            let _ = status_tx.send(format!("[INFO] Applying proxy startup parameters"));

            // Parse certificate_mode
            if let Some(cert_mode_str) = params.get_optional_string("certificate_mode") {
                config.certificate_mode = match cert_mode_str.as_str() {
                    "generate" => CertificateMode::Generate,
                    "none" => CertificateMode::None,
                    "load_from_file" => {
                        let cert_path = params
                            .get_optional_string("cert_path")
                            .context("Missing cert_path for load_from_file mode")?;
                        let key_path = params
                            .get_optional_string("key_path")
                            .context("Missing key_path for load_from_file mode")?;
                        CertificateMode::LoadFromFile {
                            cert_path: cert_path.into(),
                            key_path: key_path.into(),
                        }
                    }
                    _ => {
                        warn!("Invalid certificate_mode: {}, using default", cert_mode_str);
                        config.certificate_mode
                    }
                };
                let _ = status_tx.send(format!(
                    "[INFO] Certificate mode: {:?}",
                    config.certificate_mode
                ));
            }

            // Parse filter modes
            if let Some(mode_str) = params.get_optional_string("request_filter_mode") {
                if let Ok(mode) = serde_json::from_value(json!(mode_str)) {
                    let _ = status_tx.send(format!("[INFO] Request filter mode: {mode:?}"));
                    config.request_filter_mode = mode;
                }
            }

            if let Some(mode_str) = params.get_optional_string("response_filter_mode") {
                if let Ok(mode) = serde_json::from_value(json!(mode_str)) {
                    let _ = status_tx.send(format!("[INFO] Response filter mode: {mode:?}"));
                    config.response_filter_mode = mode;
                }
            }

            if let Some(mode_str) = params.get_optional_string("https_connection_filter_mode") {
                if let Ok(mode) = serde_json::from_value(json!(mode_str)) {
                    let _ =
                        status_tx.send(format!("[INFO] HTTPS connection filter mode: {:?}", mode));
                    config.https_connection_filter_mode = mode;
                }
            }
        }

        // Generate or load certificate based on configuration
        let cert_cache: Option<Arc<CertificateCache>> = match &config.certificate_mode {
            CertificateMode::Generate => {
                info!("Generating self-signed CA certificate for MITM");
                let _ = status_tx.send("[INFO] Generating MITM CA certificate...".to_string());
                let (ca_cert, ca_key) = Self::generate_ca_certificate()?;
                Some(Arc::new(CertificateCache::new(ca_cert, ca_key)))
            }
            CertificateMode::LoadFromFile {
                cert_path,
                key_path,
            } => {
                info!(
                    "Loading CA certificate from {:?} and {:?}",
                    cert_path, key_path
                );
                let _ = status_tx.send(format!("[INFO] Loading CA cert from {:?}", cert_path));

                // Read certificate and key files
                let _cert_pem = std::fs::read_to_string(cert_path)
                    .context("Failed to read certificate file")?;
                let key_pem =
                    std::fs::read_to_string(key_path).context("Failed to read private key file")?;

                // Parse the key pair
                let key_pair =
                    KeyPair::from_pem(&key_pem).context("Failed to parse private key")?;

                // For loading existing certificates, we need to create a Certificate from PEM
                // rcgen doesn't have direct PEM parsing, so we'll use the same params and key
                let mut params = CertificateParams::default();
                params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
                params
                    .distinguished_name
                    .push(rcgen::DnType::CommonName, "NetGet MITM Proxy CA (Loaded)");

                let cert = params
                    .self_signed(&key_pair)
                    .context("Failed to create certificate")?;

                Some(Arc::new(CertificateCache::new(cert, key_pair)))
            }
            CertificateMode::None => {
                info!("Proxy running in pass-through mode (no MITM, origin certificates)");
                let _ = status_tx.send("[INFO] Proxy: pass-through mode (no MITM)".to_string());
                None
            }
        };

        // Save the config back to state
        app_state
            .set_proxy_filter_config(server_id, config.clone())
            .await;

        let protocol = Arc::new(ProxyProtocol::new());

        // Start TCP listener for proxy connections
        let listener = tokio::net::TcpListener::bind(listen_addr)
            .await
            .context("Failed to bind proxy listener")?;

        let actual_addr = listener
            .local_addr()
            .context("Failed to get local address")?;

        info!("Proxy server listening on {}", actual_addr);
        let _ = status_tx.send(format!("→ Proxy server listening on {}", actual_addr));

        if cert_cache.is_some() {
            let _ = status_tx
                .send("[INFO] MITM mode enabled - full HTTPS decryption and inspection".to_string());
        } else {
            let _ = status_tx.send("[INFO] Pass-through mode - HTTPS allow/block only".to_string());
        }

        // Spawn proxy handler task
        let _ = status_tx.send("[INFO] >>> Spawning proxy accept loop...".to_string());
        tokio::spawn(async move {
            let _ = status_tx.send("[INFO] >>> Proxy accept loop STARTED".to_string());
            loop {
                let _ = status_tx.send("[DEBUG] >>> Waiting for proxy connection...".to_string());
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let _ = status_tx.send(format!(
                            "[INFO] >>> ACCEPTED proxy connection from {}",
                            peer_addr
                        ));
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        console_debug!(
                            status_tx,
                            "Proxy connection {} from {}",
                            connection_id,
                            peer_addr
                        );

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr: actual_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let app_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let config_clone = config.clone();
                        let cert_cache_clone = cert_cache.clone();

                        // Handle each proxy connection in a separate task
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_proxy_connection(
                                stream,
                                peer_addr,
                                connection_id,
                                server_id,
                                cert_cache_clone,
                                config_clone,
                                llm_clone,
                                app_clone.clone(),
                                status_clone.clone(),
                                protocol_clone,
                            )
                            .await
                            {
                                error!("Proxy connection {} error: {}", connection_id, e);
                                let _ = status_clone.send(format!("✗ Proxy error: {}", e));
                            }

                            // Mark connection as closed
                            app_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_clone
                                .send(format!("✗ Proxy connection {} closed", connection_id));
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept proxy connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(actual_addr)
    }

    /// Handle a single proxy connection
    async fn handle_proxy_connection(
        mut stream: tokio::net::TcpStream,
        peer_addr: SocketAddr,
        connection_id: ConnectionId,
        server_id: ServerId,
        cert_cache: Option<Arc<CertificateCache>>,
        config: ProxyFilterConfig,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<ProxyProtocol>,
    ) -> Result<()> {
        use tokio::io::AsyncReadExt;

        eprintln!(
            ">>> PROXY: handle_proxy_connection called from {}",
            peer_addr
        );
        info!(
            "Proxy: handling connection {} from {}",
            connection_id, peer_addr
        );
        let _ = status_tx.send(format!(
            "[INFO] Proxy: handling connection from {}",
            peer_addr
        ));

        // Read the initial HTTP request
        let mut buffer = vec![0u8; 8192];

        eprintln!(">>> PROXY: about to read from connection {}", connection_id);
        let n = stream
            .read(&mut buffer)
            .await
            .context("Failed to read initial request")?;

        eprintln!(
            ">>> PROXY: received {} bytes from connection {}",
            n, connection_id
        );
        console_debug!(
            status_tx,
            "Proxy connection {} received {} bytes",
            connection_id,
            n
        );

        if n == 0 {
            debug!("Client closed connection before sending data");
            return Ok(()); // Client closed connection
        }

        let request_data = &buffer[..n];
        let request_str = String::from_utf8_lossy(request_data);

        debug!(
            "Proxy {} received request:\n{}",
            connection_id,
            if request_str.len() > 200 {
                format!(
                    "{}... ({} bytes total)",
                    &request_str[..200],
                    request_str.len()
                )
            } else {
                request_str.to_string()
            }
        );
        let _ = status_tx.send(format!("[DEBUG] Proxy {} parsing request", connection_id));

        // Parse the request line
        let first_line = request_str.lines().next().context("Empty request")?;

        console_debug!(status_tx, "Request line: {}", first_line);

        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() < 3 {
            error!("Invalid HTTP request line: {}", first_line);
            return Err(anyhow::anyhow!("Invalid HTTP request line"));
        }

        let method = parts[0];
        let uri = parts[1];

        console_debug!(status_tx, "Parsed: method={}, uri={}", method, uri);

        // Check if this is an HTTPS CONNECT request
        if method == "CONNECT" {
            // HTTPS tunneling request
            return Self::handle_https_connect(
                stream,
                uri,
                peer_addr,
                connection_id,
                server_id,
                cert_cache,
                config,
                llm_client,
                app_state,
                status_tx,
                protocol,
            )
            .await;
        } else {
            // Regular HTTP request
            return Self::handle_http_request(
                stream,
                request_data,
                method,
                uri,
                peer_addr,
                connection_id,
                server_id,
                config,
                llm_client,
                app_state,
                status_tx,
                protocol,
            )
            .await;
        }
    }

    /// Handle HTTPS CONNECT request (tunneling)
    async fn handle_https_connect(
        mut client_stream: tokio::net::TcpStream,
        uri: &str,
        peer_addr: SocketAddr,
        connection_id: ConnectionId,
        server_id: ServerId,
        cert_cache: Option<Arc<CertificateCache>>,
        config: ProxyFilterConfig,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<ProxyProtocol>,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let start_time = std::time::Instant::now();

        // Parse host:port from CONNECT uri
        let parts: Vec<&str> = uri.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid CONNECT uri: {}", uri));
        }

        let dest_host = parts[0];
        let dest_port: u16 = parts[1]
            .parse()
            .context("Invalid port in CONNECT request")?;

        // TRACE: Log full connection details (metadata only, no content in pass-through)
        trace!(
            "HTTPS CONNECT from {} to {}:{}",
            peer_addr,
            dest_host,
            dest_port
        );
        trace!("  SNI: {} (from CONNECT)", dest_host);
        trace!(
            "  Certificate mode: {:?}",
            if cert_cache.is_some() {
                "MITM"
            } else {
                "Pass-through"
            }
        );
        let _ = status_tx.send(format!(
            "[TRACE] HTTPS CONNECT {} -> {}:{} ({})",
            peer_addr,
            dest_host,
            dest_port,
            if cert_cache.is_some() {
                "MITM"
            } else {
                "pass-through"
            }
        ));

        if let Some(cache) = cert_cache {
            // MITM mode - full decryption and inspection
            info!("MITM mode: will decrypt and inspect HTTPS traffic for {}:{}", dest_host, dest_port);

            // Call MITM implementation
            return tls_mitm::perform_mitm(
                client_stream,
                dest_host,
                dest_port,
                peer_addr,
                connection_id,
                server_id,
                cache,
                config,
                llm_client,
                app_state,
                status_tx,
                protocol,
            )
            .await;
        }

        // Pass-through mode or fallback - no decryption
        // Note: SNI could be extracted from TLS handshake, but we use dest_host for now
        let client_addr_str = peer_addr.to_string();
        if config.should_intercept_https_connection(
            dest_host,
            dest_port,
            Some(dest_host), // SNI - could be extracted from TLS handshake
            &client_addr_str,
        ) {
            // Consult LLM about whether to allow this HTTPS connection
            let conn_info = HttpsConnectionInfo {
                destination_host: dest_host.to_string(),
                destination_port: dest_port,
                sni: Some(dest_host.to_string()),
                client_addr: client_addr_str,
            };

            info!(
                "Consulting LLM about HTTPS connection to {}:{}",
                dest_host, dest_port
            );

            // Consult LLM
            let action = Self::consult_llm_https_connection(
                &conn_info,
                server_id,
                &llm_client,
                &app_state,
                &protocol,
                &status_tx,
            )
            .await
            .unwrap_or_else(|e| {
                error!("LLM consultation failed: {}", e);
                let _ = status_tx.send(format!("✗ LLM error: {}", e));
                // Default to blocking on error for safety
                HttpsConnectionAction::Block {
                    reason: Some(format!("LLM consultation failed: {}", e)),
                }
            });

            match action {
                HttpsConnectionAction::Allow => {
                    info!(
                        "LLM allowed HTTPS connection to {}:{}",
                        dest_host, dest_port
                    );
                    let _ =
                        status_tx.send(format!("→ Allowed HTTPS to {}:{}", dest_host, dest_port));

                    // Establish connection to destination
                    let dest_addr = format!("{}:{}", dest_host, dest_port);
                    let mut dest_stream = tokio::net::TcpStream::connect(&dest_addr)
                        .await
                        .context("Failed to connect to destination")?;

                    // Send 200 Connection Established to client
                    client_stream
                        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await?;

                    // Bidirectional copy between client and destination
                    let (mut client_read, mut client_write) = client_stream.split();
                    let (mut dest_read, mut dest_write) = dest_stream.split();

                    let client_to_dest = tokio::io::copy(&mut client_read, &mut dest_write);
                    let dest_to_client = tokio::io::copy(&mut dest_read, &mut client_write);

                    // Run both directions concurrently
                    let (up_bytes, down_bytes) = tokio::join!(client_to_dest, dest_to_client);
                    let up_bytes = up_bytes.unwrap_or(0);
                    let down_bytes = down_bytes.unwrap_or(0);
                    let total_bytes = up_bytes + down_bytes;

                    let duration = start_time.elapsed();

                    // DEBUG: Access log (pass-through - no HTTP status)
                    debug!(
                        "[ACCESS] {} CONNECT {}:{} -> TUNNEL {} bytes ({} up, {} down) in {:?}",
                        peer_addr,
                        dest_host,
                        dest_port,
                        total_bytes,
                        up_bytes,
                        down_bytes,
                        duration
                    );
                    let _ = status_tx.send(format!(
                        "[DEBUG] [ACCESS] {} CONNECT {}:{} -> TUNNEL {} bytes",
                        peer_addr, dest_host, dest_port, total_bytes
                    ));

                    trace!("HTTPS tunnel closed: {} bytes transferred", total_bytes);

                    Ok(())
                }
                HttpsConnectionAction::Block { reason } => {
                    let duration = start_time.elapsed();
                    let reason_str = reason.clone().unwrap_or_default();

                    // DEBUG: Access log
                    debug!(
                        "[ACCESS] {} CONNECT {}:{} -> 403 {} in {:?}",
                        peer_addr,
                        dest_host,
                        dest_port,
                        reason_str.len(),
                        duration
                    );
                    let _ = status_tx.send(format!(
                        "[DEBUG] [ACCESS] {} CONNECT {}:{} -> 403 BLOCKED",
                        peer_addr, dest_host, dest_port
                    ));

                    // Send 403 Forbidden to client
                    let response = format!(
                        "HTTP/1.1 403 Forbidden\r\n\
                         Content-Type: text/plain\r\n\
                         Content-Length: {}\r\n\
                         \r\n\
                         {}",
                        reason_str.len(),
                        reason_str
                    );
                    client_stream.write_all(response.as_bytes()).await?;

                    Ok(())
                }
            }
        } else {
            // Filter mode is "none" or doesn't match - pass through without LLM
            trace!(
                "Pass-through HTTPS connection to {}:{} (no LLM consultation)",
                dest_host,
                dest_port
            );

            // Establish connection to destination
            let dest_addr = format!("{}:{}", dest_host, dest_port);
            let mut dest_stream = tokio::net::TcpStream::connect(&dest_addr)
                .await
                .context("Failed to connect to destination")?;

            // Send 200 Connection Established to client
            client_stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await?;

            // Bidirectional copy
            let (mut client_read, mut client_write) = client_stream.split();
            let (mut dest_read, mut dest_write) = dest_stream.split();

            let client_to_dest = tokio::io::copy(&mut client_read, &mut dest_write);
            let dest_to_client = tokio::io::copy(&mut dest_read, &mut client_write);

            let (up_bytes, down_bytes) = tokio::join!(client_to_dest, dest_to_client);
            let up_bytes = up_bytes.unwrap_or(0);
            let down_bytes = down_bytes.unwrap_or(0);
            let total_bytes = up_bytes + down_bytes;

            let duration = start_time.elapsed();

            // DEBUG: Access log
            debug!(
                "[ACCESS] {} CONNECT {}:{} -> TUNNEL {} bytes ({} up, {} down) in {:?}",
                peer_addr, dest_host, dest_port, total_bytes, up_bytes, down_bytes, duration
            );
            let _ = status_tx.send(format!(
                "[DEBUG] [ACCESS] {} CONNECT {}:{} -> TUNNEL {} bytes",
                peer_addr, dest_host, dest_port, total_bytes
            ));

            Ok(())
        }
    }

    /// Handle regular HTTP request (no TLS)
    async fn handle_http_request(
        mut client_stream: tokio::net::TcpStream,
        request_data: &[u8],
        method: &str,
        uri: &str,
        peer_addr: SocketAddr,
        _connection_id: ConnectionId,
        server_id: ServerId,
        config: ProxyFilterConfig,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<ProxyProtocol>,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let start_time = std::time::Instant::now();

        // Parse HTTP request
        let request_str = String::from_utf8_lossy(request_data);
        let mut headers = HashMap::new();
        let mut body_start = 0;

        // Parse headers
        for (i, line) in request_str.lines().enumerate() {
            if i == 0 {
                continue; // Skip request line
            }
            if line.is_empty() {
                // End of headers
                body_start =
                    request_str[..request_str.find("\r\n\r\n").unwrap_or(request_str.len())].len()
                        + 4;
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

        // Extract host from headers or URI
        let host = headers
            .get("Host")
            .map(|s| s.as_str())
            .or_else(|| {
                // Try to extract from absolute URI
                if uri.starts_with("http://") {
                    uri.strip_prefix("http://")
                        .and_then(|s| s.split('/').next())
                } else {
                    None
                }
            })
            .unwrap_or("unknown");

        let path = if uri.starts_with("http://") {
            // Absolute URI - extract path
            uri.find('/').map(|pos| &uri[pos..]).unwrap_or("/")
        } else {
            uri
        };

        // TRACE: Log full request details
        trace!("Proxy HTTP request: {} {} from {}", method, uri, peer_addr);
        trace!("  Headers: {:#?}", headers);
        if !body.is_empty() {
            if let Ok(body_str) = std::str::from_utf8(body) {
                trace!("  Body: {}", body_str);
            } else {
                trace!("  Body: {} bytes (binary)", body.len());
            }
        }
        let _ = status_tx.send(format!(
            "[TRACE] Proxy request: {} {} from {} ({} bytes body)",
            method,
            uri,
            peer_addr,
            body.len()
        ));

        // Check if we should intercept this request
        if config.should_intercept_request(host, path, method, &headers, body) {
            info!("Request matches filters, consulting LLM");
            let _ = status_tx.send("[DEBUG] Request matched filters, consulting LLM".to_string());

            // Build request info for LLM
            let request_info = FullRequestInfo {
                method: method.to_string(),
                url: uri.to_string(),
                path: path.to_string(),
                host: host.to_string(),
                headers: headers.clone(),
                body: body.to_vec(),
                client_addr: peer_addr.to_string(),
            };

            // Consult LLM
            let action = Self::consult_llm_http_request(
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
                let _ = status_tx.send(format!("✗ LLM error: {}", e));
                // Default to passing through on error
                RequestAction::Pass
            });

            match action {
                RequestAction::Pass => {
                    info!("LLM passed request through");
                    // Forward request to destination
                    Self::forward_http_request(
                        client_stream,
                        request_data,
                        host,
                        method,
                        uri,
                        peer_addr,
                        start_time,
                        status_tx,
                    )
                    .await
                }
                RequestAction::Block { status, body } => {
                    let duration = start_time.elapsed();
                    let body_len = body.len();

                    // DEBUG: Access log
                    debug!(
                        "[ACCESS] {} {} {} -> {} {} bytes in {:?}",
                        peer_addr, method, uri, status, body_len, duration
                    );
                    let _ = status_tx.send(format!(
                        "[DEBUG] [ACCESS] {} {} {} -> {} {} bytes",
                        peer_addr, method, uri, status, body_len
                    ));

                    // TRACE: Full response details
                    trace!(
                        "Blocking response: status={}, body_len={}",
                        status,
                        body_len
                    );
                    trace!("  Response body: {}", body);

                    let response = format!(
                        "HTTP/1.1 {} Blocked\r\n\
                         Content-Type: text/plain\r\n\
                         Content-Length: {}\r\n\
                         \r\n\
                         {}",
                        status, body_len, body
                    );
                    client_stream.write_all(response.as_bytes()).await?;
                    Ok(())
                }
                ref modify_action @ RequestAction::Modify { .. } => {
                    info!("LLM requested modifications, applying...");
                    let _ = status_tx.send("[DEBUG] Applying request modifications".to_string());

                    // Apply modifications
                    let modified_request =
                        Self::apply_request_modifications(request_data, modify_action)
                            .unwrap_or_else(|e| {
                                error!("Failed to apply modifications: {}", e);
                                let _ = status_tx.send(format!("✗ Modification error: {}", e));
                                request_data.to_vec()
                            });

                    // Forward modified request
                    Self::forward_http_request(
                        client_stream,
                        &modified_request,
                        host,
                        method,
                        uri,
                        peer_addr,
                        start_time,
                        status_tx,
                    )
                    .await
                }
            }
        } else {
            // Pass through without LLM consultation
            info!("Request doesn't match filters, passing through");
            Self::forward_http_request(
                client_stream,
                request_data,
                host,
                method,
                uri,
                peer_addr,
                start_time,
                status_tx,
            )
            .await
        }
    }

    /// Forward HTTP request to destination and return response to client
    async fn forward_http_request(
        mut client_stream: tokio::net::TcpStream,
        request_data: &[u8],
        host: &str,
        method: &str,
        uri: &str,
        peer_addr: SocketAddr,
        start_time: std::time::Instant,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Parse host:port
        let (dest_host, dest_port) = if let Some(colon_pos) = host.find(':') {
            (
                &host[..colon_pos],
                host[colon_pos + 1..].parse().unwrap_or(80),
            )
        } else {
            (host, 80)
        };

        info!("Forwarding to {}:{}", dest_host, dest_port);

        // Connect to destination
        let dest_addr = format!("{}:{}", dest_host, dest_port);
        let mut dest_stream = tokio::net::TcpStream::connect(&dest_addr)
            .await
            .context(format!("Failed to connect to {}", dest_addr))?;

        // Send request to destination
        dest_stream.write_all(request_data).await?;
        trace!(
            "Sent {} bytes to upstream {}",
            request_data.len(),
            dest_addr
        );

        // Read response from destination
        let mut response_buffer = Vec::new();
        let mut temp_buffer = [0u8; 8192];
        let mut content_length: Option<usize> = None;
        let mut headers_complete = false;
        let mut headers_end = 0;

        loop {
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(5),
                dest_stream.read(&mut temp_buffer),
            )
            .await
            {
                Ok(Ok(0)) => break, // EOF
                Ok(Ok(n)) => {
                    response_buffer.extend_from_slice(&temp_buffer[..n]);

                    // Parse Content-Length if we haven't yet
                    if !headers_complete && response_buffer.len() > 4 {
                        let response_str = String::from_utf8_lossy(&response_buffer);
                        if let Some(header_end) = response_str.find("\r\n\r\n") {
                            headers_complete = true;
                            headers_end = header_end + 4;

                            // Extract Content-Length
                            for line in response_str[..header_end].lines() {
                                if line.to_lowercase().starts_with("content-length:") {
                                    if let Some(len_str) = line.split(':').nth(1) {
                                        content_length = len_str.trim().parse().ok();
                                    }
                                }
                            }
                        }
                    }

                    // Check if we have full response
                    if headers_complete {
                        if let Some(len) = content_length {
                            if response_buffer.len() >= headers_end + len {
                                break; // Have complete response
                            }
                        } else {
                            // No Content-Length, wait a bit more
                            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                            break;
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("Error reading from destination: {}", e);
                    break;
                }
                Err(_) => {
                    debug!("Timeout reading from destination, proceeding with what we have");
                    break;
                }
            }
        }

        // Parse response status for access log
        let status =
            if let Some(first_line) = String::from_utf8_lossy(&response_buffer).lines().next() {
                first_line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0)
            } else {
                0
            };

        let duration = start_time.elapsed();

        // DEBUG: Access log
        debug!(
            "[ACCESS] {} {} {} -> {} {} bytes in {:?}",
            peer_addr,
            method,
            uri,
            status,
            response_buffer.len(),
            duration
        );
        let _ = status_tx.send(format!(
            "[DEBUG] [ACCESS] {} {} {} -> {} {} bytes",
            peer_addr,
            method,
            uri,
            status,
            response_buffer.len()
        ));

        // TRACE: Full response details
        if response_buffer.len() > 0 {
            let response_str = String::from_utf8_lossy(&response_buffer);
            let lines: Vec<&str> = response_str.lines().collect();
            if !lines.is_empty() {
                trace!("Response status line: {}", lines[0]);
                trace!("Response headers:");
                for line in &lines[1..] {
                    if line.is_empty() {
                        break;
                    }
                    trace!("  {}", line);
                }
            }
        }

        // Send response back to client
        client_stream.write_all(&response_buffer).await?;
        trace!("Forwarded {} bytes to client", response_buffer.len());

        Ok(())
    }

    /// Consult LLM about an HTTP request
    async fn consult_llm_http_request(
        request_info: &FullRequestInfo,
        server_id: ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        protocol: &Arc<ProxyProtocol>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<RequestAction> {
        let _ = status_tx.send("[DEBUG] Consulting LLM about HTTP request...".to_string());

        // Format request info for event description
        let _body_preview = if request_info.body.len() > 500 {
            format!(
                "{}... ({} bytes total)",
                String::from_utf8_lossy(&request_info.body[..500]),
                request_info.body.len()
            )
        } else {
            String::from_utf8_lossy(&request_info.body).to_string()
        };

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
            None, // TODO: Add connection_id for proxy requests
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

    /// Consult LLM about an HTTPS connection (pass-through mode)
    async fn consult_llm_https_connection(
        conn_info: &HttpsConnectionInfo,
        server_id: ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        protocol: &Arc<ProxyProtocol>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<HttpsConnectionAction> {
        let _ = status_tx.send("[DEBUG] Consulting LLM about HTTPS connection...".to_string());

        // Create HTTPS CONNECT event
        let event = Event::new(
            &PROXY_HTTPS_CONNECT_EVENT,
            json!({
                "destination_host": conn_info.destination_host,
                "destination_port": conn_info.destination_port,
                "sni": conn_info.sni.as_ref().unwrap_or(&String::new()),
            }),
        );

        let execution_result = call_llm(
            llm_client,
            app_state,
            server_id,
            None, // TODO: Add connection_id for proxy responses
            &event,
            protocol.as_ref() as &dyn Server,
        )
        .await
        .context("LLM request failed")?;

        // Extract HTTPS connection action from protocol results
        for result in execution_result.protocol_results {
            if let ActionResult::Output(bytes) = result {
                // Deserialize the HttpsConnectionAction
                let action: HttpsConnectionAction = serde_json::from_slice(&bytes)
                    .context("Failed to deserialize HttpsConnectionAction")?;
                return Ok(action);
            }
        }

        // Default to allow if no explicit action found
        Ok(HttpsConnectionAction::Allow)
    }

    /// Apply modifications to HTTP request
    fn apply_request_modifications(
        request_data: &[u8],
        modifications: &RequestAction,
    ) -> Result<Vec<u8>> {
        if let RequestAction::Modify {
            headers,
            remove_headers,
            new_path,
            query_params: _,
            new_body,
            body_replacements,
        } = modifications
        {
            // Find the \r\n\r\n separator between headers and body
            let separator = b"\r\n\r\n";
            let separator_pos = request_data
                .windows(separator.len())
                .position(|window| window == separator);

            if separator_pos.is_none() {
                return Ok(request_data.to_vec());
            }

            let headers_end = separator_pos.unwrap();
            let body_start = headers_end + 4; // After \r\n\r\n

            // Extract headers section as string
            let headers_bytes = &request_data[..headers_end];
            let headers_str = String::from_utf8_lossy(headers_bytes);
            let header_lines: Vec<&str> = headers_str.lines().collect();

            if header_lines.is_empty() {
                return Ok(request_data.to_vec());
            }

            // Parse original request line
            let mut request_line = header_lines[0].to_string();
            if let Some(path) = new_path {
                let parts: Vec<&str> = header_lines[0].split_whitespace().collect();
                if parts.len() >= 3 {
                    let method = parts[0];
                    let version = parts[2];
                    request_line = format!("{} {} {}", method, path, version);
                }
            }

            // Build headers map
            let mut headers_map = HashMap::new();
            for line in &header_lines[1..] {
                if let Some(colon_pos) = line.find(':') {
                    let name = line[..colon_pos].trim().to_string();
                    let value = line[colon_pos + 1..].trim().to_string();
                    headers_map.insert(name, value);
                }
            }

            // Remove headers
            if let Some(remove) = remove_headers {
                for header_name in remove {
                    headers_map.remove(header_name);
                }
            }

            // Add/modify headers
            if let Some(add_headers) = headers {
                for (name, value) in add_headers {
                    headers_map.insert(name.clone(), value.clone());
                }
            }

            // Get body as bytes, then convert to string for modification
            let original_body = if body_start < request_data.len() {
                &request_data[body_start..]
            } else {
                &[]
            };

            let mut body = String::from_utf8_lossy(original_body).to_string();

            // Apply body modifications
            if let Some(new_body_text) = new_body {
                body = new_body_text.clone();
            }

            if let Some(replacements) = body_replacements {
                for replacement in replacements {
                    if let Ok(re) = Regex::new(&replacement.pattern) {
                        body = re
                            .replace_all(&body, replacement.replacement.as_str())
                            .to_string();
                    }
                }
            }

            // Update Content-Length to match new body size
            if !body.is_empty() {
                headers_map.insert("Content-Length".to_string(), body.len().to_string());
            } else if new_body.is_some() || body_replacements.is_some() {
                // Body was explicitly modified to empty
                headers_map.insert("Content-Length".to_string(), "0".to_string());
            }

            // Reconstruct request with proper \r\n line endings
            let mut result = Vec::new();
            result.extend_from_slice(request_line.as_bytes());
            result.extend_from_slice(b"\r\n");

            for (name, value) in headers_map {
                result.extend_from_slice(name.as_bytes());
                result.extend_from_slice(b": ");
                result.extend_from_slice(value.as_bytes());
                result.extend_from_slice(b"\r\n");
            }

            result.extend_from_slice(b"\r\n");
            if !body.is_empty() {
                result.extend_from_slice(body.as_bytes());
            }

            Ok(result)
        } else {
            Ok(request_data.to_vec())
        }
    }

    /// Generate a self-signed CA certificate for MITM proxy
    fn generate_ca_certificate() -> Result<(Certificate, KeyPair)> {
        let mut params = CertificateParams::default();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "NetGet MITM Proxy CA");

        let key_pair = KeyPair::generate()?;
        let cert = params.self_signed(&key_pair)?;

        Ok((cert, key_pair))
    }
}
