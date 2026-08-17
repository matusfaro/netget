//! Executing a confirmed [`PendingAction`].
//!
//! Stops go through `AppState::remove_server` / `remove_client` — the real
//! teardowns that abort tasks and free sockets — not the LLM's `close_*`
//! actions, which only mark status.

use crate::state::app_state::AppState;

use super::PendingAction;

/// Run the action; returns a line for the chat log.
pub async fn execute(action: &PendingAction, state: &AppState) -> String {
    match action {
        PendingAction::StopServer(id) => match state.remove_server(*id).await {
            Some(server) => format!(
                "Stopped server #{} ({} on port {})",
                id.as_u32(),
                server.protocol_name,
                server.port
            ),
            None => format!("Server #{} was already gone", id.as_u32()),
        },
        PendingAction::StopClient(id) => match state.remove_client(*id).await {
            Some(client) => format!(
                "Stopped client #{} ({} → {})",
                id.as_u32(),
                client.protocol_name,
                client.remote_addr
            ),
            None => format!("Client #{} was already gone", id.as_u32()),
        },
        PendingAction::StopAll => {
            let servers = state.get_all_server_ids().await;
            let clients = state.get_all_client_ids().await;
            for id in &servers {
                state.remove_server(*id).await;
            }
            for id in &clients {
                state.remove_client(*id).await;
            }
            format!(
                "Stopped {} server(s) and {} client(s)",
                servers.len(),
                clients.len()
            )
        }
        PendingAction::Quit => "Quitting".to_string(),
    }
}
