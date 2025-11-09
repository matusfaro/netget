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
