//! OAuth2 client E2E tests

#![cfg(all(test, feature = "oauth2"))]

use netget::cli::client_startup::start_client_by_id;
use netget::llm::OllamaClient;
use netget::state::app_state::AppState;
use netget::state::client::{ClientInstance, ClientStatus};
use netget::state::ClientId;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Helper to create a test OAuth2 client instance
fn create_oauth2_client_instance(
    remote_addr: String,
    client_id_str: String,
    client_secret: Option<String>,
    token_url: String,
    instruction: String,
) -> ClientInstance {
    let mut protocol_data = serde_json::Map::new();
    protocol_data.insert("client_id".to_string(), serde_json::json!(client_id_str));
    if let Some(secret) = client_secret {
        protocol_data.insert("client_secret".to_string(), serde_json::json!(secret));
    }
    protocol_data.insert("token_url".to_string(), serde_json::json!(token_url));

    ClientInstance {
        id: ClientId::new(1),
        protocol_name: "OAuth2".to_string(),
        remote_addr,
        instruction,
        memory: None,
        status: ClientStatus::Connecting,
        startup_params: Some(serde_json::json!(protocol_data)),
        protocol_data: Some(protocol_data),
    }
}

#[tokio::test]
#[ignore] // Ignored by default as it requires a mock OAuth2 server
async fn test_oauth2_client_initialization() {
    // This is a basic smoke test that verifies OAuth2 client can be created
    // Full E2E tests require a mock OAuth2 server

    let app_state = Arc::new(AppState::new());
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();

    // Create OAuth2 client instance
    let client = create_oauth2_client_instance(
        "http://localhost:8080".to_string(),
        "test-client-id".to_string(),
        Some("test-client-secret".to_string()),
        "http://localhost:8080/oauth/token".to_string(),
        "Test OAuth2 client initialization".to_string(),
    );

    let client_id = client.id;
    app_state.add_client(client).await;

    // Note: Actual connection requires Ollama and mock OAuth2 server
    // This test just verifies the client can be created
    assert!(app_state.get_client(client_id).await.is_some());

    // Clean up
    drop(status_tx);
    let _ = status_rx.recv().await;
}

#[tokio::test]
#[ignore] // Requires mock OAuth2 server + Ollama
async fn test_oauth2_password_flow() {
    // This test requires:
    // 1. Mock OAuth2 server running on http://localhost:8080
    // 2. Ollama instance running
    // 3. Mock server configured to accept password grant

    // TODO: Implement mock OAuth2 server
    // TODO: Test password flow end-to-end
    // TODO: Verify access token obtained and stored

    // Test skeleton:
    // 1. Start mock OAuth2 server
    // 2. Create OAuth2 client with password flow instruction
    // 3. Start client (triggers LLM + authentication)
    // 4. Wait for token obtained event
    // 5. Assert access token in protocol_data
    // 6. Stop mock server
}

#[tokio::test]
#[ignore] // Requires mock OAuth2 server + Ollama
async fn test_oauth2_client_credentials_flow() {
    // This test requires:
    // 1. Mock OAuth2 server running
    // 2. Ollama instance running
    // 3. Mock server configured to accept client_credentials grant

    // TODO: Implement mock OAuth2 server
    // TODO: Test client credentials flow
    // TODO: Verify access token obtained (no refresh token expected)
}

#[tokio::test]
#[ignore] // Requires mock OAuth2 server + Ollama
async fn test_oauth2_token_refresh() {
    // This test requires:
    // 1. Mock OAuth2 server with refresh token support
    // 2. Ollama instance running
    // 3. Initial authentication to obtain refresh token
    // 4. Trigger refresh flow

    // TODO: Implement token refresh test
    // TODO: Verify new access token replaces old one
}

#[tokio::test]
#[ignore] // Requires mock OAuth2 server + Ollama
async fn test_oauth2_error_handling() {
    // This test requires:
    // 1. Mock OAuth2 server configured to return errors
    // 2. Ollama instance running

    // TODO: Implement error handling test
    // TODO: Verify oauth2_error event fired
    // TODO: Verify client remains in Connected state
}

// Note: Device code flow and authorization code flow tests
// are not automated due to complexity (polling, browser redirects)
// These should be tested manually following the test documentation

#[cfg(test)]
mod helpers {
    use super::*;

    /// Start a mock OAuth2 server for testing
    /// Returns the server URL
    #[allow(dead_code)]
    pub async fn start_mock_oauth_server() -> String {
        // TODO: Implement mock OAuth2 server using axum
        // Should handle:
        // - POST /oauth/token for token endpoint
        // - Support multiple grant types
        // - Return mock tokens

        "http://localhost:8080".to_string()
    }

    /// Assert that an access token is stored for a client
    #[allow(dead_code)]
    pub async fn assert_token_stored(app_state: &AppState, client_id: ClientId) {
        let has_token = app_state
            .with_client_mut(client_id, |client| {
                client
                    .get_protocol_field("access_token")
                    .and_then(|v| v.as_str())
                    .is_some()
            })
            .await;

        assert!(has_token, "Access token should be stored");
    }

    /// Extract access token from client protocol data
    #[allow(dead_code)]
    pub async fn extract_access_token(app_state: &AppState, client_id: ClientId) -> Option<String> {
        app_state
            .with_client_mut(client_id, |client| {
                client
                    .get_protocol_field("access_token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .await
    }
}
