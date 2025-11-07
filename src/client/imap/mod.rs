//! IMAP client implementation
pub mod actions;

pub use actions::ImapClientProtocol;

use anyhow::{Context, Result};
use async_imap::{Client as ImapAsyncClient};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{error, info};

use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// IMAP client that connects to an IMAP server
pub struct ImapClient;

impl ImapClient {
    /// Connect to an IMAP server with integrated LLM actions
    ///
    /// This is a minimal implementation that connects and authenticates.
    /// TODO: Add full LLM integration with mailbox operations
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: crate::llm::ollama_client::OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract authentication credentials from startup params
        let (username, password) = if let Some(params) = startup_params {
            let username = params.get_string("username");
            let password = params.get_string("password");
            (username, password)
        } else {
            return Err(anyhow::anyhow!("IMAP client requires startup parameters: username, password"));
        };

        info!(
            "IMAP client {} connecting to {} (user: {})",
            client_id, remote_addr, username
        );

        // Connect to IMAP server via TCP
        let tcp_stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to IMAP at {}", remote_addr))?;

        let local_addr = tcp_stream.local_addr()?;

        // Convert tokio stream to futures-compatible stream
        let compat_stream = tcp_stream.compat();

        // Create IMAP client
        let imap_client = ImapAsyncClient::new(compat_stream);

        // Authenticate (plaintext only for now)
        match imap_client.login(&username, &password).await {
            Ok(_session) => {
                info!("IMAP client {} authenticated successfully", client_id);
                // Session is dropped here - TODO: keep session for LLM actions
            }
            Err((e, _)) => {
                error!("IMAP client {} authentication failed: {}", client_id, e);
                return Err(anyhow::anyhow!("IMAP login failed: {}", e));
            }
        }

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] IMAP client {} connected and authenticated", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // TODO: Implement LLM integration loop for actions
        // For now, we just connect and authenticate to validate the protocol works

        Ok(local_addr)
    }
}
