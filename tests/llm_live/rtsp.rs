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
//!   cannot set up a session;
//! - SETUP, PLAY and TEARDOWN are a *sequence*: the server refuses a PLAY
//!   that arrives before a completed SETUP, so those tests share one
//!   connection by necessity, not by choice;
//! - `rtsp_play_response` describes the media the server should synthesize
//!   (tone/DTMF/silence), never samples — the RTP engine renders it;
//! - a method this server does not handle raises `rtsp_other`, whose answer
//!   is a status code the client can act on (501 for "not implemented").
//!
//! COVERS: rtsp: rtsp_options, rtsp_describe, rtsp_setup, rtsp_play, rtsp_teardown, rtsp_other

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

/// SETUP → 200 with the Transport and Session headers NetGet frames itself.
/// The model owns only the status code; what this grades is that it accepts
/// the transport rather than refusing a well-formed request.
#[tokio::test]
async fn rtsp_setup_establishes_a_session() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "rtsp",
        "You are an RTSP 1.0 media server serving one G.711 mu-law audio \
         stream over RTP/AVP/UDP. Accept every SETUP request from any client.",
    )
    .start()
    .await?;

    let request = format!(
        "SETUP rtsp://127.0.0.1:{}/stream/streamid=0 RTSP/1.0\r\n\
         CSeq: 3\r\n\
         Transport: RTP/AVP;unicast;client_port=51000-51001\r\n\r\n",
        server.port
    );
    let response = as_text(&server.tcp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("RTSP/1.0 200") {
            return Err(format!(
                "a well-formed SETUP must be accepted with RTSP/1.0 200; got {:?}",
                response
            )
            .into());
        }
        if !response.contains("CSeq: 3") {
            return Err(format!("response must echo 'CSeq: 3'; got {:?}", response).into());
        }
        // NetGet frames these deterministically; their absence means the
        // action never reached the framing path.
        if !response.contains("Session:") {
            return Err(format!(
                "SETUP must return a Session header or the client has nothing to PLAY \
                 against; got {:?}",
                response
            )
            .into());
        }
        if !response.contains("server_port=") {
            return Err(format!(
                "the Transport header must name the server RTP port that was allocated; \
                 got {:?}",
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

/// PLAY → the model describes what the RTP stream carries. It must be a
/// *description* (tone at a frequency), never samples, and PLAY only works
/// after a SETUP, so the sequence is the protocol's, not the test's.
#[tokio::test]
async fn rtsp_play_streams_the_described_media() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "rtsp",
        "You are an RTSP 1.0 media server. The only thing you stream is a \
         continuous 1000 Hz test tone in G.711 mu-law. Accept SETUP from any \
         client, and on PLAY stream that tone for two seconds.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let setup = session
        .exchange(
            format!(
                "SETUP rtsp://127.0.0.1:{}/stream/streamid=0 RTSP/1.0\r\n\
                 CSeq: 3\r\n\
                 Transport: RTP/AVP;unicast;client_port=51002-51003\r\n\r\n",
                server.port
            )
            .as_bytes(),
            "SETUP",
        )
        .await?;

    // The Session id NetGet allocated; PLAY must carry it back.
    let session_id = setup
        .lines()
        .find_map(|l| l.strip_prefix("Session:"))
        .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
        .unwrap_or_default();

    let play = session
        .exchange(
            format!(
                "PLAY rtsp://127.0.0.1:{}/stream RTSP/1.0\r\n\
                 CSeq: 4\r\n\
                 Session: {}\r\n\
                 Range: npt=0.000-\r\n\r\n",
                server.port, session_id
            )
            .as_bytes(),
            "PLAY",
        )
        .await?;

    let result = (|| -> E2EResult<()> {
        if session_id.is_empty() {
            return Err(format!("SETUP returned no Session id; got {:?}", setup).into());
        }
        if !play.starts_with("RTSP/1.0 200") {
            return Err(format!("PLAY must answer RTSP/1.0 200; got {:?}", play).into());
        }
        if !play.contains("CSeq: 4") {
            return Err(format!("PLAY response must echo 'CSeq: 4'; got {:?}", play).into());
        }
        // The server writes RTP-Info itself once a stream actually starts, so
        // its presence is evidence the model's action reached the RTP engine
        // rather than being a bare status code.
        if !play.contains("RTP-Info") {
            return Err(format!(
                "a PLAY that started a stream carries RTP-Info; without it nothing was \
                 synthesized. Got {:?}",
                play
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// TEARDOWN → 200, ending the session. Again after a SETUP, because there is
/// otherwise no session to tear down.
#[tokio::test]
async fn rtsp_teardown_ends_the_session() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "rtsp",
        "You are an RTSP 1.0 media server serving one audio stream. Accept \
         SETUP from any client, and let any client end its own session with \
         TEARDOWN.",
    )
    .start()
    .await?;

    let mut session = server.tcp_session().await?;
    let setup = session
        .exchange(
            format!(
                "SETUP rtsp://127.0.0.1:{}/stream/streamid=0 RTSP/1.0\r\n\
                 CSeq: 3\r\n\
                 Transport: RTP/AVP;unicast;client_port=51004-51005\r\n\r\n",
                server.port
            )
            .as_bytes(),
            "SETUP",
        )
        .await?;
    let session_id = setup
        .lines()
        .find_map(|l| l.strip_prefix("Session:"))
        .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
        .unwrap_or_default();

    let teardown = session
        .exchange(
            format!(
                "TEARDOWN rtsp://127.0.0.1:{}/stream RTSP/1.0\r\n\
                 CSeq: 5\r\n\
                 Session: {}\r\n\r\n",
                server.port, session_id
            )
            .as_bytes(),
            "TEARDOWN",
        )
        .await?;

    let result = (|| -> E2EResult<()> {
        if !teardown.starts_with("RTSP/1.0 200") {
            return Err(format!("TEARDOWN must answer RTSP/1.0 200; got {:?}", teardown).into());
        }
        if !teardown.contains("CSeq: 5") {
            return Err(
                format!("TEARDOWN response must echo 'CSeq: 5'; got {:?}", teardown).into(),
            );
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// A method this server does not implement raises `rtsp_other`. The answer
/// must be a status code the client can act on — 501 Not Implemented — and
/// not silence, which would leave the player waiting.
#[tokio::test]
async fn rtsp_unhandled_method_is_answered_with_501() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "rtsp",
        "You are a minimal RTSP 1.0 server. You implement only OPTIONS, \
         DESCRIBE, SETUP, PLAY and TEARDOWN. Any other method is not \
         implemented and must be refused as such.",
    )
    .start()
    .await?;

    let request = format!(
        "PAUSE rtsp://127.0.0.1:{}/stream RTSP/1.0\r\nCSeq: 6\r\n\r\n",
        server.port
    );
    let response = as_text(&server.tcp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("RTSP/1.0 501") {
            return Err(format!(
                "an unimplemented method is RTSP/1.0 501 Not Implemented — a player \
                 branches on the code, so 200 or 400 sends it down the wrong path. \
                 Got {:?}",
                response
            )
            .into());
        }
        if !response.contains("CSeq: 6") {
            return Err(format!("response must echo 'CSeq: 6'; got {:?}", response).into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
