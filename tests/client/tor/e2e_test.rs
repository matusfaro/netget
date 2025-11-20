//! E2E tests for Tor client with directory query capabilities
//!
//! These tests verify Tor client functionality using mocks (no internet required).
//! Directory query actions are tested through the mock LLM infrastructure.

#[cfg(all(test, feature = "tor"))]
mod tor_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Tor client bootstrap with mocked consensus
    /// This test verifies the client can initialize without real Tor network
    /// LLM calls: 2 (client startup, bootstrap complete event)
    #[tokio::test]
    async fn test_tor_client_bootstrap_mocked() -> E2EResult<()> {
        // Start a Tor client with mocked bootstrap
        let client_config = NetGetConfig::new(
            "Connect via Tor to example.com:80. Wait for bootstrap to complete."
        )
            .with_mock(|mock| {
                mock
                    // Mock 1: Client startup (user instruction)
                    .on_instruction_containing("Connect via Tor")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": "example.com:80",
                            "protocol": "Tor",
                            "instruction": "Wait for bootstrap"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Bootstrap complete event
                    // Note: Real bootstrap would take 10-30s, but mocked bootstrap is instant
                    .on_event("tor_bootstrap_complete")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give client time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify mocks were called
        client.verify_mocks().await?;

        // Verify output shows Tor client activity
        // (In real implementation, bootstrap would emit events)
        assert!(
            client.output_contains("Tor").await || client.output_contains("CLIENT").await,
            "Client should show Tor activity. Output: {:?}",
            client.get_output().await
        );

        Ok(())
    }

    /// Test directory query: get consensus info
    /// LLM calls: 3 (client startup, bootstrap event, query action)
    #[tokio::test]
    async fn test_tor_directory_query_consensus() -> E2EResult<()> {
        let client_config = NetGetConfig::new(
            "Connect via Tor and query the consensus metadata. Report relay count."
        )
            .with_mock(|mock| {
                mock
                    // Mock 1: Open Tor client
                    .on_instruction_containing("query the consensus")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": "unused:80",
                            "protocol": "Tor",
                            "instruction": "Query consensus after bootstrap"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Bootstrap complete → query consensus
                    .on_event("tor_bootstrap_complete")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "get_consensus_info"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give client time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify mocks were called
        client.verify_mocks().await?;

        Ok(())
    }

    /// Test directory query: list relays
    /// LLM calls: 3 (client startup, bootstrap event, list action)
    #[tokio::test]
    async fn test_tor_directory_list_relays() -> E2EResult<()> {
        let client_config = NetGetConfig::new(
            "Connect via Tor and list 10 relays from the network directory."
        )
            .with_mock(|mock| {
                mock
                    // Mock 1: Open Tor client
                    .on_instruction_containing("list 10 relays")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": "unused:80",
                            "protocol": "Tor",
                            "instruction": "List relays after bootstrap"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Bootstrap complete → list relays
                    .on_event("tor_bootstrap_complete")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "list_relays",
                            "limit": 10
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give client time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify mocks were called
        client.verify_mocks().await?;

        Ok(())
    }

    /// Test directory query: search relays by flags
    /// LLM calls: 3 (client startup, bootstrap event, search action)
    #[tokio::test]
    async fn test_tor_directory_search_relays() -> E2EResult<()> {
        let client_config = NetGetConfig::new(
            "Connect via Tor and search for Exit relays. Show first 5 results."
        )
            .with_mock(|mock| {
                mock
                    // Mock 1: Open Tor client
                    .on_instruction_containing("search for Exit relays")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": "unused:80",
                            "protocol": "Tor",
                            "instruction": "Search Exit relays after bootstrap"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Bootstrap complete → search relays
                    .on_event("tor_bootstrap_complete")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "search_relays",
                            "flags": ["Exit"],
                            "limit": 5
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give client time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify mocks were called
        client.verify_mocks().await?;

        Ok(())
    }
}
