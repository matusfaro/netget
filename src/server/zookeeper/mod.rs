//! ZooKeeper server implementation
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::console_debug;
use actions::{ZookeeperProtocol, ZOOKEEPER_REQUEST_EVENT};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// ZooKeeper server implementation
pub struct ZookeeperServer {
    llm_client: OllamaClient,
    #[allow(dead_code)]
    app_state: Arc<AppState>,
    #[allow(dead_code)]
    status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl ZookeeperServer {
    /// Create a new ZooKeeper server
    pub fn new(
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
    ) -> Self {
        Self {
            llm_client,
            app_state,
            status_tx,
            server_id,
        }
    }

    /// Spawn ZooKeeper server with LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _send_first: bool,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let actual_addr = listener.local_addr()?;

        info!("ZooKeeper server starting on {}", actual_addr);
        let _ = status_tx.send(format!("[INFO] ZooKeeper server listening on {}", actual_addr));

        let server = Arc::new(ZookeeperServer::new(
            llm_client,
            app_state.clone(),
            status_tx.clone(),
            Some(server_id),
        ));

        let status_tx_clone = status_tx.clone();

        // Spawn the accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        console_debug!(status_tx, "ZooKeeper connection from {}", addr);

                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

                        // Clone server components for the connection handler
                        let server_clone = Arc::clone(&server);
                        let status_tx_conn = status_tx_clone.clone();
                        let app_state_conn = app_state.clone();
                        let server_id_opt = server.server_id;

                        // Spawn connection handler
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                server_clone,
                                status_tx_conn,
                                connection_id,
                                app_state_conn,
                                server_id_opt,
                            )
                            .await
                            {
                                error!("ZooKeeper connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("ZooKeeper accept error: {}", e);
                    }
                }
            }
        });

        Ok(actual_addr)
    }

    /// Handle a single ZooKeeper connection
    async fn handle_connection(
        stream: TcpStream,
        server: Arc<ZookeeperServer>,
        _status_tx: mpsc::UnboundedSender<String>,
        connection_id: ConnectionId,
        app_state: Arc<AppState>,
        server_id: Option<crate::state::ServerId>,
    ) -> Result<()> {
        let (mut read_half, mut write_half) = tokio::io::split(stream);

        debug!("ZooKeeper connection {} established", connection_id);

        loop {
            // Read ZooKeeper request header (4 bytes length + payload)
            let mut len_buf = [0u8; 4];
            match read_half.read_exact(&mut len_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("ZooKeeper client disconnected");
                    break;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }

            let len = i32::from_be_bytes(len_buf) as usize;
            if len > 1024 * 1024 {
                // Max 1MB
                return Err(anyhow::anyhow!("Request too large: {} bytes", len));
            }

            // Read payload
            let mut payload = vec![0u8; len];
            read_half.read_exact(&mut payload).await?;

            trace!(
                "ZooKeeper connection {} received {} bytes",
                connection_id,
                len
            );

            // Parse request (simplified - just extract op type)
            let request_info = Self::parse_request(&payload)?;

            // Call LLM with request event
            let event = Event::new(
                &ZOOKEEPER_REQUEST_EVENT,
                serde_json::json!({
                    "operation": request_info.operation,
                    "path": request_info.path,
                    "data_hex": hex::encode(&request_info.data),
                }),
            );

            let server_id = server_id.unwrap_or_else(|| crate::state::ServerId::new(0));

            let protocol = Arc::new(ZookeeperProtocol::new());

            match call_llm(
                &server.llm_client,
                &app_state,
                server_id,
                Some(connection_id),
                &event,
                protocol.as_ref(),
            )
            .await
            {
                Ok(execution_result) => {
                    // Execute actions
                    for result in execution_result.protocol_results {
                        match result {
                            ActionResult::Custom { name, data }
                                if name == "zookeeper_response" =>
                            {
                                if let Some(response_hex) =
                                    data.get("response_hex").and_then(|v| v.as_str())
                                {
                                    if let Ok(response_bytes) = hex::decode(response_hex) {
                                        // Send response with length prefix
                                        let len_bytes = (response_bytes.len() as i32).to_be_bytes();
                                        write_half.write_all(&len_bytes).await?;
                                        write_half.write_all(&response_bytes).await?;

                                        trace!(
                                            "ZooKeeper sent {} bytes to connection {}",
                                            response_bytes.len() + 4,
                                            connection_id
                                        );
                                    }
                                }
                            }
                            ActionResult::CloseConnection => {
                                debug!("ZooKeeper closing connection {}", connection_id);
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Parse ZooKeeper request (simplified)
    fn parse_request(payload: &[u8]) -> Result<ZookeeperRequest> {
        if payload.len() < 8 {
            return Ok(ZookeeperRequest {
                operation: "Unknown".to_string(),
                path: "".to_string(),
                data: vec![],
            });
        }

        // Read xid (transaction id)
        let _xid = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

        // Read op type
        let op_type = i32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

        let operation = match op_type {
            1 => "create",
            2 => "delete",
            3 => "exists",
            4 => "getData",
            5 => "setData",
            6 => "getACL",
            7 => "setACL",
            8 => "getChildren",
            9 => "sync",
            11 => "ping",
            12 => "getChildren2",
            13 => "check",
            14 => "multi",
            _ => "unknown",
        };

        // Try to extract path (if present)
        let path = if payload.len() > 12 {
            let path_len = i32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]) as usize;
            if payload.len() >= 12 + path_len {
                String::from_utf8_lossy(&payload[12..12 + path_len]).to_string()
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };

        Ok(ZookeeperRequest {
            operation: operation.to_string(),
            path,
            data: payload.to_vec(),
        })
    }
}

struct ZookeeperRequest {
    operation: String,
    path: String,
    data: Vec<u8>,
}
