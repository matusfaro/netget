//! Live-LLM TCP suite.
//!
//! Setup and request handling are deliberately separate tests: setup tests
//! evaluate the model's `open_server` behavior from a natural-language prompt
//! (LiveProtocolTest); request tests start the server deterministically with
//! `--server` (no model call) so the only unpredictable behavior under test
//! is the model answering the request event (LiveRequestTest).

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running TCP server.
#[tokio::test]
async fn tcp_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("tcp")
        .setup_prompt(
            "Start a TCP server on port {AVAILABLE_PORT}. \
             It should reply to every message with the text SETUP-OK.",
        )
        .start()
        .await?;

    // Setup evidence: the port the model chose actually accepts connections.
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request type: raw data event, echo behavior. The model must return the
/// client's own payload.
#[tokio::test]
async fn tcp_echo_roundtrip() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "tcp",
        "Echo back exactly the data you receive, unchanged, on every message.",
    )
    .start()
    .await?;

    let marker = "netget-live-echo-7431";
    let response = server.tcp_roundtrip(marker.as_bytes()).await?;
    let result =
        expect_contains(&as_text(&response), marker).and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Request type: raw data event, fixed reply. The model must answer with the
/// instructed canned string regardless of input.
#[tokio::test]
async fn tcp_canned_reply() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "tcp",
        "Whenever any data arrives on a connection, reply with exactly: ACK-GRANTED",
    )
    .start()
    .await?;

    let response = server.tcp_roundtrip(b"hello, anyone there?").await?;
    let result =
        expect_contains(&as_text(&response), "ACK-GRANTED").and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Request type: content-dependent reply. The model must reason about the
/// payload, not just pattern-match the instruction.
#[tokio::test]
async fn tcp_conditional_reply() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "tcp",
        "When a client sends the word PING reply with PONG-42. \
         For anything else reply with UNKNOWN-COMMAND.",
    )
    .start()
    .await?;

    let ping = server.tcp_roundtrip(b"PING").await?;
    let ping_result = expect_contains(&as_text(&ping), "PONG-42");

    let other = server.tcp_roundtrip(b"HELLO").await?;
    let other_result = expect_contains(&as_text(&other), "UNKNOWN-COMMAND")
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    ping_result?;
    other_result
}
