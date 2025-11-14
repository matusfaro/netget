//! AMQP client implementation using lapin library
pub mod actions;

pub use actions::AmqpClientProtocol;

use crate::client::amqp::actions::AMQP_CLIENT_CONNECTED_EVENT;
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

/// AMQP client that connects to an AMQP broker (RabbitMQ, etc.)
pub struct AmqpClient;

impl AmqpClient {
    /// Connect to an AMQP broker with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
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

        // Call LLM with amqp_connected event
        if let Some(instruction) = state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &AMQP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "broker_addr": remote_addr.clone(),
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &state,
                client_id.to_string(),
                &instruction,
                &String::new(),
                Some(&event),
                &crate::client::amqp::actions::AmqpClientProtocol,
                &status_tx,
            )
            .await
            {
                Ok(_result) => {
                    info!("AMQP client ready after connect event");
                }
                Err(e) => {
                    error!("LLM error on amqp_connected event: {}", e);
                }
            }
        }

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
