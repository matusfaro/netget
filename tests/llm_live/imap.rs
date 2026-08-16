//! Live-LLM IMAP suite.
//!
//! Protocol facts this encodes (src/server/imap/actions.rs, mod.rs):
//! - the server greets unconditionally: `imap_connection` fires on accept and
//!   the answer must be an untagged `* OK` line;
//! - `LOGIN` never reaches `imap_command` — it raises `imap_auth
//!   { tag, username, password }`;
//! - every other command raises `imap_command { tag, command, ... }`, and a
//!   command completes only when a **tagged** line carrying the client's own
//!   tag arrives. Untagged `*` data must come first.
//!
//! Tag correlation is the assertion that matters: a real client (async-imap,
//! Thunderbird) blocks forever on a missing or mismatched tag.

use crate::helpers::llm_live::{
    expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

#[tokio::test]
async fn imap_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("imap")
        .setup_prompt("Start an IMAP server on port {AVAILABLE_PORT} with one mailbox, INBOX.")
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// `imap_connection` → `* OK` greeting, then `imap_command` for CAPABILITY:
/// untagged `* CAPABILITY` advertising IMAP4rev1, then the tagged completion
/// echoing the client's tag `a1`.
#[tokio::test]
async fn imap_greeting_and_capability_tag_echo() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "imap",
        "You are an IMAP4rev1 server for the mailbox INBOX. Greet new \
         connections with an untagged * OK line naming the server. Answer \
         CAPABILITY with an untagged * CAPABILITY line advertising IMAP4rev1, \
         followed by a tagged OK completion that repeats the client's tag.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let greeting = session.read("IMAP greeting").await?;
    let capability = session
        .exchange(b"a1 CAPABILITY\r\n", "CAPABILITY reply")
        .await?;

    let greeting_ok = if greeting.trim_start().starts_with("* OK") {
        Ok(())
    } else {
        Err(format!(
            "IMAP greeting must start with the untagged '* OK' (RFC 3501 §7.1.1); got {:?}",
            greeting
        )
        .into())
    };
    // The tagged completion is what unblocks a real client.
    let tag_echo = if capability
        .lines()
        .any(|l| l.starts_with("a1 OK") || l.starts_with("a1 ok"))
    {
        Ok(())
    } else {
        Err(format!(
            "no tagged completion 'a1 OK' — a real IMAP client blocks until its \
             own tag comes back. Got: {:?}",
            capability
        )
        .into())
    };
    let untagged = if capability.lines().any(|l| l.starts_with("* CAPABILITY")) {
        Ok(())
    } else {
        Err(format!(
            "no untagged '* CAPABILITY' line before the completion. Got: {:?}",
            capability
        )
        .into())
    };

    let result = greeting_ok
        .and(untagged)
        .and(expect_contains(&capability, "IMAP4rev1"))
        .and(tag_echo)
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// LOGIN raises `imap_auth` (not `imap_command`); the answer must be a tagged
/// completion carrying that command's tag.
#[tokio::test]
async fn imap_login_tagged_completion() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "imap",
        "You are an IMAP4rev1 server. Greet connections with an untagged * OK \
         line. Accept the LOGIN of user alice with password secret, answering \
         with a tagged OK completion that repeats the client's tag.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let _greeting = session.read("IMAP greeting").await?;
    let login = session
        .exchange(b"a7 LOGIN alice secret\r\n", "LOGIN reply")
        .await?;

    let result = if login.lines().any(|l| l.to_uppercase().starts_with("A7 OK")) {
        Ok(())
    } else {
        Err(format!(
            "LOGIN must complete with 'a7 OK' (the client's own tag); got {:?}",
            login
        )
        .into())
    }
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
