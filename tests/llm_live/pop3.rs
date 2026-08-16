//! Live-LLM POP3 suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. POP3 is greeting-first like SMTP.

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running POP3 server.
#[tokio::test]
async fn pop3_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("pop3")
        .setup_prompt("Start a POP3 server on port {AVAILABLE_PORT} with an empty mailbox.")
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request types: +OK greeting on connect, +OK to USER.
#[tokio::test]
async fn pop3_greeting_and_user() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "pop3",
        "You are a POP3 server. Greet new connections with +OK and the banner \
         netget-live-pop3. Answer the USER command with +OK. Every command \
         must be answered with a POP3 response line on the wire; never leave \
         a command unanswered.",
    )
    .start()
    .await?;

    let (greeting, response) = server.tcp_greeting_roundtrip(b"USER tester\r\n").await?;
    let result = expect_contains(&as_text(&greeting), "+OK")
        .and(expect_contains(&as_text(&greeting), "netget-live-pop3"))
        .and(expect_contains(&as_text(&response), "+OK"))
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
