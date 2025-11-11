//! Redis server implementation with RESP protocol
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error};
use actions::{RedisProtocol, REDIS_COMMAND_EVENT};
use anyhow::Result;
use redis_protocol::resp2::decode::decode;
use redis_protocol::resp2::types::OwnedFrame as Frame;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// Redis server implementation
pub struct RedisServer {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    #[allow(dead_code)]
    status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl RedisServer {
    /// Create a new Redis server
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

    /// Spawn Redis server with LLM integration
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

        info!("Redis server starting on {}", actual_addr);
        let _ = status_tx.send(format!("[INFO] Redis server listening on {}", actual_addr));

        let server = Arc::new(RedisServer::new(
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
                        console_debug!(status_tx, "Redis connection from {}", addr);

                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(actual_addr);

                        // Track the connection
                        if let Some(server_id) = server.server_id {
                            use crate::state::server::{
                                ConnectionState as ServerConnectionState, ConnectionStatus,
                                ProtocolConnectionInfo,
                            };
                            let now = std::time::Instant::now();
                            let conn_state = ServerConnectionState {
                                id: connection_id,
                                remote_addr: addr,
                                local_addr: local_addr_conn,
                                bytes_sent: 0,
                                bytes_received: 0,
                                packets_sent: 0,
                                packets_received: 0,
                                last_activity: now,
                                status: ConnectionStatus::Active,
                                status_changed_at: now,
                                protocol_info: ProtocolConnectionInfo::empty(),
                            };
                            server
                                .app_state
                                .add_connection_to_server(server_id, conn_state)
                                .await;
                        }

                        let handler = RedisHandler {
                            connection_id,
                            llm_client: server.llm_client.clone(),
                            app_state: server.app_state.clone(),
                            status_tx: status_tx.clone(),
                            server_id: server.server_id,
                        };

                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_connection(stream).await {
                                error!("Redis connection error: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Redis accept error: {}", e);
                    }
                }
            }
        });

        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        Ok(actual_addr)
    }
}

/// Redis connection handler
struct RedisHandler {
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl RedisHandler {
    async fn handle_connection(self, mut stream: TcpStream) -> Result<()> {
        let protocol = Arc::new(RedisProtocol::new(
            self.connection_id,
            self.app_state.clone(),
            self.status_tx.clone(),
        ));

        let mut buffer = Vec::new();

        loop {
            // Read data from the stream
            let mut chunk = vec![0u8; 4096];
            let n = match stream.read(&mut chunk).await {
                Ok(0) => {
                    debug!("Redis client disconnected");
                    return Ok(());
                }
                Ok(n) => n,
                Err(e) => {
                    error!("Redis read error: {}", e);
                    return Err(e.into());
                }
            };

            buffer.extend_from_slice(&chunk[..n]);

            // Try to decode RESP frames
            let mut offset = 0;
            while offset < buffer.len() {
                match decode(&buffer[offset..]) {
                    Ok(Some((frame, consumed))) => {
                        trace!("Redis frame: {:?}", frame);

                        // Extract command from frame
                        let command_str = frame_to_command_string(&frame);
                        debug!("Redis command: {}", command_str);
                        let _ = self
                            .status_tx
                            .send(format!("[DEBUG] Redis command: {}", command_str));

                        // Create command event
                        let event = Event::new(
                            &REDIS_COMMAND_EVENT,
                            serde_json::json!({
                                "command": command_str.clone(),
                            }),
                        );

                        let server_id = self
                            .server_id
                            .unwrap_or_else(|| crate::state::ServerId::new(0));

                        let llm_result = call_llm(
                            &self.llm_client,
                            &self.app_state,
                            server_id,
                            Some(self.connection_id),
                            &event,
                            protocol.as_ref(),
                        )
                        .await;

                        match llm_result {
                            Ok(execution_result) => {
                                // Process action results and encode RESP responses
                                for result in execution_result.protocol_results {
                                    match result {
                                        ActionResult::Custom { name, data } => {
                                            match name.as_str() {
                                                "redis_simple_string" => {
                                                    let value = data
                                                        .get("value")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("");
                                                    let resp = encode_simple_string(value);
                                                    stream.write_all(&resp).await?;
                                                }
                                                "redis_bulk_string" => {
                                                    let value = data.get("value");
                                                    let resp = if let Some(v) = value {
                                                        if v.is_null() {
                                                            encode_null()
                                                        } else if let Some(s) = v.as_str() {
                                                            encode_bulk_string(s.as_bytes())
                                                        } else {
                                                            encode_bulk_string(
                                                                v.to_string().as_bytes(),
                                                            )
                                                        }
                                                    } else {
                                                        encode_null()
                                                    };
                                                    stream.write_all(&resp).await?;
                                                }
                                                "redis_array" => {
                                                    let values = data
                                                        .get("values")
                                                        .and_then(|v| v.as_array())
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    let resp = encode_array(&values)?;
                                                    stream.write_all(&resp).await?;
                                                }
                                                "redis_integer" => {
                                                    let value = data
                                                        .get("value")
                                                        .and_then(|v| v.as_i64())
                                                        .unwrap_or(0);
                                                    let resp = encode_integer(value);
                                                    stream.write_all(&resp).await?;
                                                }
                                                "redis_error" => {
                                                    let message = data
                                                        .get("message")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("Unknown error");
                                                    let resp = encode_error(message);
                                                    stream.write_all(&resp).await?;
                                                }
                                                "redis_null" => {
                                                    let resp = encode_null();
                                                    stream.write_all(&resp).await?;
                                                }
                                                _ => {
                                                    // Unknown custom response, ignore
                                                }
                                            }
                                        }
                                        ActionResult::CloseConnection => {
                                            debug!("Redis closing connection");
                                            return Ok(());
                                        }
                                        _ => {
                                            // Other action results are informational
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("LLM error for Redis command: {}", e);
                                let resp = encode_error(&format!("ERR LLM error: {}", e));
                                stream.write_all(&resp).await?;
                            }
                        }

                        offset += consumed;
                    }
                    Ok(None) => {
                        // Need more data
                        break;
                    }
                    Err(e) => {
                        // Invalid frame
                        error!("Redis decode error: {:?}", e);
                        return Err(e.into());
                    }
                }
            }

            // Remove processed bytes from buffer
            buffer.drain(..offset);
        }
    }
}

/// Convert RESP frame to command string for display
fn frame_to_command_string(frame: &Frame) -> String {
    match frame {
        Frame::Array(frames) => {
            let parts: Vec<String> = frames
                .iter()
                .map(|f| match f {
                    Frame::BulkString(bytes) => String::from_utf8_lossy(bytes).to_string(),
                    Frame::SimpleString(bytes) => String::from_utf8_lossy(bytes).to_string(),
                    Frame::Integer(i) => i.to_string(),
                    _ => format!("{:?}", f),
                })
                .collect();
            parts.join(" ")
        }
        _ => format!("{:?}", frame),
    }
}

/// Encode a simple string response ("+OK\r\n")
fn encode_simple_string(s: &str) -> Vec<u8> {
    format!("+{}\r\n", s).into_bytes()
}

/// Encode a bulk string response ("$5\r\nhello\r\n")
fn encode_bulk_string(bytes: &[u8]) -> Vec<u8> {
    let mut result = format!("${}\r\n", bytes.len()).into_bytes();
    result.extend_from_slice(bytes);
    result.extend_from_slice(b"\r\n");
    result
}

/// Encode a null bulk string ("$-1\r\n")
fn encode_null() -> Vec<u8> {
    b"$-1\r\n".to_vec()
}

/// Encode an integer response (":42\r\n")
fn encode_integer(i: i64) -> Vec<u8> {
    format!(":{}\r\n", i).into_bytes()
}

/// Encode an error response ("-ERR message\r\n")
fn encode_error(msg: &str) -> Vec<u8> {
    format!("-{}\r\n", msg).into_bytes()
}

/// Encode an array response
fn encode_array(values: &[serde_json::Value]) -> Result<Vec<u8>> {
    let mut result = format!("*{}\r\n", values.len()).into_bytes();

    for value in values {
        match value {
            serde_json::Value::String(s) => {
                result.extend_from_slice(&encode_bulk_string(s.as_bytes()));
            }
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    result.extend_from_slice(&encode_integer(i));
                } else {
                    // Encode as bulk string
                    let s = n.to_string();
                    result.extend_from_slice(&encode_bulk_string(s.as_bytes()));
                }
            }
            serde_json::Value::Bool(b) => {
                let s = if *b { "1" } else { "0" };
                result.extend_from_slice(&encode_bulk_string(s.as_bytes()));
            }
            serde_json::Value::Null => {
                result.extend_from_slice(&encode_null());
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                // Nested arrays/objects - encode as bulk string JSON
                let s = value.to_string();
                result.extend_from_slice(&encode_bulk_string(s.as_bytes()));
            }
        }
    }

    Ok(result)
}
