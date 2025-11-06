//! E2E tests for Redis client

#[cfg(all(test, feature = "redis"))]
mod redis_client_tests {
    use netget::state::app_state::AppState;
    use std::sync::Arc;

    /// Test Redis client initialization
    /// LLM calls: 0 (unit test)
    #[tokio::test]
    async fn test_redis_client_initialization() {
        let state = Arc::new(AppState::new_with_options(false, true));

        let client_instance = netget::state::ClientInstance::new(
            netget::state::ClientId::new(1),
            "localhost:6379".to_string(),
            "Redis".to_string(),
            "Test Redis client".to_string(),
        );

        let client_id = state.add_client(client_instance).await;

        let client = state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.protocol_name, "Redis");
        assert_eq!(client.remote_addr, "localhost:6379");

        println!("✅ Redis client initialization works");

        state.remove_client(client_id).await;
    }

    /// Test Redis client status management
    /// LLM calls: 0 (unit test)
    #[tokio::test]
    async fn test_redis_client_status() {
        let state = Arc::new(AppState::new_with_options(false, true));

        let client_instance = netget::state::ClientInstance::new(
            netget::state::ClientId::new(1),
            "localhost:6379".to_string(),
            "Redis".to_string(),
            "Test status".to_string(),
        );

        let client_id = state.add_client(client_instance).await;

        // Test status transitions
        state.update_client_status(client_id, netget::state::ClientStatus::Connecting).await;
        let client = state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.status, netget::state::ClientStatus::Connecting);

        state.update_client_status(client_id, netget::state::ClientStatus::Connected).await;
        let client = state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.status, netget::state::ClientStatus::Connected);

        println!("✅ Redis client status management works");

        state.remove_client(client_id).await;
    }
}
