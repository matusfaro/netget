//! Live-LLM UDP suite. Setup (LiveProtocolTest, model-driven) and request
//! handling (LiveRequestTest, deterministic `--server` start) are separate
//! tests — see tcp.rs for the rationale.

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running UDP server.
#[tokio::test]
async fn udp_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("udp")
        .setup_prompt(
            "Start a UDP server on port {AVAILABLE_PORT}. \
             Reply to every datagram with the text SETUP-OK.",
        )
        .start()
        .await?;
    server.finish().await
}

/// Request type: datagram event, echo behavior.
#[tokio::test]
async fn udp_echo_roundtrip() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "udp",
        "Echo each datagram back to the sender exactly as received.",
    )
    .start()
    .await?;

    let marker = "netget-live-udp-echo-5127";
    let response = server.udp_roundtrip(marker.as_bytes()).await?;
    let result =
        expect_contains(&as_text(&response), marker).and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Request type: datagram event, fixed reply.
#[tokio::test]
async fn udp_canned_reply() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "udp",
        "Whenever a datagram arrives, reply with exactly: DATAGRAM-ACK",
    )
    .start()
    .await?;

    let response = server.udp_roundtrip(b"status check").await?;
    let result = expect_contains(&as_text(&response), "DATAGRAM-ACK")
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
