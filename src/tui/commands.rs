//! Chat submit path: history, `UserCommand` parsing, LLM dispatch.
//!
//! Slash commands keep working because `UserCommand::parse` is reused
//! verbatim; the dashboard surfaces the same capabilities through the rail and
//! status bar as well (see `modal::help`).

use tokio::sync::mpsc;

use crate::events::{EventHandler, UserCommand};
use crate::state::app_state::AppState;
use crate::tui::app::DashboardApp;
use crate::tui::chat::EntryKind;
use crate::tui::modal::{Modal, PendingAction};

/// Handle one submitted line of chat input.
pub async fn submit(
    app: &mut DashboardApp,
    text: String,
    state: &AppState,
    event_handler: &EventHandler,
    status_tx: &mpsc::UnboundedSender<String>,
) {
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return;
    }
    app.core.add_to_history(trimmed.clone());
    app.chat.push(EntryKind::User, trimmed.clone());
    app.chat.scroll_to_follow();
    app.dirty = true;

    match UserCommand::parse(&trimmed) {
        UserCommand::Interpret { input } => {
            // Fire-and-forget, exactly as the rolling TUI does: output streams
            // back through the status channel.
            let mut handler = event_handler.clone();
            let tx = status_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = handler.handle_interpret_with_actions(input, tx.clone(), None).await
                {
                    let _ = tx.send(format!("[ERROR] LLM request failed: {e}"));
                }
            });
        }
        UserCommand::Quit => {
            app.modals.push(Modal::Confirm {
                message: "Quit NetGet? Running servers and clients will stop.".to_string(),
                action: PendingAction::Quit,
            });
        }
        UserCommand::StopAll => {
            app.modals.push(Modal::Confirm {
                message: "Stop every running server and client?".to_string(),
                action: PendingAction::StopAll,
            });
        }
        UserCommand::StopById { id } => {
            stop_by_unified_id(app, state, id).await;
        }
        UserCommand::ShowLogLevel => {
            app.push_system(format!("Log level: {}", app.core.log_level.as_str()));
        }
        UserCommand::ChangeLogLevel { level } => match crate::ui::app::LogLevel::parse(&level) {
            Some(parsed) => {
                app.core.set_log_level(parsed);
                app.push_system(format!("Log level set to {}", parsed.as_str()));
            }
            None => app.push_system(format!(
                "Unknown log level '{level}' (error|warn|info|debug|trace)"
            )),
        },
        UserCommand::ShowModel => {
            let model = state.get_ollama_model().await.unwrap_or_default();
            app.push_system(if model.is_empty() {
                "No model selected".to_string()
            } else {
                format!("Model: {model}")
            });
        }
        UserCommand::ChangeModel { model } => {
            state.set_ollama_model(Some(model.clone())).await;
            app.status.model = model.clone();
            app.push_system(format!("Model set to {model}"));
        }
        UserCommand::UnknownSlashCommand { command } => {
            app.chat.push(
                EntryKind::Log(crate::ui::app::LogLevel::Error),
                format!("Unknown command: /{command} — press F1 for what the dashboard offers"),
            );
        }
        other => {
            // Everything else is handled by the shared command executor, whose
            // output lines arrive on the status channel.
            let rendered = crate::tui::command_exec::execute(other, state, status_tx).await;
            for line in rendered {
                app.push_system(line);
            }
        }
    }
    app.dirty = true;
}

/// `/stop <id>`: unified id may name a server or a client.
async fn stop_by_unified_id(app: &mut DashboardApp, state: &AppState, id: u32) {
    let server_id = crate::state::ServerId::new(id);
    if state.get_server(server_id).await.is_some() {
        app.modals.push(Modal::Confirm {
            message: format!("Stop server #{id}?"),
            action: PendingAction::StopServer(server_id),
        });
        return;
    }
    let client_id = crate::state::ClientId::new(id);
    if state.get_client(client_id).await.is_some() {
        app.modals.push(Modal::Confirm {
            message: format!("Stop client #{id}?"),
            action: PendingAction::StopClient(client_id),
        });
        return;
    }
    app.push_system(format!("No server or client with id {id}"));
}
