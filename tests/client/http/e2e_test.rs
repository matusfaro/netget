//! E2E tests for HTTP client

#[cfg(all(test, feature = "http"))]
mod http_client_tests {
    use netget::state::app_state::AppState;
    use std::sync::Arc;

    /// Test HTTP client initialization
    /// LLM calls: 0 (unit test)
    #[tokio::test]
    async fn test_http_client_initialization() {
        let state = Arc::new(AppState::new_with_options(false, true));

        let client_instance = netget::state::ClientInstance::new(
            netget::state::ClientId::new(1),
            "http://httpbin.org".to_string(),
            "HTTP".to_string(),
            "Test HTTP client".to_string(),
        );

        let client_id = state.add_client(client_instance).await;

        let client = state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.protocol_name, "HTTP");
        assert_eq!(client.remote_addr, "http://httpbin.org");

        println!("✅ HTTP client initialization works");

        state.remove_client(client_id).await;
    }

    /// Test HTTP client status management
    /// LLM calls: 0 (unit test)
    #[tokio::test]
    async fn test_http_client_status() {
        let state = Arc::new(AppState::new_with_options(false, true));

        let client_instance = netget::state::ClientInstance::new(
            netget::state::ClientId::new(1),
            "http://example.com".to_string(),
            "HTTP".to_string(),
            "Test status".to_string(),
        );

        let client_id = state.add_client(client_instance).await;

        // Test status transitions
        state.update_client_status(client_id, netget::state::ClientStatus::Connected).await;
        let client = state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.status, netget::state::ClientStatus::Connected);

        state.update_client_status(client_id, netget::state::ClientStatus::Disconnected).await;
        let client = state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.status, netget::state::ClientStatus::Disconnected);

        println!("✅ HTTP client status management works");

        state.remove_client(client_id).await;
    }
}
