//! Live-LLM RTSP suite.
//!
//! Protocol facts this encodes (src/server/rtsp/actions.rs, mod.rs):
//! - each method raises its own event (`rtsp_options`, `rtsp_describe`, …)
//!   carrying `cseq`, `uri` and `method`;
//! - the server frames the status line and echoes `CSeq` itself, so what the
//!   model owns is choosing the right action and its payload;
//! - `rtsp_options_response` must advertise methods in a `Public:` header —
//!   that header is how a client learns what the server supports;
//! - `rtsp_describe_response` carries SDP, which must describe the media
//!   (`m=audio` line with an `a=rtpmap` for the payload type) or a player
//!   cannot set up a session.
//!
//! COVERS: rtsp: rtsp_options, rtsp_describe

use crate::helpers::llm_live::{as_text, live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

#[tokio::test]
async fn rtsp_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("rtsp")
        .setup_prompt("Start an RTSP media server on port {AVAILABLE_PORT} streaming audio.")
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// OPTIONS → 200 with the CSeq echoed and a Public header listing methods.
#[tokio::test]
async fn rtsp_options_advertises_methods() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "rtsp",
        "You are an RTSP 1.0 media server. Answer OPTIONS by advertising the \
         methods you support: OPTIONS, DESCRIBE, SETUP, PLAY and TEARDOWN.",
    )
    .start()
    .await?;

    let request = format!(
        "OPTIONS rtsp://127.0.0.1:{}/stream RTSP/1.0\r\nCSeq: 3\r\n\r\n",
        server.port
    );
    let response = as_text(&server.tcp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("RTSP/1.0 200") {
            return Err(format!("OPTIONS must answer RTSP/1.0 200; got {:?}", response).into());
        }
        // CSeq correlation: a client matches responses to requests by it.
        if !response.contains("CSeq: 3") {
            return Err(format!(
                "response must echo 'CSeq: 3' from the request; got {:?}",
                response
            )
            .into());
        }
        if !response.to_uppercase().contains("PUBLIC:") {
            return Err(format!(
                "OPTIONS response must carry a Public: header listing methods \
                 (RFC 2326 §10.1); got {:?}",
                response
            )
            .into());
        }
        for method in ["DESCRIBE", "SETUP", "PLAY"] {
            if !response.contains(method) {
                return Err(format!(
                    "Public: header omits {} — a player probes this list before \
                     setting up. Got: {:?}",
                    method, response
                )
                .into());
            }
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// DESCRIBE → SDP describing an audio stream.
#[tokio::test]
async fn rtsp_describe_returns_sdp() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "rtsp",
        "You are an RTSP 1.0 media server serving one G.711 mu-law (PCMU, \
         payload type 0, 8000 Hz) audio stream. Answer DESCRIBE with the SDP \
         describing it.",
    )
    .start()
    .await?;

    let request = format!(
        "DESCRIBE rtsp://127.0.0.1:{}/stream RTSP/1.0\r\nCSeq: 4\r\nAccept: application/sdp\r\n\r\n",
        server.port
    );
    let response = as_text(&server.tcp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("RTSP/1.0 200") {
            return Err(format!("DESCRIBE must answer RTSP/1.0 200; got {:?}", response).into());
        }
        if !response.contains("CSeq: 4") {
            return Err(format!("response must echo 'CSeq: 4'; got {:?}", response).into());
        }
        if !response.contains("application/sdp") {
            return Err(format!(
                "DESCRIBE body must be typed application/sdp; got {:?}",
                response
            )
            .into());
        }
        // SDP essentials: a session version line and a media description.
        if !response.contains("v=0") {
            return Err(format!("SDP must start with 'v=0'; got {:?}", response).into());
        }
        if !response.contains("m=audio") {
            return Err(format!(
                "SDP must carry an 'm=audio' media line or a player has nothing \
                 to SETUP; got {:?}",
                response
            )
            .into());
        }
        if !response.to_uppercase().contains("PCMU") {
            return Err(format!(
                "SDP must map the instructed PCMU payload (a=rtpmap:0 PCMU/8000); got {:?}",
                response
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
