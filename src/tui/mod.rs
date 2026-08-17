//! Full-screen ratatui dashboard — the default interactive UI.
//!
//! Chat sits on the left (history above, multi-line input below, same LLM
//! contract as the legacy TUI). The right-hand rail lists servers and clients
//! as horizontal bands, each split into vertical panes: identity, config,
//! routing (LLM / script / static), connected + recently-connected peers, and
//! a request log. Bands expand when few instances exist and shrink (staying
//! scrollable) as more arrive.
//!
//! The legacy rolling TUI remains available behind `--legacy-tui`; both share
//! the same startup construction, status channel, tick cadences and command
//! grammar.

pub mod app;
pub mod bands;
pub mod chat;
pub mod command_exec;
pub mod commands;
pub mod event_loop;
pub mod hit;
pub mod keymap;
pub mod modal;
pub mod projection;
pub mod render;
pub mod theme;

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{mpsc, Mutex};

use crate::cli::theme::ColorPalette;
use crate::events::EventHandler;
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::App;

use app::DashboardApp;
use event_loop::LoopContext;
use theme::Styles;

/// Entry point, mirroring `run_rolling_tui`'s signature so `cli::run` picks
/// one with a single branch.
pub async fn run_dashboard(
    state: AppState,
    mut core: App,
    event_handler: EventHandler,
    llm_client: OllamaClient,
    settings: Settings,
    args: &crate::cli::Args,
    palette: ColorPalette,
) -> Result<()> {
    let settings = Arc::new(Mutex::new(settings));

    // Resolve the model exactly as the legacy TUI does (shared module).
    let resolved = {
        let guard = settings.lock().await;
        crate::cli::model_select::resolve_startup_model(args, &guard).await?
    };
    state
        .set_ollama_model(if resolved.model.is_empty() {
            None
        } else {
            Some(resolved.model.clone())
        })
        .await;
    core.connection_info.model = resolved.model.clone();

    // Web-search mode from settings, and the CLI handler-mode override.
    let web_search_mode = settings.lock().await.get_web_search_mode();
    state.set_web_search_mode(web_search_mode).await;
    if let Ok(Some(handler_mode)) = args.parse_event_handler_mode() {
        state.set_event_handler_mode(handler_mode).await;
    }

    let (status_tx, status_rx) = mpsc::unbounded_channel::<String>();
    let (web_approval_tx, web_approval_rx) = mpsc::unbounded_channel();
    state.set_web_approval_channel(web_approval_tx).await;

    let styles = Styles::from_palette(&palette);
    let mut app = DashboardApp::new(core, styles);

    app.push_system(format!(
        "NetGet dashboard — Tab moves between chat and the instance rail, F1 for keys{}",
        if args.legacy_tui {
            ""
        } else {
            " (--legacy-tui for the old UI)"
        }
    ));
    for message in resolved.messages {
        app.push_system(message);
    }

    // ASCII banner streams into chat like any other status output.
    if !args.suppress_art {
        let base_url = resolved.base_url.clone();
        let model = resolved.model.clone();
        let tx = status_tx.clone();
        tokio::spawn(async move {
            let _ = crate::cli::banner::generate_and_stream_ascii_banner(&base_url, &model, tx)
                .await;
        });
    }

    let ctx = LoopContext {
        state,
        event_handler,
        llm_client,
        settings,
        status_tx,
        status_rx,
        web_approval_rx,
    };

    event_loop::run(app, ctx).await
}
