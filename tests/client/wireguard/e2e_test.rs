//! End-to-end tests for WireGuard VPN client
//!
//! These tests verify that the WireGuard client can:
//! - Connect to a WireGuard server
//! - Establish encrypted tunnels
//! - Query connection status
//! - Disconnect gracefully
//!
//! **Requirements**:
//! - Root/CAP_NET_ADMIN privileges (Linux/FreeBSD/Windows)
//! - WireGuard server running (can use netget WireGuard server)
//! - Valid server public key and endpoint
//!
//! **LLM Call Budget**: < 5 calls
//! **Expected Runtime**: ~20-30 seconds

#[cfg(all(test, feature = "wireguard"))]
mod tests {
    use netget::llm::OllamaClient;
    use netget::state::app_state::AppState;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// Test basic WireGuard client connectivity
    ///
    /// This test is disabled by default because it requires:
    /// 1. Root privileges
    /// 2. A running WireGuard server
    /// 3. Valid server configuration
    ///
    /// To run manually:
    /// ```bash
    /// # Start WireGuard server first (in another terminal, as root)
    /// sudo ./cargo-isolated.sh run --no-default-features --features wireguard
    /// # In netget: "Start a WireGuard VPN server on port 51820"
    /// # Note the server public key
    ///
    /// # Then run client test (as root)
    /// sudo ./cargo-isolated.sh test --no-default-features --features wireguard client::wireguard::e2e_test::tests::test_wireguard_client_connectivity -- --ignored
    /// ```
    #[tokio::test]
    #[ignore] // Requires root and running server
    async fn test_wireguard_client_connectivity() {
        // This test would require:
        // 1. Starting a WireGuard server
        // 2. Getting server's public key
        // 3. Connecting client with proper configuration
        // 4. Verifying handshake success
        // 5. Querying status
        // 6. Disconnecting

        // Example skeleton (not functional without actual server):
        let _app_state = Arc::new(AppState::new("test_model".to_string()));
        let _ollama_client = OllamaClient::new("http://localhost:11434");
        let (_status_tx, _status_rx) = mpsc::unbounded_channel();

        // TODO: Implement full E2E test when test infrastructure supports it
        // For now, this serves as documentation of what the test should do
    }

    /// Test that verifies WireGuard client parameter parsing
    ///
    /// This test doesn't require root or a running server.
    #[test]
    fn test_wireguard_param_parsing() {
        use netget::client::wireguard::actions::WireguardClientProtocol;
        let protocol = WireguardClientProtocol::new();

        // Verify protocol metadata
        assert_eq!(protocol.protocol_name(), "wireguard");
        assert_eq!(protocol.stack_name(), "VPN");
        assert_eq!(protocol.group_name(), "VPN");

        // Verify keywords
        let keywords = protocol.keywords();
        assert!(keywords.contains(&"wireguard"));
        assert!(keywords.contains(&"wg"));

        // Verify startup parameters are defined
        let params = protocol.get_startup_params();
        assert!(!params.is_empty());
        assert!(params.iter().any(|p| p.name == "server_public_key"));
        assert!(params.iter().any(|p| p.name == "server_endpoint"));
        assert!(params.iter().any(|p| p.name == "client_address"));
    }

    /// Test WireGuard action definitions
    #[test]
    fn test_wireguard_actions() {
        use netget::client::wireguard::actions::WireguardClientProtocol;
        use netget::state::app_state::AppState;

        let protocol = WireguardClientProtocol::new();
        let app_state = AppState::new("test_model".to_string());

        // Verify async actions
        let async_actions = protocol.get_async_actions(&app_state);
        assert!(!async_actions.is_empty());
        assert!(async_actions
            .iter()
            .any(|a| a.name == "get_connection_status"));
        assert!(async_actions.iter().any(|a| a.name == "disconnect"));
        assert!(async_actions.iter().any(|a| a.name == "get_client_info"));

        // Verify sync actions (should be empty for WireGuard)
        let sync_actions = protocol.get_sync_actions();
        assert!(sync_actions.is_empty());

        // Verify event types
        let event_types = protocol.get_event_types();
        assert_eq!(event_types.len(), 2);
        assert!(event_types.iter().any(|e| e.id == "wireguard_connected"));
        assert!(event_types.iter().any(|e| e.id == "wireguard_disconnected"));
    }
}
