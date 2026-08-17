//! Live-LLM FTP suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. FTP is greeting-first like SMTP.
//!
//! COVERS: ftp: ftp_command

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running FTP server.
#[tokio::test]
async fn ftp_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("ftp")
        .setup_prompt("Start an FTP server on port {AVAILABLE_PORT}.")
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request types: 220 greeting on connect, 331 to USER (password required).
#[tokio::test]
async fn ftp_greeting_and_user() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "ftp",
        "You are an FTP server. Greet new connections with code 220 and the \
         banner netget-live-ftp. Answer the USER command with code 331 asking \
         for a password.",
    )
    .start()
    .await?;

    let (greeting, response) = server.tcp_greeting_roundtrip(b"USER anonymous\r\n").await?;
    let result = expect_contains(&as_text(&greeting), "220")
        .and(expect_contains(&as_text(&greeting), "netget-live-ftp"))
        .and(expect_contains(&as_text(&response), "331"))
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
