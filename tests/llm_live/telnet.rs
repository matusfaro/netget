//! Live-LLM Telnet suite.
//!
//! Protocol facts this encodes (src/server/telnet/actions.rs):
//! - `telnet_message_received { message }` carries one line, LF-split and
//!   trimmed of the trailing CR;
//! - `send_telnet_line` appends exactly one CRLF, `send_telnet_message`
//!   writes verbatim, `send_telnet_prompt` writes with no newline.
//!
//! A Telnet client sends bare-LF lines; a well-formed answer comes back
//! CRLF-framed. The tests assert that framing, not just the text.
//!
//! COVERS: telnet: telnet_message_received

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

#[tokio::test]
async fn telnet_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("telnet")
        .setup_prompt(
            "Start a Telnet server on port {AVAILABLE_PORT} that answers each \
             line the user types.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// `telnet_message_received` → a CRLF-terminated line.
#[tokio::test]
async fn telnet_line_reply_is_crlf_terminated() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "telnet",
        "When a client sends a line of text, reply with one line of text \
         containing exactly: NETGET-LIVE-TELNET",
    )
    .start()
    .await?;

    // Telnet clients send bare LF; the server splits on \n.
    let response = server.tcp_roundtrip(b"help\n").await?;
    let text = as_text(&response);
    let framing = if text.contains("\r\n") {
        Ok(())
    } else {
        Err(format!(
            "Telnet reply is not CRLF-framed (send_telnet_line must append one \
             CRLF; a bare line leaves a real client mid-line). Got: {:?}",
            text
        )
        .into())
    };
    let result = expect_contains(&text, "NETGET-LIVE-TELNET")
        .and(framing)
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Content-dependent replies, two lines on one connection.
#[tokio::test]
async fn telnet_command_dispatch() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "telnet",
        "You are a Telnet command shell. Reply to the line 'time' with a line \
         reading TIME-OK. Reply to any other line with a line reading \
         UNKNOWN-COMMAND. Always answer every line.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let time_reply = session.exchange(b"time\n", "reply to 'time'").await?;
    let other_reply = session
        .exchange(b"frobnicate\n", "reply to unknown")
        .await?;

    let result = expect_contains(&time_reply, "TIME-OK")
        .and(expect_contains(&other_reply, "UNKNOWN-COMMAND"))
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
