//! Live-LLM NTP suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale. The response is validated at the packet
//! level: correct size, server mode bits, sane stratum.

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running NTP server.
#[tokio::test]
async fn ntp_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("ntp")
        .setup_prompt("Start an NTP server on port {AVAILABLE_PORT} serving the current time.")
        .start()
        .await?;
    server.finish().await
}

/// Request type: a real NTPv3 client packet → a wire-valid server response.
#[tokio::test]
async fn ntp_client_request() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "ntp",
        "You are an NTP time server at stratum 2. Answer every client request \
         with the current time.",
    )
    .start()
    .await?;

    // NTPv3 client packet: LI=0, VN=3, Mode=3 (client), rest zero.
    let mut request = [0u8; 48];
    request[0] = 0x1B;
    let response = server.udp_roundtrip(&request).await?;

    let result = (|| -> E2EResult<()> {
        if response.len() < 48 {
            return Err(format!(
                "NTP response must be at least 48 bytes, got {}: {:02x?}",
                response.len(),
                response
            )
            .into());
        }
        let mode = response[0] & 0x07;
        if mode != 4 {
            return Err(format!(
                "Expected mode 4 (server) in first byte, got mode {} (byte 0x{:02x})",
                mode, response[0]
            )
            .into());
        }
        let stratum = response[1];
        if stratum == 0 || stratum > 15 {
            return Err(format!("Implausible stratum {} in response", stratum).into());
        }
        println!(
            "✅ ntp: mode=4 stratum={} ({} bytes)",
            stratum,
            response.len()
        );
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
