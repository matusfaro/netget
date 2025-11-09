//! E2E tests for Ollama client
//!
//! These tests verify Ollama client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//!
//! **Note**: These tests require a running Ollama server on localhost:11434.
//! Tests will skip if Ollama is not available.

#[cfg(all(test, feature = "ollama"))]
mod ollama_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Check if Ollama server is running on localhost
    async fn is_ollama_available() -> bool {
        let client = reqwest::Client::new();
        client
            .get("http://localhost:11434/api/tags")
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_ok()
    }

    /// Skip test if Ollama is not available
    async fn require_ollama() {
        if !is_ollama_available().await {
            eprintln!("⚠️  Skipping Ollama test: Ollama server not running on localhost:11434");
            panic!("Test skipped: Ollama server not available");
        }
    }

    /// Test Ollama client can list models
    /// LLM calls: 1 (client connection with list models action)
    #[tokio::test]
    async fn test_ollama_client_list_models() -> E2EResult<()> {
        require_ollama().await;

        // Create client that lists available models
        let client_config = NetGetConfig::new(
            "Connect to Ollama at http://localhost:11434 and list all available models"
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and list models
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client output shows Ollama protocol
        assert!(
            client.output_contains("Ollama").await || client.output_contains("ollama").await,
            "Client should show Ollama protocol. Output: {:?}",
            client.get_output().await
        );

        // Verify we got models response
        assert!(
            client.output_contains("models").await
            || client.output_contains("model").await
            || client.output_contains("found").await
            || client.output_contains("received").await,
            "Client should show models response. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Ollama client listed models successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test Ollama client can generate text
    /// LLM calls: 1 (client connection with generate request)
    #[tokio::test]
    async fn test_ollama_client_generate() -> E2EResult<()> {
        require_ollama().await;

        // Create client that generates text
        let client_config = NetGetConfig::new(
            "Connect to Ollama at http://localhost:11434 and generate text: \
            'Say hello in exactly 2 words' using model qwen2.5-coder:0.5b"
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to generate
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify client shows Ollama protocol
        assert!(
            client.output_contains("Ollama").await,
            "Client should show Ollama protocol. Output: {:?}",
            client.get_output().await
        );

        // Verify we got a response
        assert!(
            client.output_contains("response").await
            || client.output_contains("generate").await
            || client.output_contains("received").await,
            "Client should show generation response. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Ollama client generated text successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test Ollama client chat completion
    /// LLM calls: 1 (client connection with chat request)
    #[tokio::test]
    async fn test_ollama_client_chat() -> E2EResult<()> {
        require_ollama().await;

        // Create client that sends chat request
        let client_config = NetGetConfig::new(
            "Connect to Ollama at http://localhost:11434 and send a chat message: \
            'What is 2+2?' using model qwen2.5-coder:0.5b"

        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to chat
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify client is Ollama protocol
        assert_eq!(client.protocol, "Ollama", "Client should be Ollama protocol");

        // Verify we got a chat response
        assert!(
            client.output_contains("chat").await
            || client.output_contains("message").await
            || client.output_contains("response").await,
            "Client should show chat response. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Ollama client chat completion worked");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test Ollama client with custom endpoint
    /// LLM calls: 1 (client connection)
    #[tokio::test]
    async fn test_ollama_client_custom_endpoint() -> E2EResult<()> {
        require_ollama().await;

        // Test with explicit endpoint
        let client_config = NetGetConfig::new(
            "Connect to Ollama API at http://localhost:11434 and list models"

        );

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client connected
        assert!(
            client.output_contains("localhost:11434").await
            || client.output_contains("11434").await
            || client.output_contains("Ollama").await,
            "Client should show connection to custom endpoint. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Ollama client with custom endpoint worked");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test Ollama client error handling (invalid server)
    /// LLM calls: 1 (client connection attempt)
    #[tokio::test]
    async fn test_ollama_client_error_handling() -> E2EResult<()> {
        // Use an invalid endpoint
        let client_config = NetGetConfig::new(
            "Connect to Ollama at http://localhost:99999 and list models"

        );

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client shows error or connection issue
        assert!(
            client.output_contains("ERROR").await
            || client.output_contains("error").await
            || client.output_contains("failed").await
            || client.output_contains("connect").await,
            "Client should show error for invalid endpoint. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Ollama client error handling worked");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
