//! Minimal test to verify mock Ollama server works

mod helpers;

#[cfg(all(test, feature = "tcp"))]
mod minimal_test {
    use serde_json::json;
    use crate::helpers::common::E2EResult;
    use crate::helpers::netget::NetGetConfig;

    #[tokio::test]
    async fn test_mock_server_minimal() -> E2EResult<()> {
        println!("\n=== Minimal Mock Server Test ===");

        // Simplest possible test: just pass a prompt and expect a mock response
        let config = NetGetConfig::new("test prompt")
            .with_log_level("debug")  // More verbose logging
            .with_mock(|mock| {
                mock
                    .on_any()  // Match ANY request
                    .respond_with_actions(json!([
                        {
                            "type": "show_message",
                            "message": "Mock server is working!"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        println!("Starting NetGet with mock...");
        let instance = crate::helpers::netget::start_netget(config).await?;

        println!("NetGet started, waiting for LLM call...");
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Get output
        let output = instance.get_output().await;
        let output_text = output.join("\n");

        println!("\n=== NetGet Output ===");
        println!("{}", output_text);
        println!("=== End Output ===\n");

        // Verify the mock was called
        println!("Verifying mock expectations...");
        instance.verify_mocks().await?;

        println!("✅ Mock server test passed!");
        Ok(())
    }
}
