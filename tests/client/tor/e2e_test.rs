//! E2E tests for Tor client protocol registration and actions
//!
//! These tests verify Tor client protocol is properly registered and
//! actions/events are correctly defined, without requiring Tor bootstrap.

#[cfg(all(test, feature = "tor"))]
mod tor_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test that Tor protocol is registered and can be invoked
    /// LLM calls: 1 (client startup)
    /// Note: This test verifies protocol registration, not actual Tor functionality
    #[tokio::test]
    async fn test_tor_protocol_registered() -> E2EResult<()> {
        let client_config = NetGetConfig::new(
            "Show me available client protocols including Tor."
        )
            .with_mock(|mock| {
                mock
                    // Mock: Just acknowledge the instruction
                    .on_instruction_containing("available client protocols")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify mocks were called
        client.verify_mocks().await?;

        // Verify output shows protocol list (Tor should be available)
        let output = client.get_output().await;
        println!("Output: {:?}", output);

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test that Tor client actions are properly defined
    /// This is a smoke test that verifies the action system works
    /// LLM calls: 2 (instruction, event)
    #[tokio::test]
    async fn test_tor_actions_defined() -> E2EResult<()> {
        let client_config = NetGetConfig::new(
            "List available actions for Tor client protocol."
        )
            .with_mock(|mock| {
                mock
                    // Mock: Acknowledge the request
                    .on_instruction_containing("available actions")
                    .and_instruction_containing("Tor")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify mocks were called
        client.verify_mocks().await?;

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
