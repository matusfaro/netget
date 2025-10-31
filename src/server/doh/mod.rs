//! DNS-over-HTTPS (DoH) server implementation
//!
//! Implements RFC 8484 DNS-over-HTTPS protocol using hickory-dns, hyper, and rustls.
//! The LLM controls DNS responses while NetGet handles the HTTPS transport layer.

pub mod actions;

use crate::protocol::Event;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::action_helper::call_llm;
use crate::server::DohProtocol;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use actions::DOH_QUERY_EVENT;
use anyhow::{Context, Result};
use hickory_proto::op::Message as DnsMessage;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http2;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, trace, warn};

/// DNS-over-HTTPS server
pub struct DohServer {
    bind_addr: SocketAddr,
}

impl DohServer {
    /// Create a new DoH server
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self { bind_addr }
    }

    /// Spawn the DoH server
    pub async fn spawn(
        bind_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let server = Self::new(bind_addr);

        // Generate TLS configuration (use default self-signed cert)
        let tls_config = crate::server::tls_cert_manager::generate_default_tls_config()
            .context("Failed to generate TLS configuration")?;

        info!("Starting DoH server on {}", bind_addr);
        let _ = status_tx.send(format!("[INFO] Starting DoH server on {}", bind_addr));

        let handle = tokio::spawn(async move {
            if let Err(e) = server.run(tls_config, llm_client, app_state, server_id, status_tx).await {
                error!("DoH server error: {}", e);
            }
        });

        Ok(handle)
    }

    /// Run the DoH server
    async fn run(
        self,
        tls_config: Arc<rustls::ServerConfig>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let listener = TcpListener::bind(self.bind_addr)
            .await
            .context("Failed to bind DoH TCP listener")?;

        let acceptor = TlsAcceptor::from(tls_config);

        info!("DoH server listening on {}", self.bind_addr);
        let _ = status_tx.send(format!("[INFO] DoH server listening on {}", self.bind_addr));

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    debug!("DoH TCP connection from {}", peer_addr);
                    let _ = status_tx.send(format!("[DEBUG] DoH TCP connection from {}", peer_addr));

                    let acceptor = acceptor.clone();
                    let llm_client = llm_client.clone();
                    let app_state = app_state.clone();
                    let status_tx = status_tx.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(
                            stream,
                            peer_addr,
                            acceptor,
                            llm_client,
                            app_state,
                            server_id,
                            status_tx,
                        )
                        .await
                        {
                            error!("DoH connection error from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to accept DoH TCP connection: {}", e);
                    let _ = status_tx.send(format!("[WARN] Failed to accept DoH TCP connection: {}", e));
                }
            }
        }
    }

    /// Handle a single DoH connection
    async fn handle_connection(
        stream: tokio::net::TcpStream,
        peer_addr: SocketAddr,
        acceptor: TlsAcceptor,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Perform TLS handshake
        let tls_stream = acceptor
            .accept(stream)
            .await
            .context("TLS handshake failed")?;

        debug!("DoH TLS handshake complete with {}", peer_addr);
        let _ = status_tx.send(format!("[DEBUG] DoH TLS handshake complete with {}", peer_addr));

        // Wrap in TokioIo for hyper compatibility
        let io = TokioIo::new(tls_stream);


        // Create service closure
        let service = service_fn(move |req: Request<hyper::body::Incoming>| {
            let llm_client = llm_client.clone();
            let app_state = app_state.clone();
            let status_tx = status_tx.clone();

            async move {
                Self::handle_request(
                    req,
                    peer_addr,
                    server_id,
                    llm_client,
                    app_state,
                    status_tx,
                )
                .await
            }
        });

        // Serve HTTP/2
        let result = http2::Builder::new(hyper_util::rt::TokioExecutor::new())
            .serve_connection(io, service)
            .await;

        if let Err(e) = result {
            debug!("DoH HTTP/2 connection error: {}", e);
        }

        Ok(())
    }

    /// Handle a single DoH HTTP request
    async fn handle_request(
        req: Request<hyper::body::Incoming>,
        peer_addr: SocketAddr,
        server_id: ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        let method = req.method().clone();
        let uri = req.uri().clone();

        debug!("DoH request: {} {}", method, uri);
        let _ = status_tx.send(format!("[DEBUG] DoH request: {} {}", method, uri));

        // Extract DNS query based on method
        let dns_bytes = match method {
            Method::GET => {
                // Extract DNS query from ?dns= parameter (base64url encoded)
                let query = uri.query().unwrap_or("");
                let mut dns_param = None;

                for param in query.split('&') {
                    if let Some(value) = param.strip_prefix("dns=") {
                        dns_param = Some(value);
                        break;
                    }
                }

                match dns_param {
                    Some(encoded) => {
                        // Decode base64url
                        match base64_url_decode(encoded) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                warn!("Invalid base64url DNS query: {}", e);
                                let _ = status_tx.send(format!("[WARN] Invalid base64url DNS query: {}", e));
                                return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid DNS query encoding"));
                            }
                        }
                    }
                    None => {
                        warn!("Missing dns= parameter in GET request");
                        let _ = status_tx.send("[WARN] Missing dns= parameter in DoH GET request".to_string());
                        return Ok(error_response(StatusCode::BAD_REQUEST, "Missing dns parameter"));
                    }
                }
            }
            Method::POST => {
                // Check Content-Type
                if let Some(content_type) = req.headers().get("content-type") {
                    if content_type != "application/dns-message" {
                        warn!("Invalid Content-Type: {:?}", content_type);
                        let _ = status_tx.send(format!("[WARN] Invalid DoH Content-Type: {:?}", content_type));
                        return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid Content-Type"));
                    }
                } else {
                    warn!("Missing Content-Type header");
                    let _ = status_tx.send("[WARN] Missing Content-Type in DoH POST".to_string());
                    return Ok(error_response(StatusCode::BAD_REQUEST, "Missing Content-Type"));
                }

                // Read request body
                let body = req.collect().await?.to_bytes();
                body.to_vec()
            }
            _ => {
                warn!("Unsupported DoH method: {}", method);
                let _ = status_tx.send(format!("[WARN] Unsupported DoH method: {}", method));
                return Ok(error_response(StatusCode::METHOD_NOT_ALLOWED, "Only GET and POST are supported"));
            }
        };

        debug!("DoH received {} bytes", dns_bytes.len());
        let _ = status_tx.send(format!("[DEBUG] DoH received {} bytes", dns_bytes.len()));

        trace!("DoH DNS query hex: {}", hex::encode(&dns_bytes));
        let _ = status_tx.send(format!("[TRACE] DoH DNS query hex: {}", hex::encode(&dns_bytes)));

        // Parse DNS query
        let dns_message = match DnsMessage::from_vec(&dns_bytes) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to parse DNS message: {}", e);
                let _ = status_tx.send(format!("[ERROR] Failed to parse DoH DNS message: {}", e));
                return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid DNS message"));
            }
        };

        // Extract query information
        let queries = dns_message.queries();
        if queries.is_empty() {
            warn!("DoH DNS message has no queries");
            let _ = status_tx.send("[WARN] DoH DNS message has no queries".to_string());
            return Ok(error_response(StatusCode::BAD_REQUEST, "No DNS queries"));
        }

        let query = &queries[0];
        let domain = query.name().to_utf8();
        let query_type = format!("{:?}", query.query_type());
        let query_id = dns_message.id();

        info!("DoH query: {} {} (ID: {})", domain, query_type, query_id);
        let _ = status_tx.send(format!("[INFO] DoH query: {} {} (ID: {})", domain, query_type, query_id));

        // Create event for LLM
        let event = Event::new(&DOH_QUERY_EVENT, json!({
            "query_id": query_id,
            "domain": domain,
            "query_type": query_type,
            "peer_addr": peer_addr.to_string(),
            "method": method.to_string(),
        }));

        // Get protocol actions
        let protocol = Arc::new(DohProtocol::new());

        debug!("DoH calling LLM for query from {}", peer_addr);
        let _ = status_tx.send(format!("[DEBUG] DoH calling LLM for query from {}", peer_addr));

        // Call LLM
        let execution_result = match call_llm(
            &llm_client,
            &app_state,
            server_id,
            None,
            &event,
            protocol.as_ref(),
        ).await {
            Ok(result) => result,
            Err(e) => {
                error!("DoH LLM call failed: {}", e);
                let _ = status_tx.send(format!("[ERROR] DoH LLM call failed: {}", e));
                return Ok(error_response(StatusCode::INTERNAL_SERVER_ERROR, "LLM error"));
            }
        };

        // Display messages from LLM
        for message in &execution_result.messages {
            info!("{}", message);
            let _ = status_tx.send(format!("[INFO] {}", message));
        }

        debug!("DoH got {} protocol results", execution_result.protocol_results.len());
        let _ = status_tx.send(format!("[DEBUG] DoH got {} protocol results", execution_result.protocol_results.len()));

        // Execute actions from LLM response
        for protocol_result in &execution_result.protocol_results {
            use crate::llm::actions::protocol_trait::ActionResult;
            match protocol_result {
                ActionResult::Custom { data, .. } => {
                    if let Some(output_data) = data.get("output_data").and_then(|v| v.as_str()) {
                        // Decode hex DNS response
                        if let Ok(response_bytes) = hex::decode(output_data) {
                            debug!("DoH sending {} bytes response", response_bytes.len());
                            let _ = status_tx.send(format!("[DEBUG] DoH sending {} bytes", response_bytes.len()));

                            trace!("DoH response hex: {}", output_data);
                            let _ = status_tx.send(format!("[TRACE] DoH response hex: {}", output_data));

                            // Return DNS response with correct Content-Type
                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .header("Content-Type", "application/dns-message")
                                .header("Content-Length", response_bytes.len())
                                .body(Full::new(Bytes::from(response_bytes)))
                                .unwrap());
                        }
                    }
                }
                ActionResult::NoAction => {
                    // Ignore query - return empty response
                    debug!("DoH query ignored by LLM");
                    let _ = status_tx.send("[DEBUG] DoH query ignored by LLM".to_string());
                    return Ok(error_response(StatusCode::NOT_FOUND, "Query ignored"));
                }
                _ => {}
            }
        }

        // Default: no response sent
        Ok(error_response(StatusCode::INTERNAL_SERVER_ERROR, "No response generated"))
    }
}

/// Decode base64url (URL-safe base64 without padding)
fn base64_url_decode(encoded: &str) -> Result<Vec<u8>> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    URL_SAFE_NO_PAD
        .decode(encoded)
        .context("Failed to decode base64url")
}

/// Create an error response
fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain")
        .body(Full::new(Bytes::from(message.to_string())))
        .unwrap()
}
