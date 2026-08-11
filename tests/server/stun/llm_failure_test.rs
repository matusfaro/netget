//! What a STUN client gets when the LLM backend fails: a Binding Error Response.
//!
//! RFC 8489 §6.3.4 defines the error-response class precisely so a server can say "I could not
//! process this". A silent drop is indistinguishable from packet loss, so the client works
//! through its full retransmission schedule (§6.2.1: 7 retries over ~39.5s) before giving up.
//!
//! The response is decoded here from the raw bytes against the RFC's header and attribute
//! layout, not through the server's own builder.

#![cfg(feature = "stun")]

use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
use std::time::Duration;
use tokio::net::UdpSocket;

const MAGIC_COOKIE: u32 = 0x2112_A442;
const TRANSACTION_ID: [u8; 12] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
];

#[tokio::test]
async fn test_stun_answers_error_response_when_llm_fails() -> E2EResult<()> {
    let prompt = "listen on port {AVAILABLE_PORT} via stun. Reflect the client address";

    let server_config = NetGetConfig::new_no_scripts(prompt).with_mock(|mock| {
        mock.on_instruction_containing("via stun")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "STUN",
                    "instruction": "Reflect the client address"
                }
            ]))
            .expect_calls(1)
            .and()
        // No rule for `stun_binding_request`: the mock answers 500.
    });

    let server = start_netget_server(server_config).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Binding Request: type 0x0001, length 0, magic cookie, transaction ID.
    let mut request = Vec::with_capacity(20);
    request.extend_from_slice(&0x0001u16.to_be_bytes());
    request.extend_from_slice(&0u16.to_be_bytes());
    request.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    request.extend_from_slice(&TRANSACTION_ID);

    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    socket.connect(format!("127.0.0.1:{}", server.port)).await?;
    socket.send(&request).await?;

    let mut buf = vec![0u8; 2048];
    let n = tokio::time::timeout(Duration::from_secs(20), socket.recv(&mut buf))
        .await
        .map_err(|_| {
            "No STUN response within 20s - the server went silent on LLM failure, which is the \
             exact defect this test exists to catch"
        })??;

    assert!(n >= 20, "response shorter than a STUN header: {n} bytes");

    let message_type = u16::from_be_bytes([buf[0], buf[1]]);
    assert_eq!(
        message_type, 0x0111,
        "expected a Binding Error Response (0x0111), got 0x{message_type:04x}"
    );
    assert_eq!(
        u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
        MAGIC_COOKIE,
        "magic cookie must be present"
    );
    assert_eq!(
        &buf[8..20],
        &TRANSACTION_ID,
        "the transaction ID must be echoed, or the client discards the response"
    );

    // Walk the attributes looking for ERROR-CODE (0x0009). Its value is
    // reserved(2) | class(1) | number(1) | reason phrase, so the code is class*100 + number.
    let message_length = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    assert!(
        20 + message_length <= n,
        "declared attribute length {message_length} runs past the {n}-byte datagram"
    );
    let attributes = &buf[20..20 + message_length];

    let mut offset = 0usize;
    let mut error_code = None;
    while offset + 4 <= attributes.len() {
        let attr_type = u16::from_be_bytes([attributes[offset], attributes[offset + 1]]);
        let attr_len =
            u16::from_be_bytes([attributes[offset + 2], attributes[offset + 3]]) as usize;
        let value_start = offset + 4;
        let value_end = value_start + attr_len;
        assert!(
            value_end <= attributes.len(),
            "attribute runs past the value"
        );
        if attr_type == 0x0009 {
            assert!(attr_len >= 4, "ERROR-CODE value is at least 4 bytes");
            let class = attributes[value_start + 2] as u16;
            let number = attributes[value_start + 3] as u16;
            error_code = Some(class * 100 + number);
        }
        // Attributes are padded to a 4-byte boundary.
        offset = value_start + attr_len.div_ceil(4) * 4;
    }

    assert_eq!(
        error_code,
        Some(500),
        "LLM failure must be reported as STUN error 500 (Server Error)"
    );

    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}
