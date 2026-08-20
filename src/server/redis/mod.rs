//! Redis server implementation with RESP protocol
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::{RedisProtocol, REDIS_COMMAND_EVENT};
use anyhow::Result;
use redis_protocol::resp2::decode::decode;
use redis_protocol::resp2::types::OwnedFrame as Frame;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};

/// Maximum bytes we will buffer for a single, still-incomplete RESP frame.
///
/// `decode()` returns `Ok(None)` while a frame is incomplete, so without this cap a client
/// that announces a huge bulk string (`$2000000000\r\n`) and then stalls would make the
/// connection buffer grow without bound. Real Redis caps a bulk string at 512 MB; 64 MB is
/// far more than any LLM-authored command needs.
const MAX_PENDING_FRAME_BYTES: usize = 64 * 1024 * 1024;

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

        Log::new(Some(&status_tx)).info(format!("Redis server listening on {}", actual_addr));

        let server = Arc::new(RedisServer::new(
            llm_client,
            app_state.clone(),
            status_tx.clone(),
            Some(server_id),
        ));

        let status_tx_clone = status_tx.clone();
        let task_registrar = app_state.clone();

        // Spawn the accept loop
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        Log::new(Some(&status_tx)).info(format!("Redis connection from {}", addr));

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
                        Log::new(Some(&status_tx)).error(format!("Redis accept error: {}", e));
                    }
                }
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

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
    /// Run the connection, then always mark it closed in `AppState`.
    ///
    /// Without this the connection stays `Active` forever in the server's connection map,
    /// which is what `src/server/redis/CLAUDE.md` already claimed happened but did not.
    async fn handle_connection(self, stream: TcpStream) -> Result<()> {
        let server_id = self.server_id;
        let connection_id = self.connection_id;
        let app_state = self.app_state.clone();

        let result = self.run(stream).await;

        if let Some(server_id) = server_id {
            app_state
                .close_connection_on_server(server_id, connection_id)
                .await;
        }

        result
    }

    async fn run(self, mut stream: TcpStream) -> Result<()> {
        let protocol = Arc::new(RedisProtocol::new(
            self.connection_id,
            self.app_state.clone(),
            self.status_tx.clone(),
        ));

        let mut buffer = Vec::new();
        let log = Log::new(Some(&self.status_tx));

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

            if let Some(server_id) = self.server_id {
                self.app_state
                    .update_connection_stats(
                        server_id,
                        self.connection_id,
                        Some(n as u64),
                        None,
                        Some(1),
                        None,
                    )
                    .await;
            }

            if buffer.len() > MAX_PENDING_FRAME_BYTES {
                log.error(format!(
                    "Redis client {} exceeded the {} byte frame buffer limit without completing a frame; closing",
                    self.connection_id, MAX_PENDING_FRAME_BYTES
                ));
                let _ = stream
                    .write_all(&encode_error(
                        "ERR Protocol error: invalid multibulk length",
                    ))
                    .await;
                return Ok(());
            }

            // Try to decode RESP frames
            let mut offset = 0;
            while offset < buffer.len() {
                match decode(&buffer[offset..]) {
                    Ok(Some((frame, consumed))) => {
                        trace!("Redis frame: {:?}", frame);

                        // Extract command from frame. FileOnly: the redis_command
                        // event template surfaces the command to the TUI.
                        let command_str = frame_to_command_string(&frame);
                        log.debug(format!("Redis command: {}", command_str));

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

                        // Collect the RESP bytes the actions produced, then write once so
                        // connection stats stay accurate and a close request still flushes
                        // whatever the LLM asked us to say first.
                        let mut response = Vec::new();
                        let mut close_after_write = false;

                        match llm_result {
                            Ok(execution_result) => {
                                for result in execution_result.protocol_results {
                                    match result {
                                        ActionResult::Custom { name, data } => {
                                            match name.as_str() {
                                                "redis_simple_string" => {
                                                    let value = data
                                                        .get("value")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("");
                                                    response.extend_from_slice(
                                                        &encode_simple_string(value),
                                                    );
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
                                                    response.extend_from_slice(&resp);
                                                }
                                                "redis_array" => {
                                                    let values = data
                                                        .get("values")
                                                        .and_then(|v| v.as_array())
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    response
                                                        .extend_from_slice(&encode_array(&values));
                                                }
                                                "redis_integer" => {
                                                    let value = data
                                                        .get("value")
                                                        .and_then(|v| v.as_i64())
                                                        .unwrap_or(0);
                                                    response
                                                        .extend_from_slice(&encode_integer(value));
                                                }
                                                "redis_error" => {
                                                    let message = data
                                                        .get("message")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("Unknown error");
                                                    response
                                                        .extend_from_slice(&encode_error(message));
                                                }
                                                "redis_null" => {
                                                    response.extend_from_slice(&encode_null());
                                                }
                                                other => {
                                                    // A Redis action that produced a result name
                                                    // this loop does not encode would leave the
                                                    // client hanging - make that loud.
                                                    warn!(
                                                        "Redis: no RESP encoding for action result '{}', client will receive nothing",
                                                        other
                                                    );
                                                }
                                            }
                                        }
                                        ActionResult::CloseConnection => {
                                            debug!("Redis closing connection");
                                            close_after_write = true;
                                        }
                                        _ => {
                                            // Other action results are informational
                                        }
                                    }
                                }

                                if response.is_empty() && !close_after_write {
                                    // Redis is strictly request/response: a command with no
                                    // reply hangs the client until its own timeout.
                                    warn!(
                                        "Redis: no response action for command '{}', replying with an error",
                                        command_str
                                    );
                                    response.extend_from_slice(&encode_error(
                                        "ERR no response produced for this command",
                                    ));
                                }
                            }
                            Err(e) => {
                                // A RESP error is the only thing a client can do anything
                                // with here, and `redis-rs`, `jedis` and `redis-py` all
                                // surface it as an exception rather than a value - so it
                                // cannot be mistaken for a reply to the command.
                                //
                                // `-LOADING` when the backend is merely saturated: clients
                                // already know that prefix means "not ready yet, retry", and
                                // several retry it automatically. Anything else is `-ERR`.
                                let overloaded = crate::llm::is_overload_error(&e);
                                let message = redis_error_message(&e, overloaded);
                                log.warn(format!(
                                    "LLM error for Redis command '{}' on connection {} (overload={}); replying -{}: {}",
                                    command_str, self.connection_id, overloaded, message, e
                                ));
                                response.extend_from_slice(&encode_error(&message));
                            }
                        }

                        if !response.is_empty() {
                            stream.write_all(&response).await?;
                            if let Some(server_id) = self.server_id {
                                self.app_state
                                    .update_connection_stats(
                                        server_id,
                                        self.connection_id,
                                        None,
                                        Some(response.len() as u64),
                                        None,
                                        Some(1),
                                    )
                                    .await;
                            }
                        }

                        if close_after_write {
                            return Ok(());
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

/// The RESP simple-error payload to send when the LLM backend fails.
///
/// The text is a category, never the error itself (`crate::utils::wire_failure`). That also
/// settles a framing hazard: a RESP simple error is CRLF-terminated with no length prefix, so
/// a newline anywhere in the text ends the frame early and the remainder is parsed as the
/// *next* reply - and backend error strings are routinely multi-line (a timeout with a URL, a
/// serde error with a snippet), so it would have desynchronised the connection for good.
fn redis_error_message(err: &anyhow::Error, overloaded: bool) -> String {
    let text = crate::utils::WireFailure::classify(err).prefixed_text();
    if overloaded {
        format!("LOADING {text}")
    } else {
        format!("ERR {text}")
    }
}

/// Encode an array response.
///
/// The element mapping is part of the `redis_array` action contract documented to the LLM:
/// strings become bulk strings, integers become RESP integers, booleans become the bulk
/// strings `"1"`/`"0"`, null becomes a nil bulk string, and nested arrays/objects are
/// serialized to JSON and sent as a bulk string (RESP2 has no JSON type).
fn encode_array(values: &[serde_json::Value]) -> Vec<u8> {
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

    result
}
