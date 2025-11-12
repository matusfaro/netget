pub mod actions;

use crate::client::pop3::actions::{
    POP3_CLIENT_CONNECTED_EVENT, POP3_CLIENT_RESPONSE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::client::{ClientConnectionState, ClientId};
use anyhow::Result;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

pub use actions::Pop3ClientProtocol;

pub struct Pop3Client;

impl Pop3Client {
    /// Connect to POP3 server with LLM integration
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        let use_tls = app_state
            .get_client_startup_param(client_id, "use_tls")
            .await
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        info!(
            "POP3 client {} connecting to {} (TLS: {})",
            client_id, remote_addr, use_tls
        );

        if use_tls {
            Self::connect_tls(
                remote_addr,
                llm_client,
                app_state,
                status_tx,
                client_id,
            )
            .await
        } else {
            Self::connect_plain(
                remote_addr,
                llm_client,
                app_state,
                status_tx,
                client_id,
            )
            .await
        }
    }

    async fn connect_plain(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        let stream = TcpStream::connect(&remote_addr).await?;
        let local_addr = stream.local_addr()?;

        info!("POP3 client {} connected to {}", client_id, remote_addr);

        let (read_half, write_half) = tokio::io::split(stream);
        let reader = BufReader::new(read_half);
        let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

        let protocol = Arc::new(Pop3ClientProtocol);

        // Spawn read loop
        tokio::spawn(async move {
            if let Err(e) = Self::read_loop(
                reader,
                write_half,
                llm_client,
                app_state,
                status_tx,
                client_id,
                protocol,
                remote_addr,
            )
            .await
            {
                error!("POP3 client {} read loop error: {}", client_id, e);
            }
        });

        Ok(local_addr)
    }

    async fn connect_tls(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        use tokio_rustls::rustls::{ClientConfig, OwnedTrustAnchor, RootCertStore};
        use tokio_rustls::TlsConnector;

        // Parse hostname from address
        let hostname = remote_addr
            .split(':')
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid remote address"))?;

        // Create TLS config
        let mut root_store = RootCertStore::empty();
        root_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));

        let config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));

        // Connect to server
        let tcp_stream = TcpStream::connect(&remote_addr).await?;
        let local_addr = tcp_stream.local_addr()?;

        // Perform TLS handshake
        let server_name = tokio_rustls::rustls::ServerName::try_from(hostname)
            .map_err(|_| anyhow::anyhow!("Invalid DNS name"))?;

        let tls_stream = connector.connect(server_name, tcp_stream).await?;

        info!(
            "POP3S client {} connected to {} with TLS",
            client_id, remote_addr
        );

        let (read_half, write_half) = tokio::io::split(tls_stream);
        let reader = BufReader::new(read_half);
        let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

        let protocol = Arc::new(Pop3ClientProtocol);

        // Spawn read loop
        tokio::spawn(async move {
            if let Err(e) = Self::read_loop(
                reader,
                write_half,
                llm_client,
                app_state,
                status_tx,
                client_id,
                protocol,
                remote_addr,
            )
            .await
            {
                error!("POP3S client {} read loop error: {}", client_id, e);
            }
        });

        Ok(local_addr)
    }

    async fn read_loop<R, W>(
        mut reader: BufReader<R>,
        write_half: Arc<tokio::sync::Mutex<W>>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        protocol: Arc<Pop3ClientProtocol>,
        remote_addr: String,
    ) -> Result<()>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        // Read greeting from server
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let greeting = line.trim().to_string();

        debug!("POP3 client {} received greeting: {}", client_id, greeting);

        let is_ok = greeting.starts_with("+OK");

        // Send connected event to LLM
        let event = Event::new(
            &POP3_CLIENT_CONNECTED_EVENT,
            json!({
                "pop3_server": remote_addr,
                "greeting": greeting,
                "is_ok": is_ok,
            }),
        );

        // Initial LLM call with greeting
        if let Err(e) = Self::handle_llm_response(
            &event,
            &llm_client,
            &app_state,
            &status_tx,
            client_id,
            &protocol,
            &write_half,
        )
        .await
        {
            error!(
                "POP3 client {} failed to process greeting: {}",
                client_id, e
            );
            return Err(e);
        }

        // Main read loop
        loop {
            // Check connection state
            let state = app_state.get_client_connection_state(client_id).await;

            match state {
                ClientConnectionState::Idle => {
                    // Ready to read next response
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            debug!("POP3 client {} connection closed by server", client_id);
                            break;
                        }
                        Ok(_) => {
                            let response = line.trim().to_string();
                            if response.is_empty() {
                                continue;
                            }

                            debug!(
                                "POP3 client {} received response: {}",
                                client_id, response
                            );

                            // Check if this is a multiline response
                            let is_multiline = response.starts_with("+OK")
                                && (line.ends_with("\r\n") || line.ends_with("\n"));

                            let full_response = if is_multiline
                                && !response.contains("octets")
                                && !response.contains("messages")
                            {
                                // Read multiline response until "."
                                let mut multiline = response.clone();
                                loop {
                                    line.clear();
                                    reader.read_line(&mut line).await?;
                                    if line.trim() == "." {
                                        break;
                                    }
                                    multiline.push_str(&line);
                                }
                                multiline
                            } else {
                                response
                            };

                            let is_ok = full_response.starts_with("+OK");

                            let event = Event::new(
                                &POP3_CLIENT_RESPONSE_RECEIVED_EVENT,
                                json!({
                                    "response": full_response,
                                    "is_ok": is_ok,
                                }),
                            );

                            if let Err(e) = Self::handle_llm_response(
                                &event,
                                &llm_client,
                                &app_state,
                                &status_tx,
                                client_id,
                                &protocol,
                                &write_half,
                            )
                            .await
                            {
                                error!(
                                    "POP3 client {} failed to process response: {}",
                                    client_id, e
                                );
                                break;
                            }
                        }
                        Err(e) => {
                            error!("POP3 client {} read error: {}", client_id, e);
                            break;
                        }
                    }
                }
                ClientConnectionState::Processing => {
                    // Wait for LLM to finish processing
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                ClientConnectionState::Accumulating => {
                    // Accumulating more data (not typical for POP3)
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }

        info!("POP3 client {} disconnected", client_id);
        Ok(())
    }

    async fn handle_llm_response<W>(
        event: &Event,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
        protocol: &Arc<Pop3ClientProtocol>,
        write_half: &Arc<tokio::sync::Mutex<W>>,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        use crate::llm::actions::client_trait::ClientActionResult;

        // Set state to Processing
        app_state
            .set_client_connection_state(client_id, ClientConnectionState::Processing)
            .await;

        // Call LLM
        let llm_result =
            call_llm_for_client(llm_client, app_state, status_tx, protocol, client_id, Some(event))
                .await?;

        // Update memory if returned
        if let Some(memory) = llm_result.memory {
            app_state.update_client_memory(client_id, memory).await;
        }

        // Execute actions
        for action in llm_result.actions {
            let action_result = protocol.as_ref().execute_action(action)?;

            match action_result {
                ClientActionResult::Custom { name, data } => {
                    if name == "pop3_command" {
                        let command = data["command"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing command in action data"))?;

                        debug!("POP3 client {} sending command: {}", client_id, command);

                        let mut writer = write_half.lock().await;
                        writer.write_all(command.as_bytes()).await?;
                        writer.write_all(b"\r\n").await?;
                        writer.flush().await?;
                    }
                }
                ClientActionResult::Disconnect => {
                    debug!("POP3 client {} disconnecting", client_id);
                    // Send QUIT command before closing
                    let mut writer = write_half.lock().await;
                    writer.write_all(b"QUIT\r\n").await?;
                    writer.flush().await?;
                    return Ok(());
                }
                ClientActionResult::WaitForMore => {
                    // Do nothing, wait for next response
                }
                _ => {
                    // Unknown action
                }
            }
        }

        // Set state back to Idle
        app_state
            .set_client_connection_state(client_id, ClientConnectionState::Idle)
            .await;

        Ok(())
    }
}
