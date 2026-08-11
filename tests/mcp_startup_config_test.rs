//! MCP mode must apply the same startup configuration as every other entry point.
//!
//! `--mcp` / `--mcp-http` return from `cli::run()` before the TUI's and the
//! non-interactive runner's configuration blocks, and `create_shared_state` used
//! to build a bare `AppState::new()`. Every flag below was therefore accepted on
//! the command line and silently ignored — including `--llm-max-concurrent`,
//! which is the one knob that could have worked around the limiter dropping
//! overlapping network requests. Since MCP is the primary headless surface,
//! these assertions are what keep the headless mode configurable at all.
//!
//! No Ollama and no real stdio: the service is constructed and its `AppState`
//! inspected directly.
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --features mcp-stdio,tcp \
//!       --test mcp_startup_config_test -- --test-threads=100

#![cfg(all(feature = "mcp-stdio", feature = "tcp"))]

use clap::Parser;
use netget::cli::Args;
use netget::llm::{DEFAULT_MAX_QUEUED, DEFAULT_QUEUE_TIMEOUT_SECS};
use netget::mcp_stdio::tools::NetGetMcpService;
use netget::settings::Settings;
use netget::state::app_state::{AppState, EventHandlerMode, ScriptingMode, WebSearchMode};

/// Build the MCP service the way `run_mcp_stdio` does and hand back the state
/// its tools mutate.
async fn mcp_state(argv: &[&str]) -> AppState {
    let args = Args::parse_from(argv);
    let service = NetGetMcpService::new(&args, Settings::default())
        .await
        .expect("service creation");
    service.app_state()
}

#[tokio::test]
async fn mcp_honours_llm_max_concurrent() {
    let state = mcp_state(&["netget", "--mcp", "--llm-max-concurrent", "7"]).await;
    let config = state.get_rate_limiter().await.get_config().await;
    assert_eq!(
        config.max_concurrent, 7,
        "--llm-max-concurrent must reach the rate limiter under --mcp"
    );
}

#[tokio::test]
async fn mcp_honours_llm_queue_bounds() {
    let state = mcp_state(&[
        "netget",
        "--mcp",
        "--llm-queue-timeout",
        "9",
        "--llm-max-queued",
        "3",
    ])
    .await;
    let config = state.get_rate_limiter().await.get_config().await;
    assert_eq!(config.queue_timeout_secs, 9);
    assert_eq!(config.max_queued, 3);
}

#[tokio::test]
async fn mcp_honours_token_limit_and_window() {
    let state = mcp_state(&[
        "netget",
        "--mcp",
        "--llm-token-limit",
        "5000",
        "--llm-token-window",
        "30",
    ])
    .await;
    let config = state.get_rate_limiter().await.get_config().await;
    assert_eq!(config.token_limit, Some(5000));
    assert_eq!(config.token_window_secs, 30);
}

/// With no flags MCP must land on the same shipped default as everything else —
/// sequential, but with a bounded queue rather than a drop.
#[tokio::test]
async fn mcp_defaults_match_the_shipped_rate_limiter_defaults() {
    let state = mcp_state(&["netget", "--mcp"]).await;
    let config = state.get_rate_limiter().await.get_config().await;
    assert_eq!(config.max_concurrent, 1);
    assert_eq!(config.queue_timeout_secs, DEFAULT_QUEUE_TIMEOUT_SECS);
    assert_eq!(config.max_queued, DEFAULT_MAX_QUEUED);
}

#[tokio::test]
async fn mcp_honours_no_scripts() {
    let state = mcp_state(&["netget", "--mcp", "--no-scripts"]).await;
    assert_eq!(
        state.get_selected_scripting_mode().await,
        ScriptingMode::Off,
        "--no-scripts must reach AppState under --mcp"
    );
}

#[tokio::test]
async fn mcp_honours_scripting_env() {
    let state = mcp_state(&["netget", "--mcp", "--env", "off"]).await;
    assert_eq!(
        state.get_selected_scripting_mode().await,
        ScriptingMode::Off
    );
}

#[tokio::test]
async fn mcp_honours_event_handler_mode() {
    let state = mcp_state(&["netget", "--mcp", "--handler", "static"]).await;
    assert_eq!(
        state.get_event_handler_mode().await,
        EventHandlerMode::Static,
        "--handler must reach AppState under --mcp"
    );
}

#[tokio::test]
async fn mcp_honours_include_disabled_protocols() {
    let state = mcp_state(&["netget", "--mcp", "--include-disabled-protocols"]).await;
    assert!(
        state.get_include_disabled_protocols().await,
        "--include-disabled-protocols must reach AppState under --mcp"
    );

    let default_state = mcp_state(&["netget", "--mcp"]).await;
    assert!(!default_state.get_include_disabled_protocols().await);
}

/// ASK needs a terminal to prompt on, which MCP does not have; the
/// non-interactive runner degrades it to OFF and MCP must do the same rather
/// than leaving a mode that can only hang.
#[tokio::test]
async fn mcp_never_selects_ask_web_search_mode() {
    let state = mcp_state(&["netget", "--mcp"]).await;
    assert_ne!(state.get_web_search_mode().await, WebSearchMode::Ask);
}
