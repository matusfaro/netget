//! HTTP server implementation using hyper

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::events::types::NetworkEvent;
use crate::network::connection::ConnectionId;

/// HTTP server that delegates request handling to LLM via events
pub struct HttpServer {
    listener: TcpListener,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
}

impl HttpServer {
    /// Create a new HTTP server
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<NetworkEvent>,
    ) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        info!("HTTP server listening on {}", addr);

        Ok(Self { listener, event_tx })
    }

    /// Get the local address the server is listening on
    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Spawn the HTTP server for TUI mode
    pub async fn spawn_tui(
        listen_addr: SocketAddr,
        network_tx: mpsc::UnboundedSender<NetworkEvent>,
    ) -> anyhow::Result<()> {
        let http_server = HttpServer::new(listen_addr, network_tx.clone()).await?;

        // Send listening event
        let _ = network_tx.send(NetworkEvent::Listening { addr: listen_addr });

        // Spawn server loop
        tokio::spawn(async move {
            if let Err(e) = http_server.accept_loop().await {
                eprintln!("HTTP server error: {}", e);
            }
        });

        Ok(())
    }

    /// Accept and handle HTTP connections
    pub async fn accept_loop(self) -> anyhow::Result<()> {
        let event_tx = Arc::new(self.event_tx);

        loop {
            let (stream, remote_addr) = match self.listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };

            let event_tx = Arc::clone(&event_tx);

            // Spawn a task to handle this connection
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let connection_id = ConnectionId::new();

                debug!("New HTTP connection from {}", remote_addr);

                // Send Connected event
                let _ = event_tx.send(NetworkEvent::Connected {
                    connection_id,
                    remote_addr,
                });

                // Clone event_tx for use in the service and after
                let event_tx_for_service = Arc::clone(&event_tx);
                let event_tx_for_disconnect = Arc::clone(&event_tx);

                // Create a service that handles requests
                let service = service_fn(move |req: Request<Incoming>| {
                    let event_tx = Arc::clone(&event_tx_for_service);
                    handle_http_request(req, connection_id, event_tx)
                });

                // Serve HTTP/1 on this connection
                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    error!("Error serving HTTP connection: {:?}", err);
                }

                // Send Disconnected event
                let _ = event_tx_for_disconnect.send(NetworkEvent::Disconnected { connection_id });
            });
        }
    }
}

/// Handle a single HTTP request
async fn handle_http_request(
    req: Request<Incoming>,
    connection_id: ConnectionId,
    event_tx: Arc<mpsc::UnboundedSender<NetworkEvent>>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!(
        "HTTP request: {} {} from {:?}",
        req.method(),
        req.uri(),
        connection_id
    );

    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri().to_string();

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Read body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            Bytes::new()
        }
    };

    // Create a oneshot channel for the response
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    // Send HttpRequest event to LLM
    let _ = event_tx.send(NetworkEvent::HttpRequest {
        connection_id,
        method,
        uri,
        headers,
        body: body_bytes,
        response_tx,
    });

    // Wait for the LLM to generate a response
    let llm_response = match response_rx.await {
        Ok(resp) => resp,
        Err(_) => {
            // If the LLM fails to respond, return a 500 error
            error!("Failed to receive response from LLM");
            return Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap());
        }
    };

    // Build the HTTP response from LLM response
    let mut response = Response::builder().status(llm_response.status);

    // Add headers
    for (name, value) in llm_response.headers {
        response = response.header(name, value);
    }

    Ok(response.body(Full::new(llm_response.body)).unwrap())
}
