//! ZooKeeper server implementation.
//!
//! # INCOMPLETE — read before using
//!
//! This server is marked [`DevelopmentState::Incomplete`] and is hidden from the LLM. It
//! parses the ZooKeeper request header and hands the fields to a handler, but it does not
//! implement the session handshake: a client's opening `ConnectRequest` (protocol version,
//! last zxid seen, timeout, session id, password) has no xid and no opcode, so it is misread
//! here as an ordinary request, and the `ConnectResponse` a client waits for is never sent.
//! No real ZooKeeper client can get a session out of it.
//!
//! The reply body is also left to the model as hand-encoded Jute hex, which violates the
//! project's no-bytes rule and which no model does reliably.
//!
//! See `src/server/zookeeper/CLAUDE.md` for the route back to `Experimental`.
//!
//! [`DevelopmentState::Incomplete`]: crate::protocol::metadata::DevelopmentState::Incomplete

pub mod actions;

use crate::console_debug;
use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::{ZookeeperProtocol, ZOOKEEPER_REQUEST_EVENT};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

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
        let _ = status_tx.send(format!(
            "[INFO] ZooKeeper server listening on {}",
            actual_addr
        ));
        warn!(
            "ZooKeeper server is INCOMPLETE: no session handshake is implemented, so a real \
             ZooKeeper client cannot establish a session. See src/server/zookeeper/CLAUDE.md."
        );
        let _ = status_tx.send(
            "[WARN] ZooKeeper is INCOMPLETE: no session handshake, no real client can connect"
                .to_string(),
        );

        let server = Arc::new(ZookeeperServer::new(
            llm_client,
            app_state.clone(),
            status_tx.clone(),
            Some(server_id),
        ));

        let status_tx_clone = status_tx.clone();

        // Spawn the accept loop
        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
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
                        // A persistent accept error (EMFILE, ENFILE, socket torn down) recurs
                        // immediately, so continuing here spins a hot loop that floods the
                        // unbounded status channel. Give up the listener instead.
                        error!("ZooKeeper accept error, stopping accept loop: {}", e);
                        let _ = status_tx_clone.send(format!(
                            "[ERROR] ZooKeeper accept failed, listener stopped: {}",
                            e
                        ));
                        break;
                    }
                }
            }
        });

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(actual_addr)
    }

    /// Handle a single ZooKeeper connection
    async fn handle_connection(
        stream: TcpStream,
        server: Arc<ZookeeperServer>,
        status_tx: mpsc::UnboundedSender<String>,
        connection_id: ConnectionId,
        app_state: Arc<AppState>,
        server_id: Option<crate::state::ServerId>,
    ) -> Result<()> {
        let peer_addr = stream.peer_addr().ok();
        let local_addr = stream.local_addr().ok();

        // Track the connection so the TUI/MCP connection list and stop_server see it.
        if let (Some(server_id), Some(remote_addr)) = (server_id, peer_addr) {
            let now = std::time::Instant::now();
            app_state
                .add_connection_to_server(
                    server_id,
                    crate::state::server::ConnectionState {
                        id: connection_id,
                        remote_addr,
                        local_addr: local_addr.unwrap_or(remote_addr),
                        bytes_sent: 0,
                        bytes_received: 0,
                        packets_sent: 0,
                        packets_received: 0,
                        last_activity: now,
                        status: crate::state::server::ConnectionStatus::Active,
                        status_changed_at: now,
                        protocol_info: crate::state::server::ProtocolConnectionInfo::empty(),
                    },
                )
                .await;
        }

        let result = Self::run_connection(
            stream,
            server,
            status_tx,
            connection_id,
            app_state.clone(),
            server_id,
        )
        .await;

        if let Some(server_id) = server_id {
            app_state
                .close_connection_on_server(server_id, connection_id)
                .await;
        }

        result
    }

    async fn run_connection(
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

            // The length prefix is a signed jute int and arrives from the network. Validate it
            // as i32 *before* widening: `negative as usize` sign-extends to ~1.8e19, and any
            // check performed after the cast is checking a number the sender never sent.
            let declared_len = i32::from_be_bytes(len_buf);
            if declared_len < MIN_REQUEST_BYTES || declared_len > MAX_REQUEST_BYTES {
                return Err(anyhow::anyhow!(
                    "ZooKeeper request length out of range: {} (expected {}..={})",
                    declared_len,
                    MIN_REQUEST_BYTES,
                    MAX_REQUEST_BYTES
                ));
            }
            let len = declared_len as usize;

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

            // Call LLM with request event. The xid must be in the event data: it is the only
            // thing a client uses to match a reply to its request, and without it neither the
            // model nor a script/static handler can echo it back.
            let event = Event::new(
                &ZOOKEEPER_REQUEST_EVENT,
                serde_json::json!({
                    "xid": request_info.xid,
                    "operation": request_info.operation,
                    "op_code": request_info.op_code,
                    "path": request_info.path,
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
                            ActionResult::Custom { name, data } if name == "zookeeper_response" => {
                                // A handler that omitted the xid gets the request's own xid
                                // rather than 0: xid 0 is never a valid reply to a real
                                // request (negative xids are reserved for pings and watch
                                // notifications) and leaves the client waiting forever.
                                let xid = data
                                    .get("xid")
                                    .and_then(|v| v.as_i64())
                                    .map(|v| v as i32)
                                    .unwrap_or(request_info.xid);
                                let zxid = data.get("zxid").and_then(|v| v.as_i64()).unwrap_or(0);
                                let error_code =
                                    data.get("error_code").and_then(|v| v.as_i64()).unwrap_or(0)
                                        as i32;
                                let body = data
                                    .get("body_hex")
                                    .and_then(|v| v.as_str())
                                    .map(hex::decode)
                                    .transpose()
                                    .unwrap_or_default()
                                    .unwrap_or_default();

                                // Reply header: xid (4) + zxid (8) + error_code (4) + body
                                let mut response = Vec::with_capacity(16 + body.len());
                                response.extend_from_slice(&xid.to_be_bytes());
                                response.extend_from_slice(&zxid.to_be_bytes());
                                response.extend_from_slice(&error_code.to_be_bytes());
                                response.extend_from_slice(&body);

                                let len_bytes = (response.len() as i32).to_be_bytes();
                                write_half.write_all(&len_bytes).await?;
                                write_half.write_all(&response).await?;

                                trace!(
                                    "ZooKeeper sent {} bytes to connection {} (xid={})",
                                    response.len() + 4,
                                    connection_id,
                                    xid
                                );
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

    /// Parse the ZooKeeper request header.
    ///
    /// Only the header (xid, opcode) and the leading path string are read; the rest of the
    /// Jute-encoded body is not decoded. A `ConnectRequest` has neither an xid nor an opcode,
    /// so it lands here as `operation: "unknown"` — see the module docs.
    fn parse_request(payload: &[u8]) -> Result<ZookeeperRequest> {
        if payload.len() < 8 {
            return Ok(ZookeeperRequest {
                xid: 0,
                op_code: 0,
                operation: "unknown".to_string(),
                path: String::new(),
            });
        }

        // Read xid (transaction id)
        let xid = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

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

        // Try to extract the path (a jute ustring: signed length + bytes).
        //
        // The length is validated as i32 against the bytes actually remaining. Casting first
        // was a remote panic: a length of -1 became usize::MAX, `12 + usize::MAX` wrapped to
        // 11, `payload.len() >= 11` passed, and `&payload[12..11]` panicked with start > end.
        let path = if payload.len() >= 12 {
            let declared = i32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
            let available = payload.len() - 12;
            if declared > 0 && (declared as usize) <= available {
                String::from_utf8_lossy(&payload[12..12 + declared as usize]).to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        Ok(ZookeeperRequest {
            xid,
            op_code: op_type,
            operation: operation.to_string(),
            path,
        })
    }
}

/// Smallest payload that can carry a request header (xid + opcode).
const MIN_REQUEST_BYTES: i32 = 8;
/// Matches ZooKeeper's own `jute.maxbuffer` default of 1 MiB.
const MAX_REQUEST_BYTES: i32 = 1024 * 1024;

struct ZookeeperRequest {
    xid: i32,
    op_code: i32,
    operation: String,
    path: String,
}
