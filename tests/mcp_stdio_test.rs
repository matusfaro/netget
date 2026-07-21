//! MCP STDIO server smoke tests
//!
//! These tests exercise the MCP server end-to-end over an in-process duplex
//! transport (no real stdin/stdout, no Ollama). They verify that the JSON-RPC
//! handshake, tool discovery, and non-LLM tool calls work.
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --features mcp-stdio,tcp --test mcp_stdio_test -- --test-threads=100

#![cfg(all(feature = "mcp-stdio", feature = "tcp"))]

use netget::cli::Args;
use netget::mcp_stdio::tools::NetGetMcpService;
use netget::settings::Settings;

use clap::Parser;
use rmcp::model::CallToolRequestParams;
use rmcp::{serve_client, RoleClient, ServiceExt};

/// Build an in-process client connected to a fresh NetGet MCP server.
async fn connect() -> rmcp::service::RunningService<RoleClient, ()> {
    // Parse a default Args (no CLI flags) so every field gets its default value.
    let args = Args::parse_from(["netget"]);
    let service = NetGetMcpService::new(&args, Settings::default())
        .await
        .expect("service creation");

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);

    // Serve the NetGet MCP server on one end of the duplex.
    tokio::spawn(async move {
        if let Ok(server) = service.serve(server_io).await {
            let _ = server.waiting().await;
        }
    });

    // The unit type `()` is a no-capability MCP client handler.
    serve_client((), client_io).await.expect("client handshake")
}

#[tokio::test]
async fn initialize_and_list_tools() {
    let client = connect().await;

    // The handshake populated server info.
    let info = client.peer_info().expect("server info");
    assert_eq!(info.server_info.name, "netget");

    // Core management tools must be advertised.
    let tools = client.list_all_tools().await.expect("list tools");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "list_protocols",
        "start_server",
        "stop_server",
        "get_status",
        "list_access_logs",
        "get_access_log",
    ] {
        assert!(
            names.contains(&expected),
            "expected tool '{}' in {:?}",
            expected,
            names
        );
    }

    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn call_list_protocols_returns_tcp() {
    let client = connect().await;

    let mut params = CallToolRequestParams::new("list_protocols");
    params.arguments = serde_json::json!({ "type": "server" })
        .as_object()
        .cloned();

    let result = client
        .call_tool(params)
        .await
        .expect("call list_protocols");

    assert_ne!(result.is_error, Some(true), "tool reported an error");

    // The rendered markdown should mention the TCP protocol that is compiled in.
    let text = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect::<String>()
        .to_lowercase();
    assert!(text.contains("tcp"), "expected tcp in output: {}", text);

    client.cancel().await.expect("shutdown");
}
