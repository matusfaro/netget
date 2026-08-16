//! Live-LLM NNTP suite.
//!
//! Protocol facts this encodes (src/server/nntp/actions.rs, mod.rs):
//! - one event, `nntp_command_received { command }`; on accept it fires with
//!   the literal command `"GREETING"`, and the answer is the banner;
//! - the greeting must be a 200 (posting allowed) or 201 (no posting) status
//!   line — RFC 3977 §5.1;
//! - `send_nntp_list` frames a multi-line block: `215` header, one line per
//!   group, then a lone `.` terminator. A client reads until that dot, so a
//!   missing terminator hangs the session;
//! - `send_nntp_group` frames `211 <count> <low> <high> <name>`.

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

#[tokio::test]
async fn nntp_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("nntp")
        .setup_prompt(
            "Start an NNTP news server on port {AVAILABLE_PORT} carrying the \
             newsgroup comp.lang.rust.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Greeting status line, then a dot-terminated LIST block.
#[tokio::test]
async fn nntp_greeting_and_list_block() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "nntp",
        "You are an NNTP news server carrying exactly one newsgroup, \
         comp.lang.rust, with articles 1 to 50. Greet new connections with a \
         200 status line. Answer LIST with the newsgroup list.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let greeting = session.read("NNTP greeting").await?;
    let list = session.exchange(b"LIST\r\n", "LIST reply").await?;

    let greeting_ok = if greeting.starts_with("200") || greeting.starts_with("201") {
        Ok(())
    } else {
        Err(format!(
            "NNTP greeting must be 200 or 201 (RFC 3977 §5.1); got {:?}",
            greeting
        )
        .into())
    };
    let list_status = if list.starts_with("215") {
        Ok(())
    } else {
        Err(format!("LIST must open with a 215 status line; got {:?}", list).into())
    };
    // The lone dot is the multi-line terminator: without it a real reader hangs.
    let terminated = if list.lines().any(|l| l.trim_end() == ".") {
        Ok(())
    } else {
        Err(format!(
            "LIST block is not terminated by a lone '.' line — a real NNTP \
             client reads until that dot and would hang. Got: {:?}",
            list
        )
        .into())
    };
    let has_group = if list.contains("comp.lang.rust") {
        Ok(())
    } else {
        Err(format!("LIST did not carry the instructed group. Got: {:?}", list).into())
    };

    let result = greeting_ok
        .and(list_status)
        .and(has_group)
        .and(terminated)
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// GROUP → `211 <count> <low> <high> <name>`, the counts a reader uses to
/// size its article range.
#[tokio::test]
async fn nntp_group_selection() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "nntp",
        "You are an NNTP news server. The group comp.lang.rust holds 50 \
         articles numbered 1 to 50. Answer the GROUP command for it with the \
         group status line reporting that count and range.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let _greeting = session.read("NNTP greeting").await?;
    let group = session
        .exchange(b"GROUP comp.lang.rust\r\n", "GROUP reply")
        .await?;

    let result = (|| -> E2EResult<()> {
        if !group.starts_with("211") {
            return Err(format!(
                "GROUP must answer 211 <count> <low> <high> <name>; got {:?}",
                group
            )
            .into());
        }
        if !group.contains("comp.lang.rust") {
            return Err(format!("211 line must name the group; got {:?}", group).into());
        }
        // RFC 3977 §6.1.1: the three fields after 211 are numeric.
        let fields: Vec<&str> = group.split_whitespace().collect();
        if fields.len() < 5 || fields[1..4].iter().any(|f| f.parse::<u64>().is_err()) {
            return Err(format!(
                "211 line must carry numeric count/low/high before the group \
                 name; got {:?}",
                group
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
