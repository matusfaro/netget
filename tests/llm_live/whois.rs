//! Live-LLM WHOIS suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale.

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running WHOIS server.
#[tokio::test]
async fn whois_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("whois")
        .setup_prompt(
            "Start a WHOIS server on port {AVAILABLE_PORT}. \
             Answer every query with a plausible whois record.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request type: whois query → instructed registrar in the record.
#[tokio::test]
async fn whois_query_instructed_record() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "whois",
        "For whois queries about netget-live.example, reply with a whois \
         record whose registrar line reads: Registrar: NETGET-LIVE-REGISTRAR",
    )
    .start()
    .await?;

    let response = server.tcp_roundtrip(b"netget-live.example\r\n").await?;
    let result = expect_contains(&as_text(&response), "NETGET-LIVE-REGISTRAR")
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
