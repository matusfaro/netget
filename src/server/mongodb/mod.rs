//! MongoDB server implementation with manual OP_MSG parsing
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error};
use actions::{MongodbProtocol, MONGODB_COMMAND_EVENT, MONGODB_DISCONNECTED_EVENT};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

#[cfg(feature = "mongodb-server")]
use bson::{doc, Bson, Document};

/// MongoDB server implementation
pub struct MongodbServer {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    _status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl MongodbServer {
    /// Create a new MongoDB server
    pub fn new(
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
    ) -> Self {
        Self {
            llm_client,
            app_state,
            _status_tx: status_tx,
            server_id,
        }
    }

    /// Spawn MongoDB server with LLM integration
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

        info!("MongoDB server starting on {}", actual_addr);
        let _ = status_tx.send(format!("[INFO] MongoDB server listening on {}", actual_addr));

        let server = Arc::new(MongodbServer::new(
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
                        console_debug!(status_tx, "MongoDB connection from {}", addr);

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

                        let handler = MongodbHandler::new(
                            connection_id,
                            server.llm_client.clone(),
                            server.app_state.clone(),
                            status_tx.clone(),
                            server.server_id,
                            addr,
                        );

                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_connection(stream).await {
                                error!("MongoDB connection error: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "MongoDB accept error: {}", e);
                    }
                }
            }
        });

        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        Ok(actual_addr)
    }
}

/// MongoDB connection handler
pub struct MongodbHandler {
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)]
    server_id: Option<crate::state::ServerId>,
    #[allow(dead_code)]
    remote_addr: SocketAddr,
    /// MongoDB protocol handler for action execution
    protocol: Arc<MongodbProtocol>,
}

impl MongodbHandler {
    pub fn new(
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
        remote_addr: SocketAddr,
    ) -> Self {
        let protocol = Arc::new(MongodbProtocol::new(
            connection_id,
            app_state.clone(),
            status_tx.clone(),
        ));

        Self {
            connection_id,
            llm_client,
            app_state,
            status_tx,
            server_id,
            remote_addr,
            protocol,
        }
    }

    /// Handle a MongoDB connection
    async fn handle_connection(self, mut stream: TcpStream) -> Result<()> {
        debug!(
            "MongoDB handler starting for connection {}",
            self.connection_id
        );

        // MongoDB doesn't require handshake - client sends first
        let (mut reader, mut writer) = stream.split();

        loop {
            // Read MongoDB wire protocol message header (16 bytes)
            // Format: messageLength (4) + requestID (4) + responseTo (4) + opCode (4)
            let mut header = [0u8; 16];
            match reader.read_exact(&mut header).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("MongoDB client disconnected");
                    break;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }

            let message_length = i32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let request_id = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            let _response_to = i32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            let op_code = i32::from_le_bytes([header[12], header[13], header[14], header[15]]);

            trace!(
                "MongoDB message: length={}, requestID={}, opCode={}",
                message_length,
                request_id,
                op_code
            );

            // Read the rest of the message body
            let body_length = (message_length - 16) as usize;
            let mut body = vec![0u8; body_length];
            reader.read_exact(&mut body).await?;

            // Parse command based on opCode
            // OP_MSG = 2013 (modern MongoDB uses this for all commands)
            let command_doc = if op_code == 2013 {
                self.parse_op_msg(&body)?
            } else {
                debug!("Unsupported MongoDB opCode: {}", op_code);
                continue;
            };

            trace!("MongoDB command document: {:?}", command_doc);

            // Extract command information
            let command_name = command_doc
                .keys()
                .next()
                .unwrap_or(&"unknown".to_string())
                .clone();
            let database = command_doc
                .get_str("$db")
                .unwrap_or("admin")
                .to_string();

            // Call LLM with command event
            let event_data = serde_json::json!({
                "command": command_name,
                "database": database,
                "collection": command_doc.get_str("collection").ok(),
                "filter": self.bson_to_json(command_doc.get("filter")),
                "document": self.bson_to_json(command_doc.get("documents").or_else(|| command_doc.get("document"))),
            });

            let event = Event::new(&MONGODB_COMMAND_EVENT, event_data);

            let llm_result = call_llm(
                &self.llm_client,
                &self.app_state,
                self.connection_id.to_string(),
                Some(&event),
                self.protocol.as_ref(),
                &self.status_tx,
            )
            .await?;

            // Execute actions from LLM
            for action in llm_result.actions {
                match self.protocol.execute_action(action.clone())? {
                    ActionResult::SendData { data } => {
                        let response_doc = self.json_to_bson_doc(&data)?;
                        let response_bytes = self.encode_op_msg_response(request_id, response_doc)?;
                        writer.write_all(&response_bytes).await?;
                    }
                    ActionResult::CloseConnection => {
                        debug!("Closing MongoDB connection");
                        return Ok(());
                    }
                    ActionResult::NoAction => {}
                    _ => {
                        debug!("Unhandled action result: {:?}", action);
                    }
                }
            }
        }

        // Send disconnected event
        let event = Event::new(
            &MONGODB_DISCONNECTED_EVENT,
            serde_json::json!({"reason": "client_disconnect"}),
        );
        let _ = call_llm(
            &self.llm_client,
            &self.app_state,
            self.connection_id.to_string(),
            Some(&event),
            self.protocol.as_ref(),
            &self.status_tx,
        )
        .await;

        Ok(())
    }

    /// Parse OP_MSG body (MongoDB 3.6+ wire protocol)
    #[cfg(feature = "mongodb-server")]
    fn parse_op_msg(&self, body: &[u8]) -> Result<Document> {
        // OP_MSG format: flagBits (4) + sections
        // We only handle section kind 0 (body document)
        if body.len() < 5 {
            return Err(anyhow::anyhow!("OP_MSG body too short"));
        }

        let _flag_bits = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        let section_kind = body[4];

        if section_kind != 0 {
            return Err(anyhow::anyhow!("Unsupported OP_MSG section kind: {}", section_kind));
        }

        // Parse BSON document starting at byte 5
        let doc = Document::from_reader(&body[5..])?;
        Ok(doc)
    }

    #[cfg(not(feature = "mongodb-server"))]
    fn parse_op_msg(&self, _body: &[u8]) -> Result<Document> {
        Err(anyhow::anyhow!("MongoDB server feature not enabled"))
    }

    /// Encode OP_MSG response
    #[cfg(feature = "mongodb-server")]
    fn encode_op_msg_response(&self, request_id: i32, doc: Document) -> Result<Vec<u8>> {
        let mut body = vec![0u8; 5]; // flagBits (4) + section kind (1)
        body[4] = 0; // Section kind 0 (body)

        // Serialize BSON document
        let doc_bytes = bson::to_vec(&doc)?;
        body.extend_from_slice(&doc_bytes);

        // Create header
        let message_length = (16 + body.len()) as i32;
        let response_to = request_id;
        let op_code = 2013i32; // OP_MSG

        let mut message = Vec::new();
        message.extend_from_slice(&message_length.to_le_bytes());
        message.extend_from_slice(&0i32.to_le_bytes()); // responseID (0 = server)
        message.extend_from_slice(&response_to.to_le_bytes());
        message.extend_from_slice(&op_code.to_le_bytes());
        message.extend_from_slice(&body);

        Ok(message)
    }

    #[cfg(not(feature = "mongodb-server"))]
    fn encode_op_msg_response(&self, _request_id: i32, _doc: Document) -> Result<Vec<u8>> {
        Err(anyhow::anyhow!("MongoDB server feature not enabled"))
    }

    /// Convert BSON to JSON
    #[cfg(feature = "mongodb-server")]
    fn bson_to_json(&self, bson_opt: Option<&Bson>) -> serde_json::Value {
        match bson_opt {
            Some(bson) => bson.clone().into_relaxed_extjson(),
            None => serde_json::Value::Null,
        }
    }

    #[cfg(not(feature = "mongodb-server"))]
    fn bson_to_json(&self, _bson_opt: Option<&Bson>) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Convert JSON action to BSON document for response
    #[cfg(feature = "mongodb-server")]
    fn json_to_bson_doc(&self, json: &serde_json::Value) -> Result<Document> {
        let action_type = json
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing action type")?;

        match action_type {
            "find_response" => {
                let documents = json
                    .get("documents")
                    .and_then(|v| v.as_array())
                    .context("Missing documents")?;

                let cursor_docs: Vec<Bson> = documents
                    .iter()
                    .filter_map(|d| Bson::try_from(d.clone()).ok())
                    .collect();

                Ok(doc! {
                    "ok": 1,
                    "cursor": {
                        "id": 0i64,
                        "ns": "test.collection",
                        "firstBatch": cursor_docs
                    }
                })
            }
            "insert_response" => {
                let n = json
                    .get("inserted_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as i32;
                Ok(doc! { "ok": 1, "n": n })
            }
            "update_response" => {
                let matched = json
                    .get("matched_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as i32;
                let modified = json
                    .get("modified_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as i32;
                Ok(doc! { "ok": 1, "n": matched, "nModified": modified })
            }
            "delete_response" => {
                let n = json
                    .get("deleted_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as i32;
                Ok(doc! { "ok": 1, "n": n })
            }
            "error_response" => {
                let code = json
                    .get("code")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let message = json
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                Ok(doc! { "ok": 0, "code": code, "errmsg": message })
            }
            _ => Ok(doc! { "ok": 1 }),
        }
    }

    #[cfg(not(feature = "mongodb-server"))]
    fn json_to_bson_doc(&self, _json: &serde_json::Value) -> Result<Document> {
        Err(anyhow::anyhow!("MongoDB server feature not enabled"))
    }
}
