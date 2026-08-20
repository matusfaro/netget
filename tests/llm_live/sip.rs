//! Live-LLM SIP suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. The OPTIONS response must echo the
//! request's Call-ID and CSeq — SIP's correlation fields — or a real UA
//! would discard it.
//!
//! Every method raises its own event, and every response must echo the
//! request's Call-ID and CSeq (RFC 3261 §8.2.6.2) — those two fields plus the
//! branch are how a UA matches a response to the transaction it sent. The
//! CSeq echo includes the *method*: `CSeq: 2 INVITE`, never a bare number.
//!
//! ACK is the exception: RFC 3261 §17 makes it the one request that is never
//! answered on the wire, so it is graded as an event case rather than a wire
//! exchange — there are no bytes to assert.
//!
//! COVERS: sip: sip_options, sip_register, sip_invite, sip_bye, sip_cancel

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::llm_live_case::EventCase;
use crate::helpers::E2EResult;
use serde_json::json;

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

/// Build a SIP request with the correlation fields a UA will check on the way
/// back: Call-ID identifies the dialog, CSeq the transaction within it.
fn sip_request(method: &str, port: u16, call_id: &str, cseq: u32, extra: &str) -> String {
    format!(
        "{m} sip:test@127.0.0.1:{p} SIP/2.0\r\n\
         Via: SIP/2.0/UDP 127.0.0.1:5099;branch=z9hG4bK-netget-{c}\r\n\
         From: <sip:tester@netget.example>;tag=7431\r\n\
         To: <sip:test@127.0.0.1>\r\n\
         Call-ID: {id}\r\n\
         CSeq: {c} {m}\r\n\
         Max-Forwards: 70\r\n\
         {extra}\
         Content-Length: 0\r\n\r\n",
        m = method,
        p = port,
        c = cseq,
        id = call_id,
        extra = extra
    )
}

/// A response must echo Call-ID and `CSeq: <n> <METHOD>`, or the UA discards
/// it as belonging to some other transaction.
fn echoes_correlation(response: &str, call_id: &str, cseq: u32, method: &str) -> E2EResult<()> {
    if !response.contains(call_id) {
        return Err(format!(
            "response must echo Call-ID {:?} — it identifies the dialog; got {:?}",
            call_id, response
        )
        .into());
    }
    let expected = format!("CSeq: {} {}", cseq, method);
    if !response.contains(&expected) {
        return Err(format!(
            "response must echo {:?}; the CSeq echo carries the method, not just the \
             number, and a UA matches the transaction on both. Got {:?}",
            expected, response
        )
        .into());
    }
    Ok(())
}

/// REGISTER → 200 accepting the binding, with the granted Expires.
#[tokio::test]
async fn sip_register_accepts_the_binding() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "sip",
        "You are a SIP registrar for the netget.example domain. Accept every \
         REGISTER request and grant a registration lifetime of 1800 seconds.",
    )
    .start()
    .await?;

    let call_id = "netget-live-reg-7431@127.0.0.1";
    let request = sip_request(
        "REGISTER",
        server.port,
        call_id,
        1,
        "Contact: <sip:tester@127.0.0.1:5099>;expires=1800\r\n",
    );
    let response = as_text(&server.udp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("SIP/2.0 200") {
            return Err(format!(
                "the registrar was told to accept every REGISTER, so this is 200; a 401 \
                 or 403 would send the UA into an authentication loop it cannot finish. \
                 Got {:?}",
                response
            )
            .into());
        }
        echoes_correlation(&response, call_id, 1, "REGISTER")?;
        if !response.contains("1800") {
            return Err(format!(
                "the granted registration lifetime (1800) must appear in the response, \
                 or the UA re-registers on its own guess; got {:?}",
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

/// INVITE accepted → 200 with the answering SDP. A 200 without a body leaves
/// the caller with nowhere to send media.
#[tokio::test]
async fn sip_invite_answers_with_sdp() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "sip",
        "You are a SIP voicemail endpoint that always answers calls. Accept \
         every INVITE with 200 OK and an SDP answer offering G.711 mu-law \
         (PCMU, payload type 0) audio.",
    )
    .start()
    .await?;

    let call_id = "netget-live-inv-7431@127.0.0.1";
    let offer = "v=0\r\no=tester 0 0 IN IP4 127.0.0.1\r\ns=Call\r\n\
                 c=IN IP4 127.0.0.1\r\nt=0 0\r\nm=audio 51100 RTP/AVP 0\r\n\
                 a=rtpmap:0 PCMU/8000\r\n";
    let request = format!(
        "INVITE sip:test@127.0.0.1:{p} SIP/2.0\r\n\
         Via: SIP/2.0/UDP 127.0.0.1:5099;branch=z9hG4bK-netget-2\r\n\
         From: <sip:tester@netget.example>;tag=7431\r\n\
         To: <sip:test@127.0.0.1>\r\n\
         Call-ID: {id}\r\n\
         CSeq: 2 INVITE\r\n\
         Contact: <sip:tester@127.0.0.1:5099>\r\n\
         Max-Forwards: 70\r\n\
         Content-Type: application/sdp\r\n\
         Content-Length: {len}\r\n\r\n{offer}",
        p = server.port,
        id = call_id,
        len = offer.len(),
        offer = offer
    );
    let response = as_text(&server.udp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("SIP/2.0 200") {
            return Err(format!(
                "the endpoint always answers, so the INVITE is accepted with 200; got {:?}",
                response
            )
            .into());
        }
        echoes_correlation(&response, call_id, 2, "INVITE")?;
        if !response.contains("v=0") || !response.contains("m=audio") {
            return Err(format!(
                "a 200 to an INVITE carries the answering SDP (v=0 + an m=audio line); \
                 without it the caller has nowhere to send media. Got {:?}",
                response
            )
            .into());
        }
        if !response.to_uppercase().contains("PCMU") && !response.contains("RTP/AVP 0") {
            return Err(format!(
                "the answer must offer the instructed PCMU/0 codec; got {:?}",
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

/// A declined call is a final failure status the caller can report to the
/// user — 486 Busy Here or 603 Decline — not silence and not a 200.
#[tokio::test]
async fn sip_invite_declined_is_a_failure_status() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "sip",
        "You are a SIP endpoint that is currently on another call and cannot \
         take a second one. Refuse every incoming INVITE as busy.",
    )
    .start()
    .await?;

    let call_id = "netget-live-busy-7431@127.0.0.1";
    let request = sip_request("INVITE", server.port, call_id, 3, "");
    let response = as_text(&server.udp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        let status = response.split_whitespace().nth(1).unwrap_or("").to_string();
        // 486 Busy Here is the exact answer; 603 Decline is the other final
        // refusal a UA renders the same way.
        if status != "486" && status != "603" {
            return Err(format!(
                "a busy endpoint answers 486 (Busy Here) or 603 (Decline); a 200 would \
                 connect the call it cannot take, and a 4xx like 400 tells the caller \
                 its request was malformed. Got status {:?} in {:?}",
                status, response
            )
            .into());
        }
        echoes_correlation(&response, call_id, 3, "INVITE")?;
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// BYE → 200 acknowledging the teardown.
#[tokio::test]
async fn sip_bye_acknowledges_the_teardown() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "sip",
        "You are a SIP endpoint. When the far end hangs up with BYE, \
         acknowledge the teardown so the call ends cleanly.",
    )
    .start()
    .await?;

    let call_id = "netget-live-bye-7431@127.0.0.1";
    let request = sip_request("BYE", server.port, call_id, 4, "");
    let response = as_text(&server.udp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("SIP/2.0 200") {
            return Err(format!(
                "BYE is acknowledged with 200; anything else leaves the far end \
                 retransmitting until it times out. Got {:?}",
                response
            )
            .into());
        }
        echoes_correlation(&response, call_id, 4, "BYE")
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// CANCEL → 200 confirming the pending INVITE was withdrawn.
#[tokio::test]
async fn sip_cancel_confirms_the_cancellation() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "sip",
        "You are a SIP endpoint. If a caller cancels before you have answered, \
         confirm the cancellation.",
    )
    .start()
    .await?;

    let call_id = "netget-live-cancel-7431@127.0.0.1";
    let request = sip_request("CANCEL", server.port, call_id, 5, "");
    let response = as_text(&server.udp_roundtrip(request.as_bytes()).await?);

    let result = (|| -> E2EResult<()> {
        if !response.starts_with("SIP/2.0 200") {
            return Err(format!("CANCEL is confirmed with 200; got {:?}", response).into());
        }
        echoes_correlation(&response, call_id, 5, "CANCEL")
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// ACK is the one SIP request that is never answered on the wire (RFC 3261
/// §17), so there are no bytes to grade — only the action, which records that
/// the dialog is now established. A model that tries to reply here would be
/// sending a response to a request that has none.
#[tokio::test]
async fn sip_ack_is_recorded_not_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "sip",
        "You are a SIP endpoint that answers calls. Track each dialog as it \
         becomes established.",
        "sip_ack",
        json!({
            "method": "ACK",
            "call_id": "netget-live-ack-7431@127.0.0.1",
            "cseq": "2 ACK",
            "from": "<sip:tester@netget.example>;tag=7431",
            "to": "<sip:test@127.0.0.1>;tag=netget",
            "peer_addr": "127.0.0.1:5099"
        }),
    )
    .expect_action("sip_ack")
    .run()
    .await
}
