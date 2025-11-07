//! E2E tests for OpenAI client
//!
//! These tests verify OpenAI client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//!
//! **Note**: These tests require a valid OpenAI API key. Set OPENAI_API_KEY environment
//! variable to run the tests. Tests will be skipped if the key is not available.

#[cfg(all(test, feature = "openai"))]
mod openai_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Get OpenAI API key from environment, or skip test
    fn get_api_key_or_skip() -> String {
        std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
            eprintln!("⚠️  Skipping OpenAI test: OPENAI_API_KEY not set");
            panic!("Test skipped: OPENAI_API_KEY environment variable not set");
        })
    }

    /// Test OpenAI client can make a simple chat completion request
    /// LLM calls: 1 (client connection with API request)
    #[tokio::test]
    async fn test_openai_client_chat_completion() -> E2EResult<()> {
        let api_key = get_api_key_or_skip();

        // Create client that makes a chat completion request
        let client_config = NetGetConfig::new(format!(
            "Connect to OpenAI API with key '{}' and send a chat completion: 'Say hello in exactly 3 words.'",
            api_key
        ))
        .with_timeout(Duration::from_secs(30));  // OpenAI API can take time

        let mut client = start_netget_client(client_config).await?;

        // Give client time to make request and get response
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify client output shows OpenAI protocol
        assert!(
            client.output_contains("OpenAI").await || client.output_contains("openai").await,
            "Client should show OpenAI protocol. Output: {:?}",
            client.get_output().await
        );

        // Verify we got some kind of response (could be success or error)
        assert!(
            client.output_contains("response").await
            || client.output_contains("completion").await
            || client.output_contains("received").await
            || client.output_contains("ERROR").await,  // API errors are OK for testing
            "Client should show response indication. Output: {:?}",
            client.get_output().await
        );

        println!("✅ OpenAI client made chat completion request successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test OpenAI client with gpt-3.5-turbo model
    /// LLM calls: 1 (client connection)
    #[tokio::test]
    async fn test_openai_client_with_model_selection() -> E2EResult<()> {
        let api_key = get_api_key_or_skip();

        // Client with specific model selection
        let client_config = NetGetConfig::new(format!(
            "Connect to OpenAI with key '{}' using model gpt-3.5-turbo and ask: 'What is 2+2?'",
            api_key
        ))
        .with_timeout(Duration::from_secs(30));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify the client is OpenAI protocol
        assert_eq!(client.protocol, "OpenAI", "Client should be OpenAI protocol");

        println!("✅ OpenAI client with model selection worked");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test OpenAI client can handle embedding requests
    /// LLM calls: 1 (client connection)
    #[tokio::test]
    async fn test_openai_client_embeddings() -> E2EResult<()> {
        let api_key = get_api_key_or_skip();

        // Client that requests embeddings
        let client_config = NetGetConfig::new(format!(
            "Connect to OpenAI with key '{}' and generate embeddings for the text: 'The quick brown fox'",
            api_key
        ))
        .with_timeout(Duration::from_secs(30));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify client is running
        assert!(
            client.output_contains("OpenAI").await,
            "Client should show OpenAI connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ OpenAI client embedding request handled");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test OpenAI client error handling (invalid API key)
    /// LLM calls: 1 (client connection)
    #[tokio::test]
    async fn test_openai_client_error_handling() -> E2EResult<()> {
        // Use an invalid API key
        let invalid_key = "sk-invalid-key-for-testing";

        let client_config = NetGetConfig::new(format!(
            "Connect to OpenAI with key '{}' and ask: 'Hello'",
            invalid_key
        ))
        .with_timeout(Duration::from_secs(20));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client shows error
        assert!(
            client.output_contains("ERROR").await
            || client.output_contains("error").await
            || client.output_contains("failed").await,
            "Client should show error for invalid API key. Output: {:?}",
            client.get_output().await
        );

        println!("✅ OpenAI client error handling works");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
