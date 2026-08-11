//! What an NNTP client gets when the LLM backend fails: a 4xx, never silence and never a 2xx.
//!
//! NNTP is strictly one response line per command, so a missing response does not merely delay
//! the client - it desynchronises the session, because the next command's reply is read as the
//! answer to this one. Both failure points are now answered:
//!
//! * the greeting, where RFC 3977 §5.1 defines `400 service temporarily unavailable` as a legal
//!   greeting, after which the server closes;
//! * any command, answered `403` ("internal fault or problem preventing action being taken"),
//!   which leaves the session usable.
//!
//! Every code here is 4xx. None of them can be confused with 200/201 (ready), 211 (group
//! selected), 220 (article follows) or 281 (authentication accepted).

#![cfg(all(test, feature = "nntp"))]

use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

async fn read_line(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> E2EResult<String> {
    let mut line = String::new();
    let n = tokio::time::timeout(Duration::from_secs(20), reader.read_line(&mut line))
        .await
        .map_err(|_| {
            "No NNTP response within 20s - the server went silent on LLM failure, which is the \
             exact defect this test exists to catch"
        })??;
    if n == 0 {
        return Err("NNTP connection closed without a response".into());
    }
    Ok(line)
}

/// The greeting fails: `400`, then EOF.
#[tokio::test]
async fn test_nntp_answers_400_when_greeting_llm_fails() -> E2EResult<()> {
    let prompt = "listen on port {AVAILABLE_PORT} via nntp. Serve comp.lang.rust";

    let config = NetGetConfig::new_no_scripts(prompt).with_mock(|mock| {
        mock.on_instruction_containing("via nntp")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "nntp",
                    "instruction": "Serve comp.lang.rust"
                }
            ]))
            .expect_calls(1)
            .and()
        // No rule for `nntp_command_received`, so the GREETING event fails.
    });

    let server = start_netget_server(config).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let stream = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    let (read_half, _write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let greeting = read_line(&mut reader).await?;
    println!("NNTP greeting: {}", greeting.trim());
    assert!(
        greeting.starts_with("400 "),
        "expected 400 (service temporarily unavailable) instead of a 200 that cannot be \
         honoured, got: {greeting}"
    );
    assert!(
        greeting.contains("netget"),
        "the text should name the source of the failure: {greeting}"
    );

    let mut trailing = String::new();
    let n = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut trailing))
        .await
        .map_err(|_| "the server did not close the connection after the 400 greeting")??;
    assert_eq!(n, 0, "expected EOF after 400, got: {trailing}");

    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}

/// A command fails after a good greeting: `403`, and the session stays open.
#[tokio::test]
async fn test_nntp_answers_403_when_command_llm_fails() -> E2EResult<()> {
    let prompt = "listen on port {AVAILABLE_PORT} via nntp. Greet, then serve comp.lang.rust";

    let config = NetGetConfig::new_no_scripts(prompt).with_mock(|mock| {
        mock.on_instruction_containing("via nntp")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "nntp",
                    "instruction": "Greet, then serve comp.lang.rust"
                }
            ]))
            .expect_calls(1)
            .and()
            // Only the greeting is answered. LIST is not.
            .on_event("nntp_command_received")
            .and_event_data_contains("command", "GREETING")
            .respond_with_actions(serde_json::json!([
                {"type": "send_nntp_response", "code": 200, "text": "NetGet NNTP ready"}
            ]))
            .expect_calls(1)
            .and()
    });

    let server = start_netget_server(config).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let stream = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let greeting = read_line(&mut reader).await?;
    println!("NNTP greeting: {}", greeting.trim());
    assert!(
        greeting.starts_with("200 "),
        "expected the mocked 200 greeting, got: {greeting}"
    );

    write_half.write_all(b"LIST\r\n").await?;
    write_half.flush().await?;

    let reply = read_line(&mut reader).await?;
    println!("NNTP LIST reply: {}", reply.trim());
    assert!(
        reply.starts_with("403 "),
        "expected 403 (internal fault) for a command the backend could not answer, got: {reply}"
    );
    assert!(
        !reply.starts_with('2'),
        "a backend failure must never be reported as success: {reply}"
    );

    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}
