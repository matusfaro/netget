//! SMTP server implementation

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[cfg(feature = "smtp")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "smtp")]
use crate::llm::prompt::PromptBuilder;
#[cfg(feature = "smtp")]
use crate::llm::{ActionResponse, execute_actions, ProtocolActions, ActionResult};
#[cfg(feature = "smtp")]
use crate::network::SmtpProtocol;
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
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("SMTP server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(SmtpProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        debug!("SMTP connection from {}", remote_addr);
                        let _ = status_tx.send(format!("[DEBUG] SMTP connection from {}", remote_addr));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;

                            // Send greeting
                            let greeting_prompt = PromptBuilder::build_network_event_action_prompt(
                                &state_clone,
                                "SMTP connection established. Send greeting banner.",
                                protocol_clone.get_sync_actions()
                            ).await;

                            let mut session = SmtpSession {
                                stream,
                                llm_client: llm_clone.clone(),
                                app_state: state_clone.clone(),
                                status_tx: status_clone.clone(),
                                protocol: protocol_clone.clone(),
                                model: model.clone(),
                            };

                            // Send initial greeting
                            if let Ok(llm_output) = llm_clone.generate(&model, &greeting_prompt).await {
                                if let Ok(action_response) = ActionResponse::from_str(&llm_output) {
                                    if let Ok(result) = execute_actions(
                                        action_response.actions,
                                        &state_clone,
                                        Some(protocol_clone.as_ref())
                                    ).await {
                                        for protocol_result in result.protocol_results {
                                            if let ActionResult::Output(data) = protocol_result {
                                                use tokio::io::AsyncWriteExt;
                                                let _ = session.stream.write_all(&data).await;
                                                let _ = session.stream.flush().await;
                                            }
                                        }
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
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<SmtpProtocol>,
    model: String,
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

            // Build prompt with command context
            let event_description = format!("SMTP command: {}", command);
            let prompt = PromptBuilder::build_network_event_action_prompt(
                &self.app_state,
                &event_description,
                self.protocol.get_sync_actions()
            ).await;

            // Get LLM response
            if let Ok(llm_output) = self.llm_client.generate(&self.model, &prompt).await {
                if let Ok(action_response) = ActionResponse::from_str(&llm_output) {
                    if let Ok(result) = execute_actions(
                        action_response.actions,
                        &self.app_state,
                        Some(self.protocol.as_ref())
                    ).await {
                        for protocol_result in result.protocol_results {
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
