//! Live-LLM SOCKS5 suite.
//!
//! Protocol facts this encodes (src/server/socks5/mod.rs, filter.rs):
//! - method selection (phase 1) is answered by Rust with no model call, so
//!   the model's first decision point is the CONNECT request;
//! - `socks5_connect_request { target, username }` is only raised when the
//!   filter says so: the default `filter_mode` is `selective` with empty
//!   patterns, which never asks. These tests pass `filter_mode: "ask_llm"`,
//!   without which the protocol would answer entirely without the model;
//! - the decision is read from the **action name**: `allow_socks5_connect`
//!   permits, `deny_socks5_connect` refuses, and no decision action denies
//!   (fail-closed). A denial must still be a well-formed reply — RFC 1928
//!   §6 `[VER, REP, RSV, ATYP, BND.ADDR, BND.PORT]` — not a dropped socket.
//!
//! COVERS: socks5: socks5_connect_request

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest, LiveServer};
use crate::helpers::E2EResult;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Phase 1: offer "no authentication" and require the 2-byte selection.
/// This is deterministic Rust, so it must succeed before any model call.
async fn handshake_no_auth(addr: &str) -> E2EResult<TcpStream> {
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).await?;
    if reply[0] != 0x05 {
        return Err(format!("bad SOCKS version in method selection: {:#04x}", reply[0]).into());
    }
    if reply[1] != 0x00 {
        return Err(format!(
            "server did not select 'no authentication' (0x00); got method {:#04x}",
            reply[1]
        )
        .into());
    }
    Ok(stream)
}

/// Phase 3: CONNECT to an IPv4 target, returning the reply code (REP).
async fn connect_ipv4(stream: &mut TcpStream, ip: [u8; 4], port: u16) -> E2EResult<u8> {
    let mut req = vec![0x05, 0x01, 0x00, 0x01];
    req.extend_from_slice(&ip);
    req.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&req).await?;

    let mut head = [0u8; 4];
    tokio::time::timeout(
        crate::helpers::llm_live::FIRST_BYTE_TIMEOUT,
        stream.read_exact(&mut head),
    )
    .await
    .map_err(|_| "no SOCKS5 CONNECT reply (model never answered the event)".to_string())??;

    if head[0] != 0x05 {
        return Err(format!("bad SOCKS version in CONNECT reply: {:#04x}", head[0]).into());
    }
    // Drain the bound address so the reply is proven well-formed, not truncated.
    match head[3] {
        0x01 => {
            let mut rest = [0u8; 6];
            stream.read_exact(&mut rest).await?;
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut rest = vec![0u8; len[0] as usize + 2];
            stream.read_exact(&mut rest).await?;
        }
        0x04 => {
            let mut rest = [0u8; 18];
            stream.read_exact(&mut rest).await?;
        }
        other => return Err(format!("unsupported ATYP {:#04x} in CONNECT reply", other).into()),
    }
    println!("📥 socks5 CONNECT reply code {:#04x}", head[1]);
    Ok(head[1])
}

fn ask_llm_server(instruction: &str) -> LiveRequestTest {
    // Without ask_llm the filter answers on its own and the model is never consulted.
    LiveRequestTest::new("socks5", instruction).server_params(json!({
        "auth_methods": ["none"],
        "filter_mode": "ask_llm"
    }))
}

#[tokio::test]
async fn socks5_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("socks5")
        .setup_prompt("Start a SOCKS5 proxy server on port {AVAILABLE_PORT}.")
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// The model allows a CONNECT: reply code must be 0x00 (succeeded).
#[tokio::test]
async fn socks5_connect_allowed() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    // A real target must exist, or even an allowed CONNECT fails at dial time.
    let target = TcpListener::bind("127.0.0.1:0").await?;
    let target_port = target.local_addr()?.port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = target.accept().await {
            let _ = sock.write_all(b"hello from target").await;
        }
    });

    let server = ask_llm_server(
        "You are a SOCKS5 proxy. Allow every CONNECT request to a loopback \
         address (127.0.0.1). Deny anything else.",
    )
    .start()
    .await?;

    let mut stream = handshake_no_auth(&server.addr()).await?;
    let code = connect_ipv4(&mut stream, [127, 0, 0, 1], target_port).await?;

    let result = if code == 0x00 {
        Ok(())
    } else {
        Err(format!(
            "expected REP 0x00 (succeeded) for an allowed CONNECT; got {:#04x}",
            code
        )
        .into())
    }
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// The model denies a CONNECT: the reply must be a well-formed refusal, not a
/// dropped connection, and must not be 0x00.
#[tokio::test]
async fn socks5_connect_denied_is_wellformed() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = ask_llm_server(
        "You are a SOCKS5 proxy enforcing a security policy: deny every \
         CONNECT request to port 9999. Allow nothing on that port.",
    )
    .start()
    .await?;

    let mut stream = handshake_no_auth(&server.addr()).await?;
    let code = connect_ipv4(&mut stream, [127, 0, 0, 1], 9999).await?;

    let result = if code != 0x00 {
        // 0x02 is "connection not allowed by ruleset" (RFC 1928 §6).
        println!("✅ denial reported as REP {:#04x}", code);
        Ok(())
    } else {
        Err(
            "model's denial produced REP 0x00 (succeeded) — the client would \
             believe the tunnel is open"
                .to_string()
                .into(),
        )
    }
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

// Keeps the LiveServer import meaningful for readers of the helper above.
#[allow(dead_code)]
fn _addr_of(server: &LiveServer) -> String {
    server.addr()
}
