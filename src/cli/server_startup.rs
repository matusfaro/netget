//! Server startup logic for TUI mode
//!
//! Handles spawning TCP and HTTP servers based on application state

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::ServerId;

/// Start a specific server by ID
pub async fn start_server_by_id(
    state: &AppState,
    server_id: ServerId,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<()> {
    // Get server info
    let server = match state.get_server(server_id).await {
        Some(s) => s,
        None => {
            let _ = status_tx.send(format!("[ERROR] Server #{} not found", server_id.as_u32()));
            return Ok(());
        }
    };

    // Build listen address
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", server.port).parse()?;

    let protocol_name = server.protocol_name.clone();
    let msg = format!(
        "[SERVER] Starting server #{} ({}) on {}",
        server_id.as_u32(),
        protocol_name,
        listen_addr
    );
    let _ = status_tx.send(msg.clone());

    // Actually spawn the server using the registry
    use crate::state::server::ServerStatus;

    // Get protocol implementation from registry
    let protocol = crate::protocol::registry::registry()
        .get(&protocol_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown protocol: {}", protocol_name))?;

    // Build type-safe startup params if provided
    let startup_params = if let Some(params_json) = server.startup_params.clone() {
        // Get the parameter schema from the protocol
        let schema = protocol.get_startup_parameters();
        // Create validated StartupParams
        Some(crate::protocol::StartupParams::new(params_json, schema))
    } else {
        None
    };

    // Build spawn context
    let spawn_ctx = crate::protocol::SpawnContext {
        listen_addr,
        llm_client: llm_client.clone(),
        state: Arc::new(state.clone()),
        status_tx: status_tx.clone(),
        server_id,
        startup_params,
    };

    // Spawn the server using the protocol's spawn method
    match protocol.spawn(spawn_ctx).await {
        Ok(actual_addr) => {
            // Update server with actual listen address
            state.update_server_local_addr(server_id, actual_addr).await;
            state
                .update_server_status(server_id, ServerStatus::Running)
                .await;
            let _ = status_tx.send(format!(
                "[SERVER] {} server #{} listening on {}",
                protocol_name,
                server_id.as_u32(),
                actual_addr
            ));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
        }
        Err(e) => {
            state
                .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                .await;
            let _ = status_tx.send(format!(
                "[ERROR] Failed to start {} server #{}: {}",
                protocol_name,
                server_id.as_u32(),
                e
            ));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
            return Err(e);
        }
    }

    Ok(())
}
