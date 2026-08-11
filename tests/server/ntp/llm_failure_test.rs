//! What an NTP client gets when the LLM backend fails: a Kiss-o'-Death packet.
//!
//! NTP has no error message, but RFC 5905 §7.4 defines a packet whose whole purpose is for a
//! server to say "do not use me": stratum 0, leap indicator 3, and a four-character kiss code
//! in the reference identifier. `chrony`, `ntpd` and `ntpdate` recognise it, refuse to take
//! time from it, and stop polling.
//!
//! That matters twice over. Silence looks to a client like a merely slow server, so it keeps
//! retrying; and a KoD can never be mistaken for a time sample, so an outage cannot silently
//! hand anyone a fabricated clock reading.
//!
//! The packet is decoded here byte by byte against the RFC's field layout rather than through
//! the server's own builder, so the test is evidence and not a tautology.

#![cfg(feature = "ntp")]

use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
use std::time::Duration;
use tokio::net::UdpSocket;

/// The client's transmit timestamp, which must come back as the reply's origin timestamp.
const CLIENT_TRANSMIT: u64 = 0xE5F1_2345_89AB_CDEF;

#[tokio::test]
async fn test_ntp_answers_kiss_of_death_when_llm_fails() -> E2EResult<()> {
    let prompt = "listen on port {AVAILABLE_PORT} via ntp. Answer with the current time";

    let server_config = NetGetConfig::new_no_scripts(prompt).with_mock(|mock| {
        mock.on_instruction_containing("via ntp")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "NTP",
                    "instruction": "Answer with the current time"
                }
            ]))
            .expect_calls(1)
            .and()
        // No rule for `ntp_request`: the mock answers 500.
    });

    let server = start_netget_server(server_config).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // A minimal NTPv4 client request: LI=0, VN=4, Mode=3.
    let mut request = vec![0u8; 48];
    request[0] = (4 << 3) | 3;
    request[40..48].copy_from_slice(&CLIENT_TRANSMIT.to_be_bytes());

    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    socket.connect(format!("127.0.0.1:{}", server.port)).await?;
    socket.send(&request).await?;

    let mut buf = vec![0u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(20), socket.recv(&mut buf))
        .await
        .map_err(|_| {
            "No NTP response within 20s - the server went silent on LLM failure, which is the \
             exact defect this test exists to catch"
        })??;

    assert_eq!(n, 48, "an NTP packet is 48 bytes");

    let leap_indicator = (buf[0] >> 6) & 0x03;
    let version = (buf[0] >> 3) & 0x07;
    let mode = buf[0] & 0x07;
    assert_eq!(mode, 4, "mode must be 4 (server)");
    assert_eq!(version, 4, "the reply must use the client's NTP version");
    assert_eq!(
        leap_indicator, 3,
        "LI must be 3 (unsynchronized) so no client treats this as usable time"
    );
    assert_eq!(
        buf[1], 0,
        "stratum 0 is what marks a packet as a Kiss-o'-Death (RFC 5905 §7.4)"
    );

    // The kiss code lives in the reference identifier, bytes 12-15.
    let kiss_code = String::from_utf8_lossy(&buf[12..16]).to_string();
    assert_eq!(
        kiss_code, "INIT",
        "a non-overload failure should report the INIT kiss code, got {kiss_code:?}"
    );

    // Origin timestamp (bytes 24-31) must be the client's transmit timestamp verbatim, or the
    // client discards the reply as unrelated to its request - which is silence again.
    let origin = u64::from_be_bytes(buf[24..32].try_into().expect("8 bytes"));
    assert_eq!(
        origin, CLIENT_TRANSMIT,
        "the client's transmit timestamp must be echoed as the origin timestamp"
    );

    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}
