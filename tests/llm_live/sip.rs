//! Live-LLM SIP suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. The OPTIONS response must echo the
//! request's Call-ID and CSeq — SIP's correlation fields — or a real UA
//! would discard it.

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running SIP server.
#[tokio::test]
async fn sip_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("sip")
        .setup_prompt("Start a SIP server on port {AVAILABLE_PORT}.")
        .start()
        .await?;
    server.finish().await
}

/// Request type: OPTIONS over UDP → 200 OK echoing Call-ID and CSeq.
#[tokio::test]
async fn sip_options_request() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "sip",
        "You are a SIP server. Answer OPTIONS requests with a 200 OK response.",
    )
    .start()
    .await?;

    let call_id = "netget-live-call-7431@127.0.0.1";
    let request = format!(
        "OPTIONS sip:test@127.0.0.1 SIP/2.0\r\n\
         Via: SIP/2.0/UDP 127.0.0.1:5099;branch=z9hG4bK-netget-7431\r\n\
         From: <sip:tester@netget.example>;tag=7431\r\n\
         To: <sip:test@127.0.0.1>\r\n\
         Call-ID: {}\r\n\
         CSeq: 1 OPTIONS\r\n\
         Max-Forwards: 70\r\n\
         Content-Length: 0\r\n\r\n",
        call_id
    );

    let response = server.udp_roundtrip(request.as_bytes()).await?;
    let text = as_text(&response);
    let result = expect_contains(&text, "SIP/2.0 200")
        .and(expect_contains(&text, call_id))
        .and(expect_contains(&text, "CSeq: 1 OPTIONS"))
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
