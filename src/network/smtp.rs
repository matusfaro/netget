//! SMTP server implementation

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[cfg(feature = "smtp")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "smtp")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "smtp")]
use crate::llm::ActionResult;
#[cfg(feature = "smtp")]
use crate::network::smtp_actions::SMTP_COMMAND_EVENT;
#[cfg(feature = "smtp")]
use crate::network::SmtpProtocol;
#[cfg(feature = "smtp")]
use crate::protocol::Event;
#[cfg(feature = "smtp")]
use crate::state::app_state::AppState;

/// SMTP server that forwards mail to LLM
pub struct SmtpServer;

#[cfg(feature = "smtp")]
impl SmtpServer {
    /// Spawn SMTP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("SMTP server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(SmtpProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::network::connection::ConnectionId::new();
                        debug!("SMTP connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[DEBUG] SMTP connection {} from {}", connection_id, remote_addr));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let mut session = SmtpSession {
                                stream,
                                connection_id,
                                server_id,
                                llm_client: llm_clone.clone(),
                                app_state: state_clone.clone(),
                                status_tx: status_clone.clone(),
                                protocol: protocol_clone.clone(),
                            };

                            // Send initial greeting using Event
                            let greeting_event = Event::new(&SMTP_COMMAND_EVENT, serde_json::json!({
                                "command": "CONNECTION_ESTABLISHED"
                            }));

                            if let Ok(execution_result) = call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                Some(connection_id),
                                &greeting_event,
                                protocol_clone.as_ref(),
                            ).await {
                                for protocol_result in execution_result.protocol_results {
                                    if let ActionResult::Output(data) = protocol_result {
                                        use tokio::io::AsyncWriteExt;
                                        let _ = session.stream.write_all(&data).await;
                                        let _ = session.stream.flush().await;
                                    }
                                }
                            }

                            // Handle SMTP session
                            if let Err(e) = session.handle().await {
                                error!("SMTP session error: {}", e);
                                let _ = status_clone.send(format!("[ERROR] SMTP session error: {}", e));
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept SMTP connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

#[cfg(feature = "smtp")]
struct SmtpSession {
    stream: tokio::net::TcpStream,
    connection_id: crate::network::connection::ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<SmtpProtocol>,
}

#[cfg(feature = "smtp")]
impl SmtpSession {
    async fn handle(&mut self) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (read_half, mut write_half) = tokio::io::split(&mut self.stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                break;
            }

            let command = line.trim();
            debug!("SMTP received: {}", command);
            let _ = self.status_tx.send(format!("[DEBUG] SMTP received: {}", command));

            // Create SMTP command event
            let event = Event::new(&SMTP_COMMAND_EVENT, serde_json::json!({
                "command": command
            }));

            // Get LLM response
            if let Ok(execution_result) = call_llm(
                &self.llm_client,
                &self.app_state,
                self.server_id,
                Some(self.connection_id),
                &event,
                self.protocol.as_ref(),
            ).await {
                for protocol_result in execution_result.protocol_results {
                    match protocol_result {
                        ActionResult::Output(data) => {
                            write_half.write_all(&data).await?;
                            write_half.flush().await?;

                            let response = String::from_utf8_lossy(&data);
                            debug!("SMTP sent: {}", response.trim());
                            let _ = self.status_tx.send(format!("[DEBUG] SMTP sent: {}", response.trim()));
                        }
                        ActionResult::CloseConnection => {
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(not(feature = "smtp"))]
impl SmtpServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        anyhow::bail!("SMTP feature not enabled")
    }
}
