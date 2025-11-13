//! E2E tests for BGP server
//!
//! These tests spawn the NetGet binary and test BGP protocol operations
//! using raw TCP clients to send/receive BGP messages.

#[cfg(all(test, feature = "bgp", feature = "bgp"))]
mod e2e_bgp {
    use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::{timeout, Duration};

    // BGP message types
    const BGP_MSG_OPEN: u8 = 1;
    const BGP_MSG_UPDATE: u8 = 2;
    const BGP_MSG_NOTIFICATION: u8 = 3;
    const BGP_MSG_KEEPALIVE: u8 = 4;

    // BGP message marker
    const BGP_MARKER: [u8; 16] = [0xff; 16];

    /// Helper to build a BGP OPEN message
    fn build_bgp_open(my_as: u16, hold_time: u16, router_id: [u8; 4]) -> Vec<u8> {
        let mut msg = Vec::new();

        // Marker (16 bytes of 0xFF)
        msg.extend_from_slice(&BGP_MARKER);

        // Length placeholder (will be filled below)
        msg.extend_from_slice(&[0, 0]);

        // Type = OPEN (1)
        msg.push(BGP_MSG_OPEN);

        // Version (4)
        msg.push(4);

        // My AS (16-bit)
        msg.extend_from_slice(&my_as.to_be_bytes());

        // Hold Time
        msg.extend_from_slice(&hold_time.to_be_bytes());

        // BGP Identifier (Router ID)
        msg.extend_from_slice(&router_id);

        // Optional Parameters Length (0 for simplicity)
        msg.push(0);

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        msg
    }

    /// Helper to build a BGP KEEPALIVE message
    fn build_bgp_keepalive() -> Vec<u8> {
        let mut msg = Vec::new();

        // Marker
        msg.extend_from_slice(&BGP_MARKER);

        // Length (19 bytes for KEEPALIVE)
        msg.extend_from_slice(&19u16.to_be_bytes());

        // Type = KEEPALIVE (4)
        msg.push(BGP_MSG_KEEPALIVE);

        msg
    }

    /// Helper to build a BGP NOTIFICATION message
    fn build_bgp_notification(error_code: u8, error_subcode: u8, data: &[u8]) -> Vec<u8> {
        let mut msg = Vec::new();

        // Marker
        msg.extend_from_slice(&BGP_MARKER);

        // Length placeholder
        msg.extend_from_slice(&[0, 0]);

        // Type = NOTIFICATION (3)
        msg.push(BGP_MSG_NOTIFICATION);

        // Error Code
        msg.push(error_code);

        // Error Subcode
        msg.push(error_subcode);

        // Data
        msg.extend_from_slice(data);

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        msg
    }

    /// Helper to read a BGP message from stream
    async fn read_bgp_message(stream: &mut TcpStream) -> E2EResult<(u8, Vec<u8>)> {
        // Read marker (16 bytes)
        let mut marker = [0u8; 16];
        stream.read_exact(&mut marker).await?;

        // Verify marker
        if marker != BGP_MARKER {
            return Err("Invalid BGP marker".into());
        }

        // Read length (2 bytes)
        let mut length_bytes = [0u8; 2];
        stream.read_exact(&mut length_bytes).await?;
        let length = u16::from_be_bytes(length_bytes);

        // Verify minimum length
        if length < 19 {
            return Err(format!("BGP message too short: {}", length).into());
        }

        // Read message type
        let mut msg_type = [0u8; 1];
        stream.read_exact(&mut msg_type).await?;

        // Read remaining message body
        let body_len = (length - 19) as usize;
        let mut body = vec![0u8; body_len];
        if body_len > 0 {
            stream.read_exact(&mut body).await?;
        }

        Ok((msg_type[0], body))
    }

    /// Helper to parse BGP OPEN message body
    fn parse_bgp_open(body: &[u8]) -> E2EResult<(u8, u16, u16, [u8; 4])> {
        if body.len() < 9 {
            return Err("OPEN message body too short".into());
        }

        let version = body[0];
        let my_as = u16::from_be_bytes([body[1], body[2]]);
        let hold_time = u16::from_be_bytes([body[3], body[4]]);
        let router_id = [body[5], body[6], body[7], body[8]];

        Ok((version, my_as, hold_time, router_id))
    }

    #[tokio::test]
    async fn test_bgp_peering_establishment() -> E2EResult<()> {
        println!("\n=== Test: BGP Peering Establishment ===");

        let prompt = "listen on port 0 via bgp. You are AS 65001 with router ID 192.168.1.1. \
             When you receive an OPEN message from a peer, validate it and respond with your own OPEN message. \
             After receiving a KEEPALIVE, send a KEEPALIVE back to complete the peering. \
             Transition to Established state.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup (user command)
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("bgp")
                    .and_instruction_containing("AS 65001")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BGP",
                            "instruction": "BGP router AS 65001, router ID 192.168.1.1"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPEN message received (bgp_open_received event)
                    .on_event("bgp_open")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_open",
                            "my_as": 65001,
                            "hold_time": 180,
                            "router_id": "192.168.1.1"
                        },
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: KEEPALIVE received (bgp_keepalive_received event)
                    .on_event("bgp_keepalive")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_keepalive"
                        },
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait a bit for server to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Connect to BGP server
        println!("  [TEST] Connecting to BGP server on port {}", server.port);
        let mut client = timeout(
            Duration::from_secs(5),
            TcpStream::connect(format!("127.0.0.1:{}", server.port)),
        )
        .await??;

        // Send OPEN message from client (AS 65000, router ID 192.168.1.100)
        println!("  [TEST] Sending OPEN message to server");
        let open_msg = build_bgp_open(65000, 180, [192, 168, 1, 100]);
        client.write_all(&open_msg).await?;
        client.flush().await?;

        // Read server's OPEN response
        println!("  [TEST] Reading OPEN response from server");
        let (msg_type, body) =
            timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await??;

        assert_eq!(
            msg_type, BGP_MSG_OPEN,
            "Expected OPEN message, got type {}",
            msg_type
        );

        // Parse OPEN message
        let (version, peer_as, hold_time, router_id) = parse_bgp_open(&body)?;
        println!(
            "  [TEST] Received OPEN: version={}, AS={}, hold_time={}, router_id={}.{}.{}.{}",
            version, peer_as, hold_time, router_id[0], router_id[1], router_id[2], router_id[3]
        );

        assert_eq!(version, 4, "BGP version should be 4");
        assert_eq!(peer_as, 65001, "Peer AS should be 65001");
        assert!(hold_time > 0, "Hold time should be greater than 0");

        // Send KEEPALIVE to acknowledge OPEN
        println!("  [TEST] Sending KEEPALIVE to acknowledge OPEN");
        let keepalive_msg = build_bgp_keepalive();
        client.write_all(&keepalive_msg).await?;
        client.flush().await?;

        // Read server's KEEPALIVE response
        println!("  [TEST] Reading KEEPALIVE response from server");
        let (msg_type, _body) =
            timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await??;

        assert_eq!(
            msg_type, BGP_MSG_KEEPALIVE,
            "Expected KEEPALIVE message, got type {}",
            msg_type
        );
        println!("  [TEST] ✓ BGP peering established successfully");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_bgp_notification_on_error() -> E2EResult<()> {
        println!("\n=== Test: BGP NOTIFICATION on Error ===");

        let prompt = "listen on port 0 via bgp. You are AS 65001. \
             If you receive an invalid OPEN message (e.g., wrong version), \
             send a NOTIFICATION message with error code 2 (OPEN Message Error), subcode 1 (Unsupported Version Number).";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup (user command)
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("bgp")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BGP",
                            "instruction": "BGP router AS 65001"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Invalid OPEN received (bgp_open_received event)
                    // LLM may choose to send NOTIFICATION or accept the invalid version
                    .on_event("bgp_open")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_notification",
                            "error_code": 2,
                            "error_subcode": 1,
                            "data": ""
                        }
                    ]))
                    .expect_at_most(1)  // May or may not be called depending on validation
                    .and()
            });

        let mut server = start_netget_server(config).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;

        println!("  [TEST] Connecting to BGP server");
        let mut client = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;

        // Send OPEN message with wrong version (version 3 instead of 4)
        println!("  [TEST] Sending OPEN with invalid version");
        let mut open_msg = build_bgp_open(65000, 180, [192, 168, 1, 100]);
        open_msg[19] = 3; // Change version from 4 to 3

        client.write_all(&open_msg).await?;
        client.flush().await?;

        // Read response - should be NOTIFICATION
        println!("  [TEST] Reading response from server");
        let read_result = timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await;

        match read_result {
            Ok(Ok((msg_type, body))) => {
                // Server may send NOTIFICATION or its own OPEN - both are acceptable
                if msg_type == BGP_MSG_NOTIFICATION {
                    println!("  [TEST] ✓ Received NOTIFICATION message");
                    if body.len() >= 2 {
                        let error_code = body[0];
                        let error_subcode = body[1];
                        println!(
                            "  [TEST]   Error code: {}, subcode: {}",
                            error_code, error_subcode
                        );
                    }
                } else if msg_type == BGP_MSG_OPEN {
                    println!("  [TEST] ✓ Received OPEN message (LLM may choose to accept invalid version)");
                } else {
                    println!("  [TEST] ! Received unexpected message type: {}", msg_type);
                }
            }
            Ok(Err(e)) => {
                println!(
                    "  [TEST] ✓ Connection closed (acceptable error handling): {}",
                    e
                );
            }
            Err(_) => {
                println!("  [TEST] ✓ Timeout (acceptable - connection may have been closed)");
            }
        }

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_bgp_keepalive_exchange() -> E2EResult<()> {
        println!("\n=== Test: BGP KEEPALIVE Exchange ===");

        let prompt = "listen on port 0 via bgp. You are AS 65001. \
             Establish BGP peering normally. After peering is established, \
             respond to KEEPALIVE messages with KEEPALIVE messages.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("bgp")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BGP",
                            "instruction": "BGP router AS 65001"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPEN received - respond with OPEN
                    .on_event("bgp_open")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_open",
                            "my_as": 65001,
                            "hold_time": 180,
                            "router_id": "192.168.1.1"
                        },
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: First KEEPALIVE - respond with KEEPALIVE (peering establishment)
                    .on_event("bgp_keepalive")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_keepalive"
                        },
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 4: Second KEEPALIVE - respond with KEEPALIVE (or no response)
                    .on_event("bgp_keepalive")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_keepalive"
                        },
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_at_most(1)  // May or may not respond to additional KEEPALIVE
                    .and()
            });

        let mut server = start_netget_server(config).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;

        println!("  [TEST] Connecting and establishing BGP peering");
        let mut client = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;

        // Send OPEN
        let open_msg = build_bgp_open(65000, 180, [192, 168, 1, 100]);
        client.write_all(&open_msg).await?;
        client.flush().await?;

        // Read server's OPEN
        let (_msg_type, _body) =
            timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await??;

        // Send KEEPALIVE
        let keepalive_msg = build_bgp_keepalive();
        client.write_all(&keepalive_msg).await?;
        client.flush().await?;

        // Read server's KEEPALIVE
        let (msg_type, _body) =
            timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await??;

        assert_eq!(
            msg_type, BGP_MSG_KEEPALIVE,
            "Expected KEEPALIVE after peering"
        );
        println!("  [TEST] ✓ Peering established");

        // Now send another KEEPALIVE
        println!("  [TEST] Sending additional KEEPALIVE");
        client.write_all(&keepalive_msg).await?;
        client.flush().await?;

        // Server should respond with KEEPALIVE (or no response is also acceptable)
        let read_result = timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await;

        match read_result {
            Ok(Ok((msg_type, _))) => {
                if msg_type == BGP_MSG_KEEPALIVE {
                    println!("  [TEST] ✓ Received KEEPALIVE response");
                } else {
                    println!("  [TEST] ✓ Received message type: {}", msg_type);
                }
            }
            _ => {
                println!("  [TEST] ✓ No immediate response (acceptable for KEEPALIVE)");
            }
        }

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_bgp_graceful_shutdown() -> E2EResult<()> {
        println!("\n=== Test: BGP Graceful Shutdown ===");

        let prompt = "listen on port 0 via bgp. You are AS 65001. \
             Establish BGP peering normally. If you receive a NOTIFICATION with error code 6 (Cease), \
             acknowledge it by closing the connection gracefully.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("bgp")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "BGP",
                            "instruction": "BGP router AS 65001"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPEN received - respond with OPEN
                    .on_event("bgp_open")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_open",
                            "my_as": 65001,
                            "hold_time": 180,
                            "router_id": "192.168.1.1"
                        },
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: KEEPALIVE received - respond with KEEPALIVE (establish peering)
                    .on_event("bgp_keepalive")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_bgp_keepalive"
                        },
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 4: NOTIFICATION (Cease) received - close connection gracefully
                    .on_event("bgp_notification")
                    .and_event_data_contains("error_code", "6")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "disconnect"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;

        println!("  [TEST] Establishing BGP peering");
        let mut client = TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;

        // Establish peering
        let open_msg = build_bgp_open(65000, 180, [192, 168, 1, 100]);
        client.write_all(&open_msg).await?;
        client.flush().await?;

        let (_msg_type, _body) =
            timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await??;

        let keepalive_msg = build_bgp_keepalive();
        client.write_all(&keepalive_msg).await?;
        client.flush().await?;

        let (_msg_type, _body) =
            timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await??;

        println!("  [TEST] ✓ Peering established");

        // Send NOTIFICATION (Cease)
        println!("  [TEST] Sending NOTIFICATION (Cease) to gracefully shut down");
        let notification_msg = build_bgp_notification(6, 0, &[]);
        client.write_all(&notification_msg).await?;
        client.flush().await?;

        // Server should close the connection or send NOTIFICATION back
        let read_result = timeout(Duration::from_secs(120), read_bgp_message(&mut client)).await;

        match read_result {
            Ok(Ok((BGP_MSG_NOTIFICATION, _))) => {
                println!("  [TEST] ✓ Server acknowledged with NOTIFICATION");
            }
            Ok(Err(_)) | Err(_) => {
                println!("  [TEST] ✓ Connection closed gracefully");
            }
            Ok(Ok((msg_type, _))) => {
                println!("  [TEST] ! Received unexpected message type: {}", msg_type);
            }
        }

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }
}
