//! Client startup logic for TUI mode
//!
//! Handles connecting clients based on application state

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::events::ActionExecutionError;
use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::ClientId;

/// Start a specific client by ID
pub async fn start_client_by_id(
    state: &AppState,
    client_id: ClientId,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<(), ActionExecutionError> {
    // Get client info
    let client = match state.get_client(client_id).await {
        Some(c) => c,
        None => {
            let _ = status_tx.send(format!("[ERROR] Client #{} not found", client_id.as_u32()));
            return Ok(());
        }
    };

    let protocol_name = client.protocol_name.clone();
    let remote_addr = client.remote_addr.clone();

    let msg = format!(
        "[CLIENT] Starting client #{} ({}) connecting to {}",
        client_id.as_u32(),
        protocol_name,
        remote_addr
    );
    let _ = status_tx.send(msg.clone());

    // Actually connect the client using the registry
    use crate::state::client::ClientStatus;

    // Get protocol implementation from registry
    let protocol = crate::protocol::CLIENT_REGISTRY
        .get(&protocol_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown client protocol: {}", protocol_name))?;

    // Build type-safe startup params if provided
    let startup_params = if let Some(params_json) = client.startup_params.clone() {
        // Get the parameter schema from the protocol
        let schema = protocol.get_startup_parameters();
        // Create validated StartupParams
        Some(crate::protocol::StartupParams::new(params_json, schema))
    } else {
        None
    };

    // Build connect context
    let connect_ctx = crate::protocol::ConnectContext {
        remote_addr: remote_addr.clone(),
        llm_client: llm_client.clone(),
        state: Arc::new(state.clone()),
        status_tx: status_tx.clone(),
        client_id,
        startup_params,
    };

    // Connect the client using the protocol's connect method
    match protocol.connect(connect_ctx).await {
        Ok(local_addr) => {
            // Update client status to connected
            state
                .update_client_status(client_id, ClientStatus::Connected)
                .await;
            let _ = status_tx.send(format!(
                "[CLIENT] {} client #{} connected to {} (local: {})",
                protocol_name,
                client_id.as_u32(),
                remote_addr,
                local_addr
            ));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
        }
        Err(e) => {
            // For connection errors, set client status to error
            state
                .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                .await;
            let _ = status_tx.send(format!(
                "[ERROR] Failed to connect {} client #{} to {}: {}",
                protocol_name,
                client_id.as_u32(),
                remote_addr,
                e
            ));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
            return Err(ActionExecutionError::Fatal(e));
        }
    }

    Ok(())
}
