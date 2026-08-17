//! Live-LLM JSON-RPC suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. The response must be valid JSON-RPC 2.0
//! with the request's numeric `id` echoed — the protocol's correlation field.
//!
//! COVERS: jsonrpc: jsonrpc_method_call

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running JSON-RPC server.
#[tokio::test]
async fn jsonrpc_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("jsonrpc")
        .setup_prompt("Start a JSON-RPC server on port {AVAILABLE_PORT}.")
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request type: method call → JSON-RPC 2.0 response, id echoed, instructed
/// result value.
#[tokio::test]
async fn jsonrpc_method_call() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "jsonrpc",
        "You are a JSON-RPC 2.0 server. For the method ping, return the \
         string result netget-live-pong.",
    )
    .start()
    .await?;

    let (status, body) = server
        .http_request(
            "POST",
            "/",
            Some((
                "application/json",
                r#"{"jsonrpc":"2.0","method":"ping","id":7431}"#.to_string(),
            )),
        )
        .await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("Expected HTTP 200, got {}. Body: {}", status, body).into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("Body is not valid JSON ({}): {}", e, body))?;
        if json["id"].as_u64() != Some(7431) {
            return Err(format!("Request id 7431 not echoed. Body: {}", body).into());
        }
        let result_text = json["result"].to_string();
        if !result_text.contains("netget-live-pong") {
            return Err(format!(
                "Expected result containing netget-live-pong. Body: {}",
                body
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
