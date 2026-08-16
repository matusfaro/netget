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
//! COVERS: telnet: telnet_message_received, telnet_connection_opened

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

/// `telnet_connection_opened` fires only under `send_first`, and it is the
/// only chance to show a login banner before the user types. A prompt must
/// *not* end with a newline — `send_telnet_prompt` writes verbatim so the
/// cursor stays on the prompt line, which is what a real terminal expects.
#[tokio::test]
async fn telnet_connection_opened_shows_a_login_prompt() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "telnet",
        "You are a Telnet login service. The moment a client connects, before \
         it types anything, show one banner line reading NETGET-LOGIN-7431 \
         and then the prompt 'login: ' on its own, with no newline after the \
         prompt so the cursor stays on that line.",
    )
    .server_params(serde_json::json!({ "send_first": true }))
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let greeting = session.read("connect-time banner").await?;

    let framing = if greeting.trim_end().ends_with(':') || greeting.ends_with(' ') {
        Ok(())
    } else {
        Err(format!(
            "the prompt must be written without a trailing newline \
             (send_telnet_prompt), or the user types on the line below it. Got: {:?}",
            greeting
        )
        .into())
    };

    let result = expect_contains(&greeting, "NETGET-LOGIN-7431")
        .and(framing)
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
