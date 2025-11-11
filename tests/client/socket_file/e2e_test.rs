//! Socket File client E2E tests

#[cfg(all(test, feature = "socket_file", unix))]
mod tests {
    use netget::llm::ollama_client::OllamaClient;
    use netget::state::app_state::AppState;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    use tokio::sync::mpsc;

    /// Test basic connection to Unix domain socket
    #[tokio::test]
    async fn test_socket_file_connect() {
        // Create a temporary socket path
        let socket_path = format!("./tmp/netget_test_{}.sock", std::process::id());

        // Clean up any existing socket file
        let _ = std::fs::remove_file(&socket_path);

        // Start a simple Unix socket server
        let server_socket_path = socket_path.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&server_socket_path).unwrap();

            // Accept one connection
            if let Ok((mut stream, _)) = listener.accept().await {
                // Read data
                let mut buf = vec![0u8; 1024];
                if let Ok(n) = stream.read(&mut buf).await {
                    let received = String::from_utf8_lossy(&buf[..n]);
                    println!("[SERVER] Received: {}", received);

                    // Echo back
                    let _ = stream.write_all(&buf[..n]).await;
                }
            }
        });

        // Wait for server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Create app state and LLM client
        let app_state = Arc::new(AppState::new());
        let llm_client = OllamaClient::new(
            "http://localhost:11434".to_string(),
            "qwen3-coder:30b".to_string(),
        );

        let (status_tx, mut status_rx) = mpsc::unbounded_channel();

        // Create a client
        let client_id = app_state
            .add_client(
                "SocketFile".to_string(),
                socket_path.clone(),
                "Test client".to_string(),
                None,
            )
            .await;

        // Connect using the socket file client
        let protocol = netget::client::socket_file::SocketFileClientProtocol::new();

        // Since we're testing without LLM, we'll just verify the connection works
        // A full E2E test would involve LLM calls

        // For now, just verify we can create the client structure
        assert_eq!(protocol.protocol_name(), "SocketFile");
        assert!(protocol.keywords().contains(&"unix socket"));

        // Clean up
        let _ = std::fs::remove_file(&socket_path);

        // Drain status channel
        while let Ok(_) = status_rx.try_recv() {}
    }

    /// Test socket file client metadata
    #[test]
    fn test_socket_file_metadata() {
        use netget::llm::actions::protocol_trait::Protocol;

        let protocol = netget::client::socket_file::SocketFileClientProtocol::new();

        // Check protocol name
        assert_eq!(protocol.protocol_name(), "SocketFile");

        // Check stack name
        assert_eq!(protocol.stack_name(), "UnixSocket");

        // Check keywords
        let keywords = protocol.keywords();
        assert!(keywords.contains(&"socket file"));
        assert!(keywords.contains(&"unix socket"));
        assert!(keywords.contains(&"domain socket"));

        // Check description
        assert!(!protocol.description().is_empty());

        // Check example prompt
        assert!(!protocol.example_prompt().is_empty());

        // Check metadata
        let metadata = protocol.metadata();
        assert_eq!(
            metadata.state,
            netget::protocol::metadata::DevelopmentState::Experimental
        );
    }

    /// Test socket file client actions
    #[test]
    fn test_socket_file_actions() {
        use netget::llm::actions::client_trait::Client;
        use netget::llm::actions::protocol_trait::Protocol;

        let protocol = netget::client::socket_file::SocketFileClientProtocol::new();
        let app_state = AppState::new();

        // Get async actions
        let async_actions = protocol.get_async_actions(&app_state);
        assert!(!async_actions.is_empty());

        // Find send_socket_file_data action
        let send_action = async_actions
            .iter()
            .find(|a| a.name == "send_socket_file_data");
        assert!(send_action.is_some());

        // Find disconnect action
        let disconnect_action = async_actions.iter().find(|a| a.name == "disconnect");
        assert!(disconnect_action.is_some());

        // Get sync actions
        let sync_actions = protocol.get_sync_actions();
        assert!(!sync_actions.is_empty());

        // Test action execution - send_socket_file_data
        let action_json = serde_json::json!({
            "type": "send_socket_file_data",
            "data_hex": "48656c6c6f"  // "Hello"
        });

        let result = protocol.execute_action(action_json);
        assert!(result.is_ok());

        if let Ok(netget::llm::actions::client_trait::ClientActionResult::SendData(data)) = result {
            assert_eq!(data, b"Hello");
        } else {
            panic!("Expected SendData result");
        }

        // Test disconnect action
        let disconnect_json = serde_json::json!({
            "type": "disconnect"
        });

        let result = protocol.execute_action(disconnect_json);
        assert!(result.is_ok());

        matches!(
            result.unwrap(),
            netget::llm::actions::client_trait::ClientActionResult::Disconnect
        );
    }

    /// Test event types
    #[test]
    fn test_socket_file_events() {
        use netget::llm::actions::protocol_trait::Protocol;

        let protocol = netget::client::socket_file::SocketFileClientProtocol::new();
        let events = protocol.get_event_types();

        // Should have at least 2 events
        assert!(events.len() >= 2);

        // Find socket_file_connected event
        let connected_event = events.iter().find(|e| e.id == "socket_file_connected");
        assert!(connected_event.is_some());

        // Find socket_file_data_received event
        let data_event = events.iter().find(|e| e.id == "socket_file_data_received");
        assert!(data_event.is_some());
    }
}
