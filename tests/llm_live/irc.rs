//! Live-LLM IRC suite.
//!
//! Protocol facts this encodes (src/server/irc/actions.rs, mod.rs):
//! - one event, `irc_message_received { message }`, carrying a single line
//!   with the CRLF stripped. The server never speaks first, so registration
//!   starts with the client's NICK/USER;
//! - `send_irc_welcome` frames `:server 001 <nick> :<text>` — numeric 001 is
//!   what tells a real client registration succeeded;
//! - `send_irc_pong` frames `PONG :<token>`, and the token must be the one
//!   the client sent or the client treats the link as dead.
//!
//! COVERS: irc: irc_message_received

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

#[tokio::test]
async fn irc_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("irc")
        .setup_prompt(
            "Start an IRC server on port {AVAILABLE_PORT} that registers users \
             and welcomes them.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// NICK+USER registration burst → numeric 001 naming the nick.
#[tokio::test]
async fn irc_registration_welcome_numeric() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "irc",
        "You are an IRC server named irc.netget.example. When a client \
         completes registration by sending NICK and USER, welcome it with the \
         001 welcome numeric addressed to that nickname.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    // Registration is two lines; the server processes them sequentially and
    // usually answers only once the USER line completes the pair.
    session.send(b"NICK liveuser\r\n").await?;
    let registration = session
        .exchange(b"USER liveuser 0 * :Live User\r\n", "registration reply")
        .await?;

    let result = (|| -> E2EResult<()> {
        // ":<server> 001 <nick> :<text>" — numeric first, nick second.
        let welcome = registration
            .lines()
            .find(|l| l.contains(" 001 "))
            .ok_or_else(|| {
                format!(
                    "no 001 welcome numeric — a real IRC client stays \
                     unregistered without it. Got: {:?}",
                    registration
                )
            })?;
        if !welcome.contains("liveuser") {
            return Err(format!(
                "001 numeric must be addressed to the registered nick; got {:?}",
                welcome
            )
            .into());
        }
        if !welcome.starts_with(':') {
            return Err(format!(
                "001 numeric must carry a server prefix (':server 001 nick :text'); got {:?}",
                welcome
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// PING → PONG echoing the client's token.
#[tokio::test]
async fn irc_ping_token_echo() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "irc",
        "You are an IRC server. Answer a PING from a client with a PONG \
         carrying back the same token the client sent.",
    )
    .start()
    .await?;

    let token = "LIVE7431";
    let mut session = server.tcp_session().await?;
    let pong = session
        .exchange(format!("PING :{}\r\n", token).as_bytes(), "PONG reply")
        .await?;

    let result = (|| -> E2EResult<()> {
        if !pong.to_uppercase().contains("PONG") {
            return Err(format!("PING must be answered with PONG; got {:?}", pong).into());
        }
        if !pong.contains(token) {
            return Err(format!(
                "PONG must echo the client's token {:?} or the client considers \
                 the link dead; got {:?}",
                token, pong
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
