//! Live-LLM STUN suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. The binding response is validated at the
//! packet level: success type, magic cookie, and — the part that actually
//! exercises the model — the 12-byte transaction ID echoed from the request.
//!
//! COVERS: stun: stun_binding_request

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running STUN server.
#[tokio::test]
async fn stun_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("stun")
        .setup_prompt("Start a STUN server on port {AVAILABLE_PORT}.")
        .start()
        .await?;
    server.finish().await
}

/// Request type: RFC 5389 binding request → success response with the
/// transaction ID echoed.
#[tokio::test]
async fn stun_binding_request() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "stun",
        "You are a STUN server. Answer binding requests with a binding \
         success response reporting the client's address.",
    )
    .start()
    .await?;

    // Binding request: type 0x0001, length 0, magic cookie, fixed txn id.
    let txn_id: [u8; 12] = *b"netget-live!";
    let mut request = Vec::with_capacity(20);
    request.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    request.extend_from_slice(&0x2112A442u32.to_be_bytes());
    request.extend_from_slice(&txn_id);

    let response = server.udp_roundtrip(&request).await?;

    let result = (|| -> E2EResult<()> {
        if response.len() < 20 {
            return Err(format!(
                "STUN response must be at least 20 bytes, got {}: {:02x?}",
                response.len(),
                response
            )
            .into());
        }
        let msg_type = u16::from_be_bytes([response[0], response[1]]);
        if msg_type != 0x0101 {
            return Err(format!(
                "Expected binding success (0x0101), got message type 0x{:04x}",
                msg_type
            )
            .into());
        }
        let cookie = u32::from_be_bytes([response[4], response[5], response[6], response[7]]);
        if cookie != 0x2112A442 {
            return Err(format!("Invalid magic cookie 0x{:08x}", cookie).into());
        }
        if response[8..20] != txn_id {
            return Err(format!(
                "Transaction ID not echoed: sent {:02x?}, got {:02x?}",
                txn_id,
                &response[8..20]
            )
            .into());
        }
        println!("✅ stun: binding success with echoed transaction ID");
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
