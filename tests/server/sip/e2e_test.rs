//! E2E tests for SIP protocol
//!
//! These tests verify SIP server functionality by starting NetGet with SIP prompts
//! and using raw UDP sockets to send SIP messages.

#![cfg(feature = "sip")]

use crate::server::helpers::*;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

#[tokio::test]
async fn test_sip_comprehensive() -> E2EResult<()> {
    // Single comprehensive server with scripting for all test cases
    let config = ServerConfig::new(
        r#"listen on port 0 via sip

You are a SIP (Session Initiation Protocol) server implementing RFC 3261.

REGISTRATION (REGISTER method):
- User 'alice@localhost' with any contact: Accept (200 OK), set expires to 3600
- User 'bob@localhost' with any contact: Accept (200 OK), set expires to 1800
- All other users: Reject (403 Forbidden)

INCOMING CALLS (INVITE method):
- From alice to bob: Accept (200 OK) with SDP:
  v=0
  o=- 12345 12345 IN IP4 127.0.0.1
  s=Test Call
  c=IN IP4 127.0.0.1
  t=0 0
  m=audio 8000 RTP/AVP 0
  a=rtpmap:0 PCMU/8000

- From bob to alice: Reject (486 Busy Here)
- From unknown users: Reject (403 Forbidden)

CALL TERMINATION (BYE method):
- Always accept (200 OK)

CAPABILITY QUERY (OPTIONS method):
- Return 200 OK with Allow header: INVITE, ACK, BYE, REGISTER, OPTIONS

ACKNOWLEDGMENT (ACK method):
- No response needed (ACK is not a request that requires response)

Use scripting mode to handle all requests without LLM calls after initial setup.
"#,
    )
    .with_log_level("off");

    let test_state = start_netget_server(config).await?;

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    let server_addr: SocketAddr = format!("127.0.0.1:{}", test_state.port)
        .parse()
        .expect("Failed to parse server address");

    // Create UDP client socket
    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("Failed to set read timeout");

    println!("✓ SIP server started on {}", server_addr);

    // Test 1: REGISTER alice (should succeed)
    println!("\n[Test 1] REGISTER alice@localhost");
    let register_alice = build_sip_register("alice@localhost", "alice", &server_addr);
    client
        .send_to(register_alice.as_bytes(), server_addr)
        .expect("Failed to send REGISTER");

    let mut buf = vec![0u8; 65535];
    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive REGISTER response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(response.contains("SIP/2.0 200"), "Expected 200 OK for alice");
    assert!(response.contains("Expires:"), "Expected Expires header");
    println!("✓ alice registered successfully");

    // Test 2: REGISTER bob (should succeed)
    println!("\n[Test 2] REGISTER bob@localhost");
    let register_bob = build_sip_register("bob@localhost", "bob", &server_addr);
    client
        .send_to(register_bob.as_bytes(), server_addr)
        .expect("Failed to send REGISTER");

    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive REGISTER response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(response.contains("SIP/2.0 200"), "Expected 200 OK for bob");
    println!("✓ bob registered successfully");

    // Test 3: REGISTER unknown user (should be rejected)
    println!("\n[Test 3] REGISTER charlie@localhost (should be rejected)");
    let register_charlie = build_sip_register("charlie@localhost", "charlie", &server_addr);
    client
        .send_to(register_charlie.as_bytes(), server_addr)
        .expect("Failed to send REGISTER");

    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive REGISTER response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(
        response.contains("SIP/2.0 403"),
        "Expected 403 Forbidden for charlie"
    );
    println!("✓ charlie registration rejected as expected");

    // Test 4: OPTIONS query
    println!("\n[Test 4] OPTIONS query");
    let options = build_sip_options(&server_addr);
    client
        .send_to(options.as_bytes(), server_addr)
        .expect("Failed to send OPTIONS");

    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive OPTIONS response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(response.contains("SIP/2.0 200"), "Expected 200 OK for OPTIONS");
    assert!(
        response.contains("Allow:"),
        "Expected Allow header in OPTIONS response"
    );
    println!("✓ OPTIONS query successful");

    // Test 5: INVITE alice→bob (should accept)
    println!("\n[Test 5] INVITE from alice to bob (should accept)");
    let invite_alice_to_bob = build_sip_invite("alice", "bob", &server_addr, "call-123");
    client
        .send_to(invite_alice_to_bob.as_bytes(), server_addr)
        .expect("Failed to send INVITE");

    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive INVITE response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(
        response.contains("SIP/2.0 200"),
        "Expected 200 OK for alice→bob"
    );
    assert!(
        response.contains("Content-Type: application/sdp"),
        "Expected SDP in response"
    );
    assert!(
        response.contains("v=0"),
        "Expected SDP body with v=0 in response"
    );
    println!("✓ alice→bob INVITE accepted with SDP");

    // Test 6: BYE to terminate call
    println!("\n[Test 6] BYE to terminate call");
    let bye = build_sip_bye("alice", "bob", &server_addr, "call-123");
    client
        .send_to(bye.as_bytes(), server_addr)
        .expect("Failed to send BYE");

    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive BYE response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(response.contains("SIP/2.0 200"), "Expected 200 OK for BYE");
    println!("✓ BYE call termination successful");

    // Test 7: INVITE bob→alice (should reject with Busy)
    println!("\n[Test 7] INVITE from bob to alice (should reject)");
    let invite_bob_to_alice = build_sip_invite("bob", "alice", &server_addr, "call-456");
    client
        .send_to(invite_bob_to_alice.as_bytes(), server_addr)
        .expect("Failed to send INVITE");

    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive INVITE response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(
        response.contains("SIP/2.0 486") || response.contains("Busy"),
        "Expected 486 Busy Here for bob→alice"
    );
    println!("✓ bob→alice INVITE rejected as expected");

    // Test 8: INVITE from unknown user (should reject)
    println!("\n[Test 8] INVITE from charlie to bob (should reject)");
    let invite_charlie_to_bob = build_sip_invite("charlie", "bob", &server_addr, "call-789");
    client
        .send_to(invite_charlie_to_bob.as_bytes(), server_addr)
        .expect("Failed to send INVITE");

    let (len, _) = client
        .recv_from(&mut buf)
        .expect("Failed to receive INVITE response");
    let response = String::from_utf8_lossy(&buf[..len]);
    println!("Response:\n{}", response);
    assert!(
        response.contains("SIP/2.0 403"),
        "Expected 403 Forbidden for charlie→bob"
    );
    println!("✓ charlie→bob INVITE rejected as expected");

    println!("\n✓ All SIP tests passed!");

    // Cleanup
    test_state.stop().await?;
    Ok(())
}

/// Build SIP REGISTER request
fn build_sip_register(user: &str, from_tag: &str, server_addr: &SocketAddr) -> String {
    let call_id = format!("reg-{}@127.0.0.1", user);
    let branch = format!("z9hG4bK{}", user);

    format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/UDP 127.0.0.1:5060;branch={}\r\n\
         From: <sip:{}@localhost>;tag={}\r\n\
         To: <sip:{}@localhost>\r\n\
         Call-ID: {}\r\n\
         CSeq: 1 REGISTER\r\n\
         Contact: <sip:{}@127.0.0.1:5060>\r\n\
         Expires: 3600\r\n\
         Content-Length: 0\r\n\
         \r\n",
        server_addr.ip(),
        branch,
        user,
        from_tag,
        user,
        call_id,
        user
    )
}

/// Build SIP OPTIONS request
fn build_sip_options(server_addr: &SocketAddr) -> String {
    let call_id = "options-test@127.0.0.1";
    let branch = "z9hG4bKoptions";

    format!(
        "OPTIONS sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/UDP 127.0.0.1:5060;branch={}\r\n\
         From: <sip:test@localhost>;tag=12345\r\n\
         To: <sip:{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: 1 OPTIONS\r\n\
         Content-Length: 0\r\n\
         \r\n",
        server_addr.ip(),
        branch,
        server_addr,
        call_id
    )
}

/// Build SIP INVITE request
fn build_sip_invite(from: &str, to: &str, server_addr: &SocketAddr, call_id: &str) -> String {
    let branch = format!("z9hG4bK{}", call_id);
    let sdp = format!(
        "v=0\r\n\
         o=- 53655765 2353687637 IN IP4 127.0.0.1\r\n\
         s=Call\r\n\
         c=IN IP4 127.0.0.1\r\n\
         t=0 0\r\n\
         m=audio 49170 RTP/AVP 0\r\n\
         a=rtpmap:0 PCMU/8000\r\n"
    );

    format!(
        "INVITE sip:{}@{} SIP/2.0\r\n\
         Via: SIP/2.0/UDP 127.0.0.1:5060;branch={}\r\n\
         From: <sip:{}@localhost>;tag={}-tag\r\n\
         To: <sip:{}@localhost>\r\n\
         Call-ID: {}\r\n\
         CSeq: 1 INVITE\r\n\
         Contact: <sip:{}@127.0.0.1:5060>\r\n\
         Content-Type: application/sdp\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        to,
        server_addr.ip(),
        branch,
        from,
        from,
        to,
        call_id,
        from,
        sdp.len(),
        sdp
    )
}

/// Build SIP BYE request
fn build_sip_bye(from: &str, to: &str, server_addr: &SocketAddr, call_id: &str) -> String {
    let branch = format!("z9hG4bKbye-{}", call_id);

    format!(
        "BYE sip:{}@{} SIP/2.0\r\n\
         Via: SIP/2.0/UDP 127.0.0.1:5060;branch={}\r\n\
         From: <sip:{}@localhost>;tag={}-tag\r\n\
         To: <sip:{}@localhost>;tag={}-tag\r\n\
         Call-ID: {}\r\n\
         CSeq: 2 BYE\r\n\
         Content-Length: 0\r\n\
         \r\n",
        to, server_addr.ip(), branch, from, from, to, to, call_id
    )
}
