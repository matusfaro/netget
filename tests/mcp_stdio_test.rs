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
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::{serve_client, RoleClient, ServiceExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Build an in-process client connected to a NetGet MCP server started with `argv`.
async fn connect_args(argv: &[&str]) -> rmcp::service::RunningService<RoleClient, ()> {
    let args = Args::parse_from(argv);
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

/// Build an in-process client connected to a fresh default NetGet MCP server.
async fn connect() -> rmcp::service::RunningService<RoleClient, ()> {
    connect_args(&["netget"]).await
}

/// Call a tool with JSON arguments and return the result.
async fn call(
    client: &rmcp::service::RunningService<RoleClient, ()>,
    name: &'static str,
    args: serde_json::Value,
) -> CallToolResult {
    let mut params = CallToolRequestParams::new(name);
    params.arguments = args.as_object().cloned();
    client.call_tool(params).await.expect("call tool")
}

/// Concatenate the text content of a tool result.
fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect()
}

/// Parse the digits following `marker` in `text` (e.g. "port " → 54321).
fn parse_number_after(text: &str, marker: &str) -> u64 {
    let idx = text
        .find(marker)
        .unwrap_or_else(|| panic!("marker {:?} not found in: {}", marker, text));
    text[idx + marker.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("no number after {:?} in: {}", marker, text))
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

/// In `--llm-agent` mode the agent-queue tools are advertised.
#[tokio::test]
async fn agent_mode_exposes_queue_tools() {
    let client = connect_args(&["netget", "--llm-agent"]).await;

    let tools = client.list_all_tools().await.expect("list tools");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "get_next_llm_request",
        "answer_llm_request",
        "list_llm_requests",
    ] {
        assert!(
            names.contains(&expected),
            "expected tool '{}' in {:?}",
            expected,
            names
        );
    }

    // get_status should report the agent backend.
    let status = call(&client, "get_status", serde_json::json!({})).await;
    assert!(
        text_of(&status).to_lowercase().contains("agent"),
        "expected agent backend in status: {}",
        text_of(&status)
    );

    client.cancel().await.expect("shutdown");
}

/// End-to-end: a real TCP server queues its LLM request, the MCP agent fetches it
/// and answers, and the answer drives the socket response.
#[tokio::test]
async fn agent_answers_tcp_request_end_to_end() {
    let client = connect_args(&["netget", "--llm-agent"]).await;

    // Start a TCP server on an OS-assigned port.
    let started = call(
        &client,
        "start_server",
        serde_json::json!({ "protocol": "tcp", "port": 0 }),
    )
    .await;
    assert_ne!(started.is_error, Some(true), "start_server errored");

    // Discover the bound port.
    let servers = call(&client, "list_servers", serde_json::json!({})).await;
    let port = parse_number_after(&text_of(&servers), "port ") as u16;
    assert!(port > 0, "expected a bound port, got {}", port);

    // Connect a real client and send a line — this triggers tcp_data_received,
    // which enqueues an LLM request the agent must answer.
    let mut sock = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("tcp connect");
    sock.write_all(b"PING\n").await.expect("tcp write");

    // Long-poll for the queued request.
    let next = call(
        &client,
        "get_next_llm_request",
        serde_json::json!({ "wait_seconds": 5 }),
    )
    .await;
    let next_text = text_of(&next);
    assert!(
        !next_text.contains("(no pending requests)"),
        "expected a queued request, got: {}",
        next_text
    );
    let request_id = parse_number_after(&next_text, "request #");

    // Answer it with a send_tcp_data action (one of the actions offered for this
    // event) so the server writes "PONG" back on the wire.
    let answered = call(
        &client,
        "answer_llm_request",
        serde_json::json!({
            "request_id": request_id,
            "actions": [
                { "type": "send_tcp_data", "data": "PONG\n" }
            ]
        }),
    )
    .await;
    assert_ne!(
        answered.is_error,
        Some(true),
        "answer_llm_request errored: {}",
        text_of(&answered)
    );

    // The socket should receive the agent's answer.
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(std::time::Duration::from_secs(3), sock.read(&mut buf))
        .await
        .expect("socket read timed out")
        .expect("socket read");
    let received = String::from_utf8_lossy(&buf[..n]);
    assert!(
        received.contains("PONG"),
        "expected PONG on the wire, got: {:?}",
        received
    );

    client.cancel().await.expect("shutdown");
}

/// A queued request that is never answered errors out after the configured timeout,
/// after which it can no longer be answered.
#[tokio::test]
async fn agent_request_times_out() {
    let client = connect_args(&["netget", "--llm-agent", "--llm-agent-timeout", "1"]).await;

    call(
        &client,
        "start_server",
        serde_json::json!({ "protocol": "tcp", "port": 0 }),
    )
    .await;
    let servers = call(&client, "list_servers", serde_json::json!({})).await;
    let port = parse_number_after(&text_of(&servers), "port ") as u16;

    let mut sock = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("tcp connect");
    sock.write_all(b"PING\n").await.expect("tcp write");

    // Claim the request but do not answer it.
    let next = call(
        &client,
        "get_next_llm_request",
        serde_json::json!({ "wait_seconds": 5 }),
    )
    .await;
    let request_id = parse_number_after(&text_of(&next), "request #");

    // Wait past the 1s answer timeout; the backend expires the request.
    tokio::time::sleep(std::time::Duration::from_millis(1600)).await;

    // Answering an expired request must now fail.
    let answered = call(
        &client,
        "answer_llm_request",
        serde_json::json!({ "request_id": request_id, "actions": [] }),
    )
    .await;
    assert_eq!(
        answered.is_error,
        Some(true),
        "expected answering an expired request to fail, got: {}",
        text_of(&answered)
    );

    client.cancel().await.expect("shutdown");
}
