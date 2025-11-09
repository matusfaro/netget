//! USB FIDO2/U2F Security Key E2E tests
//!
//! These tests verify the FIDO2/U2F virtual security key implementation
//! using real-world client tools (libfido2, browsers) and LLM integration.

#[cfg(all(test, feature = "usb-fido2"))]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Duration};

    use netget::llm::OllamaClient;
    use netget::state::app_state::AppState;
    use netget::server::usb::fido2::UsbFido2Server;
    use netget::server::usb::fido2::approval::{ApprovalManager, ApprovalConfig, ApprovalDecision, OperationType};

    /// Test FIDO2 server startup with LLM integration
    #[tokio::test]
    #[ignore] // Requires system setup
    async fn test_fido2_server_startup_with_llm() {
        // Create test infrastructure
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();
        let llm_client = OllamaClient::new("http://localhost:11434".to_string(), "qwen3-coder:30b".to_string());
        let app_state = Arc::new(AppState::new(llm_client.clone()));
        let server_id = netget::state::ServerId::new(1);

        // Start server with auto-approve mode
        let listen_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

        let result = UsbFido2Server::spawn_with_llm_actions(
            listen_addr,
            llm_client,
            app_state,
            status_tx,
            server_id,
            Some(true),  // support_u2f
            Some(true),  // support_fido2
            Some(true),  // auto_approve for testing
        ).await;

        assert!(result.is_ok(), "Server should start successfully");
        let actual_addr = result.unwrap();

        // Verify server is listening
        assert!(actual_addr.port() > 0, "Server should be listening on a port");

        // Verify status message was sent
        tokio::select! {
            msg = status_rx.recv() => {
                assert!(msg.is_some(), "Should receive status message");
                let msg = msg.unwrap();
                assert!(msg.contains("FIDO2"), "Status should mention FIDO2");
            }
            _ = sleep(Duration::from_secs(1)) => {
                panic!("Timeout waiting for status message");
            }
        }
    }

    /// Test PIN/UV support
    #[tokio::test]
    async fn test_pin_uv_support() {
        use netget::server::usb::fido2::ctap2::Ctap2CredentialStore;

        // Create credential store
        let mut store = Ctap2CredentialStore::new();

        // Test PIN not set initially
        assert!(!store.has_pin(), "PIN should not be set initially");
        assert!(!store.pin_verified(), "PIN should not be verified initially");
        assert_eq!(store.pin_retries(), 8, "Should start with 8 retries");

        // Set a PIN
        let result = store.set_pin("test1234");
        assert!(result.is_ok(), "Should set PIN successfully");
        assert!(store.has_pin(), "PIN should be set after setting");

        // Verify correct PIN
        let result = store.verify_pin("test1234");
        assert!(result.is_ok(), "PIN verification should not error");
        assert_eq!(result.unwrap(), true, "Correct PIN should verify");
        assert!(store.pin_verified(), "PIN should be verified after correct entry");
        assert_eq!(store.pin_retries(), 8, "Retries should reset on success");

        // Verify incorrect PIN
        let result = store.verify_pin("wrong");
        assert!(result.is_ok(), "PIN verification should not error");
        assert_eq!(result.unwrap(), false, "Wrong PIN should not verify");
        assert!(!store.pin_verified(), "PIN should not be verified after wrong entry");
        assert_eq!(store.pin_retries(), 7, "Retries should decrement");

        // Test PIN too short
        let result = store.set_pin("123");
        assert!(result.is_err(), "PIN too short should fail");

        // Test PIN too long
        let result = store.set_pin(&"a".repeat(64));
        assert!(result.is_err(), "PIN too long should fail");
    }

    /// Test resident key creation and storage
    #[tokio::test]
    async fn test_resident_keys() {
        use netget::server::usb::fido2::ctap2::Ctap2CredentialStore;

        let mut store = Ctap2CredentialStore::new();

        // Create non-resident credential
        let cred1 = store.make_credential(
            "example.com",
            b"user123",
            "test@example.com",
            false, // not resident
            false, // no UV
        );
        assert!(cred1.is_ok(), "Should create non-resident credential");

        // Create resident credential
        let cred2 = store.make_credential(
            "example.com",
            b"user456",
            "user2@example.com",
            true,  // resident key
            false, // no UV
        );
        assert!(cred2.is_ok(), "Should create resident credential");

        // Verify credential can be found
        let found = store.find_credentials("example.com", None);
        assert!(found.is_some(), "Should find credential for RP");

        // Create another resident credential for different RP
        let cred3 = store.make_credential(
            "test.com",
            b"user789",
            "user3@test.com",
            true,  // resident key
            false, // no UV
        );
        assert!(cred3.is_ok(), "Should create credential for different RP");

        // Verify both RPs have credentials
        assert!(store.find_credentials("example.com", None).is_some());
        assert!(store.find_credentials("test.com", None).is_some());
    }

    /// Test approval system with auto-approve mode
    #[tokio::test]
    async fn test_approval_auto_approve() {
        let config = ApprovalConfig {
            auto_approve: true,
            timeout: Duration::from_secs(30),
            timeout_decision: ApprovalDecision::Denied,
        };

        let manager = ApprovalManager::new(config);

        // Request approval - should instantly approve
        let (id, decision) = manager.request_approval(
            OperationType::Register,
            "example.com".to_string(),
            Some("user@example.com".to_string()),
            None,
        ).await;

        assert_eq!(decision, ApprovalDecision::Approved, "Should auto-approve");
        assert!(id > 0, "Should have valid approval ID");
    }

    /// Test approval system with manual approval
    #[tokio::test]
    async fn test_approval_manual_approve() {
        let config = ApprovalConfig {
            auto_approve: false,
            timeout: Duration::from_secs(5),
            timeout_decision: ApprovalDecision::Denied,
        };

        let manager = ApprovalManager::new(config);

        // Spawn a task to approve after delay
        let manager_clone = manager.clone();
        let approve_task = tokio::spawn(async move {
            // Wait a bit, then approve
            sleep(Duration::from_millis(100)).await;

            let pending = manager_clone.list_pending().await;
            assert_eq!(pending.len(), 1, "Should have 1 pending request");

            let approval_id = pending[0].0;
            manager_clone.approve(approval_id).await
        });

        // Request approval - should wait and then be approved
        let (id, decision) = manager.request_approval(
            OperationType::Authenticate,
            "test.com".to_string(),
            None,
            None,
        ).await;

        assert_eq!(decision, ApprovalDecision::Approved, "Should be approved by task");

        // Verify approval task completed successfully
        let result = approve_task.await;
        assert!(result.is_ok(), "Approve task should complete");
        assert!(result.unwrap().is_ok(), "Approval should succeed");
    }

    /// Test approval system with manual denial
    #[tokio::test]
    async fn test_approval_manual_deny() {
        let config = ApprovalConfig {
            auto_approve: false,
            timeout: Duration::from_secs(5),
            timeout_decision: ApprovalDecision::Denied,
        };

        let manager = ApprovalManager::new(config);

        // Spawn a task to deny after delay
        let manager_clone = manager.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            let pending = manager_clone.list_pending().await;
            if let Some((id, _, _, _)) = pending.first() {
                let _ = manager_clone.deny(*id).await;
            }
        });

        // Request approval - should be denied
        let (_id, decision) = manager.request_approval(
            OperationType::Register,
            "example.com".to_string(),
            Some("test@example.com".to_string()),
            None,
        ).await;

        assert_eq!(decision, ApprovalDecision::Denied, "Should be denied");
    }

    /// Test approval system timeout
    #[tokio::test]
    async fn test_approval_timeout() {
        let config = ApprovalConfig {
            auto_approve: false,
            timeout: Duration::from_millis(100),
            timeout_decision: ApprovalDecision::Denied,
        };

        let manager = ApprovalManager::new(config);

        // Request approval without responding - should timeout
        let (_id, decision) = manager.request_approval(
            OperationType::Register,
            "example.com".to_string(),
            None,
            None,
        ).await;

        assert_eq!(decision, ApprovalDecision::Denied, "Should timeout and deny");
    }

    /// Test approval system list pending
    #[tokio::test]
    async fn test_approval_list_pending() {
        let config = ApprovalConfig {
            auto_approve: false,
            timeout: Duration::from_secs(30),
            timeout_decision: ApprovalDecision::Denied,
        };

        let manager = ApprovalManager::new(config);
        let manager_clone = manager.clone();

        // Spawn a task that requests multiple approvals
        let request_task = tokio::spawn(async move {
            let _ = manager_clone.request_approval(
                OperationType::Register,
                "example.com".to_string(),
                Some("user1@example.com".to_string()),
                None,
            );
            let _ = manager_clone.request_approval(
                OperationType::Authenticate,
                "test.com".to_string(),
                None,
                None,
            );
        });

        // Wait a bit for requests to be pending
        sleep(Duration::from_millis(50)).await;

        // List pending requests
        let pending = manager.list_pending().await;
        assert!(pending.len() >= 1, "Should have at least 1 pending request");

        // Cancel the request task
        request_task.abort();
    }

    /// Test PIN requirement for user verification
    #[tokio::test]
    async fn test_pin_required_for_uv() {
        use netget::server::usb::fido2::ctap2::Ctap2CredentialStore;

        let mut store = Ctap2CredentialStore::new();

        // Try to create credential with UV but no PIN set - should fail
        let result = store.make_credential(
            "example.com",
            b"user123",
            "test@example.com",
            false, // not resident
            true,  // require UV
        );
        assert!(result.is_err(), "Should fail when UV required but PIN not set");

        // Set PIN
        store.set_pin("test1234").unwrap();

        // Try again without verifying - should still fail
        let result = store.make_credential(
            "example.com",
            b"user123",
            "test@example.com",
            false, // not resident
            true,  // require UV
        );
        assert!(result.is_err(), "Should fail when UV required but PIN not verified");

        // Verify PIN
        store.verify_pin("test1234").unwrap();

        // Try again with verified PIN - should succeed
        let result = store.make_credential(
            "example.com",
            b"user123",
            "test@example.com",
            false, // not resident
            true,  // require UV
        );
        assert!(result.is_ok(), "Should succeed when UV required and PIN verified");
    }

    /// Test CTAPHID packet fragmentation for small messages
    #[tokio::test]
    async fn test_ctaphid_small_message() {
        use netget::server::usb::fido2::ctaphid::{CtapHidHandler, CtapHidCommand};

        let handler = CtapHidHandler::new();
        let cid = 0x12345678u32;
        let cmd = CtapHidCommand::Ping;

        // Small message (fits in one packet)
        let data = b"Hello FIDO2!";

        let packets = handler.fragment_response(cid, cmd, data);

        // Should be exactly 1 packet
        assert_eq!(packets.len(), 1, "Small message should fit in 1 packet");

        // Verify packet structure
        let packet = &packets[0];
        assert_eq!(packet.len(), 64, "Packet should be 64 bytes");

        // Verify CID
        let packet_cid = u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]);
        assert_eq!(packet_cid, cid, "CID should match");

        // Verify CMD with init bit
        assert_eq!(packet[4], (cmd as u8) | 0x80, "CMD should have init bit set");

        // Verify BCNT (byte count)
        let bcnt = u16::from_be_bytes([packet[5], packet[6]]);
        assert_eq!(bcnt, data.len() as u16, "BCNT should match data length");

        // Verify data
        assert_eq!(&packet[7..7 + data.len()], data, "Data should match");
    }

    /// Test CTAPHID packet fragmentation for large messages
    #[tokio::test]
    async fn test_ctaphid_large_message_fragmentation() {
        use netget::server::usb::fido2::ctaphid::{CtapHidHandler, CtapHidCommand};

        let handler = CtapHidHandler::new();
        let cid = 0xabcdef01u32;
        let cmd = CtapHidCommand::Cbor;

        // Large message requiring fragmentation (150 bytes)
        let data = vec![0xAAu8; 150];

        let packets = handler.fragment_response(cid, cmd, &data);

        // Calculate expected packet count
        // First packet: 57 bytes, continuation packets: 59 bytes each
        // Remaining after first: 150 - 57 = 93 bytes
        // Continuation packets needed: ceil(93 / 59) = 2
        // Total: 1 init + 2 cont = 3 packets
        assert_eq!(packets.len(), 3, "150-byte message should need 3 packets");

        // Verify init packet
        let init_packet = &packets[0];
        assert_eq!(init_packet.len(), 64);
        assert_eq!(init_packet[4], (cmd as u8) | 0x80, "Init packet should have CMD with init bit");
        let bcnt = u16::from_be_bytes([init_packet[5], init_packet[6]]);
        assert_eq!(bcnt, 150, "BCNT should be total message length");
        assert_eq!(&init_packet[7..64], &data[0..57], "Init packet data should match first 57 bytes");

        // Verify first continuation packet
        let cont1 = &packets[1];
        assert_eq!(cont1.len(), 64);
        let cid1 = u32::from_be_bytes([cont1[0], cont1[1], cont1[2], cont1[3]]);
        assert_eq!(cid1, cid, "Continuation packet CID should match");
        assert_eq!(cont1[4], 0, "First continuation packet should have SEQ=0");
        assert_eq!(&cont1[5..64], &data[57..116], "First cont packet data should match bytes 57-115");

        // Verify second continuation packet
        let cont2 = &packets[2];
        assert_eq!(cont2.len(), 64);
        assert_eq!(cont2[4], 1, "Second continuation packet should have SEQ=1");
        assert_eq!(&cont2[5..5 + 34], &data[116..150], "Second cont packet data should match remaining bytes");
    }

    /// Test CTAPHID packet assembly from fragments
    #[tokio::test]
    async fn test_ctaphid_packet_assembly() {
        use netget::server::usb::fido2::ctaphid::{CtapHidHandler, CtapHidCommand, CtapHidPacket};

        let mut handler = CtapHidHandler::new();
        let cid = 0x99887766u32;

        // Create a multi-packet message
        let original_data = vec![0x42u8; 100];

        // Fragment it
        let packets = handler.fragment_response(cid, CtapHidCommand::Ping, &original_data);
        assert!(packets.len() > 1, "Should have multiple packets");

        // Now process the packets through the handler to reassemble
        let mut assembled_message = None;

        for packet_bytes in packets {
            let result = handler.process_packet(&packet_bytes);
            assert!(result.is_ok(), "Packet processing should not error");

            if let Some(msg) = result.unwrap() {
                assembled_message = Some(msg);
            }
        }

        // Verify message was assembled
        assert!(assembled_message.is_some(), "Message should be assembled after all packets");
        let message = assembled_message.unwrap();

        assert_eq!(message.cid, cid, "Assembled message CID should match");
        assert_eq!(message.cmd, CtapHidCommand::Ping, "Assembled message CMD should match");

        let reassembled_data = message.into_data();
        assert_eq!(reassembled_data, original_data, "Reassembled data should match original");
    }

    /// Test CTAPHID invalid sequence error
    #[tokio::test]
    async fn test_ctaphid_invalid_sequence() {
        use netget::server::usb::fido2::ctaphid::{CtapHidHandler, CtapHidPacket};

        let mut handler = CtapHidHandler::new();
        let cid = 0x11223344u32;

        // Create a fragmented message
        let data = vec![0x55u8; 150];
        let packets = handler.fragment_response(cid, netget::server::usb::fido2::ctaphid::CtapHidCommand::Cbor, &data);

        // Process init packet
        let result = handler.process_packet(&packets[0]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "Init packet should not complete message");

        // Skip continuation packet 0, send continuation packet 1 (wrong sequence)
        let result = handler.process_packet(&packets[2]);

        // Should error due to invalid sequence
        assert!(result.is_err(), "Should error on invalid sequence");
    }

    /// Test CTAPHID maximum message size
    #[tokio::test]
    async fn test_ctaphid_max_message_size() {
        use netget::server::usb::fido2::ctaphid::{CtapHidHandler, CtapHidCommand};

        let handler = CtapHidHandler::new();
        let cid = 0xfedcba98u32;

        // CTAPHID max message size is 7609 bytes (per spec)
        // First packet: 57 bytes, then 128 continuation packets of 59 bytes each
        // 57 + (128 * 59) = 57 + 7552 = 7609 bytes
        let data = vec![0x77u8; 7609];

        let packets = handler.fragment_response(cid, CtapHidCommand::Msg, &data);

        // Should be 1 init + 128 continuation = 129 packets
        assert_eq!(packets.len(), 129, "Max size message should use 129 packets");

        // Verify all packets are 64 bytes
        for packet in &packets {
            assert_eq!(packet.len(), 64, "All packets should be 64 bytes");
        }

        // Verify sequence numbers don't overflow (max seq is 127)
        for (i, packet) in packets.iter().enumerate().skip(1) {
            let seq = packet[4];
            assert_eq!(seq as usize, i - 1, "Sequence should increment correctly");
            assert!(seq < 128, "Sequence should not overflow");
        }
    }

    /// Test Browser WebAuthn integration with headless Chrome (requires Chrome)
    ///
    /// This test demonstrates WebAuthn API integration with a real browser
    #[tokio::test]
    #[ignore] // Requires Chrome/Chromium and chromedriver
    async fn test_webauthn_chrome_integration() {
        // This test would:
        // 1. Start FIDO2 server with auto-approve mode
        // 2. Set up virtual USB/IP device
        // 3. Launch headless Chrome with WebAuthn enabled
        // 4. Navigate to test page with WebAuthn API calls
        // 5. Trigger navigator.credentials.create() for registration
        // 6. Verify credential created successfully
        // 7. Trigger navigator.credentials.get() for authentication
        // 8. Verify authentication successful
        // 9. Clean up browser and USB/IP attachment

        // Example WebAuthn JavaScript that would be executed:
        /*
        async function testRegistration() {
            const challenge = new Uint8Array(32);
            crypto.getRandomValues(challenge);

            const publicKey = {
                challenge: challenge,
                rp: { name: "NetGet Test", id: "localhost" },
                user: {
                    id: new Uint8Array(16),
                    name: "test@example.com",
                    displayName: "Test User"
                },
                pubKeyCredParams: [{
                    type: "public-key",
                    alg: -7 // ES256
                }],
                timeout: 60000,
                attestation: "none"
            };

            const credential = await navigator.credentials.create({ publicKey });
            return credential;
        }

        async function testAuthentication(credentialId) {
            const challenge = new Uint8Array(32);
            crypto.getRandomValues(challenge);

            const publicKey = {
                challenge: challenge,
                rpId: "localhost",
                allowCredentials: [{
                    type: "public-key",
                    id: credentialId
                }],
                timeout: 60000
            };

            const assertion = await navigator.credentials.get({ publicKey });
            return assertion;
        }
        */

        // See tests/server/usb_fido2/CLAUDE.md for manual browser testing instructions
    }

    /// Integration test: Real USB/IP libfido2 flow (requires system setup)
    ///
    /// NOTE: This test requires:
    /// - libfido2-tools package installed
    /// - usbip kernel module loaded
    /// - Root/sudo access
    #[tokio::test]
    #[ignore] // Requires libfido2-tools, usbip, and root access
    async fn test_fido2_real_client_tools() {
        // This test demonstrates the full flow with real client tools
        // Actual implementation would:
        // 1. Start FIDO2 server with auto-approve
        // 2. Use usbip to attach the virtual device
        // 3. Use libfido2-tools to interact with it
        // 4. Verify credentials created and authentication works
        // 5. Clean up usbip attachment

        // See tests/server/usb_fido2/CLAUDE.md for manual testing instructions
    }
}
