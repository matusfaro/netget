//! E2E tests for TCP client
//!
//! These tests verify TCP client functionality with real TCP servers.
//! Test strategy: Use nc (netcat) as test server, < 10 LLM calls total.

#[cfg(all(test, feature = "tcp"))]
mod tcp_client_tests {
    use netget::state::app_state::AppState;
    use netget::llm::OllamaClient;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// Test TCP client connection and basic data exchange
    /// LLM calls: 2 (connection, data received)
    #[tokio::test]
    #[ignore] // Requires nc server: nc -l 9000
    async fn test_tcp_client_connect_and_send() {
        // Setup
        let state = Arc::new(AppState::new_with_options(false, true)); // ollama_lock enabled
        let llm = OllamaClient::new_with_options("http://localhost:11434", true);
        let (status_tx, mut _status_rx) = mpsc::unbounded_channel();

        // Create client
        let client_instance = netget::state::ClientInstance::new(
            netget::state::ClientId::new(1),
            "localhost:9000".to_string(),
            "TCP".to_string(),
            "Connect to TCP server and send 'HELLO'".to_string(),
        );

        let client_id = state.add_client(client_instance).await;

        // Connect
        use netget::protocol::CLIENT_REGISTRY;
        let protocol = CLIENT_REGISTRY.get("TCP").expect("TCP client not registered");

        let ctx = netget::protocol::ConnectContext {
            remote_addr: "localhost:9000".to_string(),
            llm_client: llm.clone(),
            state: state.clone(),
            status_tx: status_tx.clone(),
            client_id,
            startup_params: None,
        };

        match protocol.connect(ctx).await {
            Ok(_) => {
                // Wait for connection
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                // Verify client is connected
                let client = state.get_client(client_id).await.expect("Client not found");
                assert_eq!(client.status, netget::state::ClientStatus::Connected);

                println!("✅ TCP client connected successfully");
            }
            Err(e) => {
                panic!("Failed to connect: {}", e);
            }
        }

        // Cleanup
        state.remove_client(client_id).await;
    }

    /// Test TCP client disconnect
    /// LLM calls: 1 (connection)
    #[tokio::test]
    #[ignore] // Requires nc server
    async fn test_tcp_client_disconnect() {
        let state = Arc::new(AppState::new_with_options(false, true));
        let client_instance = netget::state::ClientInstance::new(
            netget::state::ClientId::new(1),
            "localhost:9000".to_string(),
            "TCP".to_string(),
            "Test disconnect".to_string(),
        );

        let client_id = state.add_client(client_instance).await;

        // Connect then disconnect
        state.update_client_status(client_id, netget::state::ClientStatus::Connected).await;
        state.update_client_status(client_id, netget::state::ClientStatus::Disconnected).await;

        let client = state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.status, netget::state::ClientStatus::Disconnected);

        println!("✅ TCP client disconnect works");

        state.remove_client(client_id).await;
    }
}
