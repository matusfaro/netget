//! AMQP client implementation using lapin library
pub mod actions;

pub use actions::AmqpClientProtocol;

use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

/// AMQP client that connects to an AMQP broker (RabbitMQ, etc.)
pub struct AmqpClient;

impl AmqpClient {
    /// Connect to an AMQP broker with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("AMQP client {} connecting to {}", client_id, remote_addr);

        // Connect to AMQP broker using lapin
        let conn = lapin::Connection::connect(
            &format!("amqp://{}", remote_addr),
            lapin::ConnectionProperties::default(),
        )
        .await
        .context(format!("Failed to connect to AMQP broker at {}", remote_addr))?;

        // Get local address (placeholder since lapin doesn't expose it)
        let local_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();

        info!(
            "AMQP client {} connected to {} (local: {})",
            client_id, remote_addr, local_addr
        );

        // Update client state
        state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] AMQP client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn a task to keep the connection alive
        let client_id_clone = client_id;
        let state_clone = state.clone();
        let status_tx_clone = status_tx.clone();
        tokio::spawn(async move {
            // Just keep connection alive - LLM integration simplified
            let _ = conn.run();
            info!("AMQP client {} connection closed", client_id_clone);
            state_clone
                .update_client_status(client_id_clone, ClientStatus::Disconnected)
                .await;
            let _ = status_tx_clone.send(format!("[CLIENT] AMQP client {} disconnected", client_id_clone));
            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        });

        Ok(local_addr)
    }
}
