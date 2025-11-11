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
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

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

        console_info!(status_tx, "[INFO] Starting DoH server on {}", bind_addr);

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

        console_info!(status_tx, "[INFO] DoH server listening on {}", self.bind_addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    console_debug!(status_tx, "[DEBUG] DoH TCP connection from {}", peer_addr);

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
                    console_warn!(status_tx, "[WARN] Failed to accept DoH TCP connection: {}", e);
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

        console_debug!(status_tx, "[DEBUG] DoH TLS handshake complete with {}", peer_addr);

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

        console_debug!(status_tx, "[DEBUG] DoH request: {} {}", method, uri);

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
                                console_warn!(status_tx, "[WARN] Invalid base64url DNS query: {}", e);
                                return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid DNS query encoding"));
                            }
                        }
                    }
                    None => {
                        console_warn!(status_tx, "[WARN] Missing dns= parameter in DoH GET request");
                        return Ok(error_response(StatusCode::BAD_REQUEST, "Missing dns parameter"));
                    }
                }
            }
            Method::POST => {
                // Check Content-Type
                if let Some(content_type) = req.headers().get("content-type") {
                    if content_type != "application/dns-message" {
                        console_warn!(status_tx, "[WARN] Invalid DoH Content-Type: {:?}", content_type);
                        return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid Content-Type"));
                    }
                } else {
                    console_warn!(status_tx, "[WARN] Missing Content-Type in DoH POST");
                    return Ok(error_response(StatusCode::BAD_REQUEST, "Missing Content-Type"));
                }

                // Read request body
                let body = req.collect().await?.to_bytes();
                body.to_vec()
            }
            _ => {
                console_warn!(status_tx, "[WARN] Unsupported DoH method: {}", method);
                return Ok(error_response(StatusCode::METHOD_NOT_ALLOWED, "Only GET and POST are supported"));
            }
        };

        console_debug!(status_tx, "[DEBUG] DoH received {} bytes", dns_bytes.len());

        console_trace!(status_tx, "[TRACE] DoH DNS query hex: {}", hex::encode(&dns_bytes));

        // Parse DNS query
        let dns_message = match DnsMessage::from_vec(&dns_bytes) {
            Ok(msg) => msg,
            Err(e) => {
                console_error!(status_tx, "[ERROR] Failed to parse DoH DNS message: {}", e);
                return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid DNS message"));
            }
        };

        // Extract query information
        let queries = dns_message.queries();
        if queries.is_empty() {
            console_warn!(status_tx, "[WARN] DoH DNS message has no queries");
            return Ok(error_response(StatusCode::BAD_REQUEST, "No DNS queries"));
        }

        let query = &queries[0];
        let domain = query.name().to_utf8();
        let query_type = format!("{:?}", query.query_type());
        let query_id = dns_message.id();

        console_info!(status_tx, "[INFO] DoH query: {} {} (ID: {})", domain, query_type, query_id);

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

        console_debug!(status_tx, "[DEBUG] DoH calling LLM for query from {}", peer_addr);

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
                console_error!(status_tx, "[ERROR] DoH LLM call failed: {}", e);
                return Ok(error_response(StatusCode::INTERNAL_SERVER_ERROR, "LLM error"));
            }
        };

        // Display messages from LLM
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        console_debug!(status_tx, "[DEBUG] DoH got {} protocol results", execution_result.protocol_results.len());

        // Execute actions from LLM response
        for protocol_result in &execution_result.protocol_results {
            use crate::llm::actions::protocol_trait::ActionResult;
            match protocol_result {
                ActionResult::Output(bytes) => {
                    // DNS action returned binary response directly
                    console_debug!(status_tx, "[DEBUG] DoH sending {} bytes", bytes.len());

                    console_trace!(status_tx, "[TRACE] DoH response hex: {}", hex::encode(bytes));

                    // Return DNS response with correct Content-Type
                    return Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/dns-message")
                        .header("Content-Length", bytes.len())
                        .body(Full::new(Bytes::from(bytes.clone())))
                        .unwrap());
                }
                ActionResult::Custom { data, .. } => {
                    if let Some(output_data) = data.get("output_data").and_then(|v| v.as_str()) {
                        // Decode hex DNS response
                        if let Ok(response_bytes) = hex::decode(output_data) {
                            console_debug!(status_tx, "[DEBUG] DoH sending {} bytes", response_bytes.len());

                            console_trace!(status_tx, "[TRACE] DoH response hex: {}", output_data);

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
                    console_debug!(status_tx, "[DEBUG] DoH query ignored by LLM");
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
