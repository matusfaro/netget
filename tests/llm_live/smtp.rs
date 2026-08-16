//! Live-LLM SMTP suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. SMTP is greeting-first: the greeting itself
//! is a model call (connection-open event), the EHLO reply a second one.

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running SMTP server.
#[tokio::test]
async fn smtp_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("smtp")
        .setup_prompt(
            "Start an SMTP server on port {AVAILABLE_PORT} for the domain \
             netget.example.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request types: connection-open greeting (must be a 220 line) and EHLO
/// (must be answered 250). Two live model calls, both asserted.
#[tokio::test]
async fn smtp_greeting_and_ehlo() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "smtp",
        "You are an SMTP server for the domain netget.example. Greet new \
         connections with code 220 and the banner text netget-live-smtp. \
         Answer EHLO with a 250 response listing the domain.",
    )
    .start()
    .await?;

    let (greeting, response) = server
        .tcp_greeting_roundtrip(b"EHLO client.test\r\n")
        .await?;
    let result = expect_contains(&as_text(&greeting), "220")
        .and(expect_contains(&as_text(&greeting), "netget-live-smtp"))
        .and(expect_contains(&as_text(&response), "250"))
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
