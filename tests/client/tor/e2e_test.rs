//! E2E tests for Tor client with local directory server
//!
//! These tests verify Tor client functionality using a local tor_directory server
//! (fully local, no internet required).

#[cfg(all(test, feature = "tor"))]
mod tor_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Tor client connecting to local tor_directory server
    /// This test starts a local tor_directory server serving mock consensus,
    /// then connects a Tor client configured to use that local directory.
    ///
    /// LLM calls: 4 (server startup, directory request, client startup, bootstrap event)
    #[tokio::test]
    async fn test_tor_client_with_local_directory() -> E2EResult<()> {
        // Start a local tor_directory server with mocked consensus
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Tor Directory. Serve a simple test consensus."
        )
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Listen on port")
                    .and_instruction_containing("Tor Directory")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Tor Directory",
                            "instruction": "Serve a minimal test consensus with 3 mock relays when requested"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Directory request from client
                    .on_event("tor_directory_request")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "serve_consensus",
                            "consensus": "network-status-version 3\nvote-status consensus\nconsensus-method 35\nvalid-after 2025-01-01 00:00:00\nfresh-until 2025-01-01 01:00:00\nvalid-until 2025-01-01 03:00:00\nvoting-delay 300 300\nclient-versions 0.4.7.0-0.4.8.0\nserver-versions 0.4.7.0-0.4.8.0\nknown-flags Authority BadExit Exit Fast Guard HSDir NoEdConsensus Running Stable StaleDesc V2Dir Valid\nparams cbtnummodes=3 maxunmeasuredbw=10000\nr TestRelay1 AAAAAAAAAAAAAAAAAAAAAAAAAAA 127.0.0.1 9001 0 0\ns Exit Fast Guard Running Stable Valid\nw Bandwidth=1000\np accept 1-65535\nr TestRelay2 BBBBBBBBBBBBBBBBBBBBBBBBBBB 127.0.0.1 9002 0 0\ns Fast Guard Running Stable Valid\nw Bandwidth=1000\np accept 1-65535\nr TestRelay3 CCCCCCCCCCCCCCCCCCCCCCCCCCC 127.0.0.1 9003 0 0\ns Fast Running Stable Valid\nw Bandwidth=1000\np accept 1-65535\n"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_secs(1)).await;

        println!("✓ Directory server started on port {}", server.port);

        // Now start Tor client pointing to local directory
        let client_config = NetGetConfig::new(format!(
            "Connect via Tor to example.com:80 using local directory server at 127.0.0.1:{}",
            server.port
        ))
            .with_mock(|mock| {
                mock
                    // Mock 1: Client startup with directory_server parameter
                    .on_instruction_containing("Connect via Tor")
                    .and_instruction_containing("local directory")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": "example.com:80",
                            "protocol": "Tor",
                            "instruction": "Bootstrap and query consensus",
                            "startup_params": {
                                "directory_server": format!("127.0.0.1:{}", server.port)
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Bootstrap complete event
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

        // Give time for bootstrap (local directory is fast)
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify mocks were called
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Verify output shows Tor client connected to local directory
        let client_output = client.get_output().await;
        println!("Client output: {:?}", client_output);

        let has_tor_mention = client_output.iter().any(|line| {
            line.contains("Tor") || line.contains("directory") || line.contains("CLIENT")
        });

        assert!(
            has_tor_mention,
            "Client should mention Tor or directory. Output: {:?}",
            client_output
        );

        println!("✓ Tor client successfully connected to local directory server");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test directory query actions with local directory
    /// LLM calls: 6 (server startup, directory request, client startup, bootstrap, get_consensus_info, response)
    #[tokio::test]
    async fn test_tor_directory_query_local() -> E2EResult<()> {
        // Start local tor_directory server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Tor Directory. Serve test consensus."
        )
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("Tor Directory")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Tor Directory",
                            "instruction": "Serve consensus"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("tor_directory_request")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "serve_consensus",
                            "consensus": "network-status-version 3\nvote-status consensus\nconsensus-method 35\nvalid-after 2025-01-01 00:00:00\nfresh-until 2025-01-01 01:00:00\nvalid-until 2025-01-01 03:00:00\nvoting-delay 300 300\nr TestRelay AAAAAAAAAAAAAAAAAAAAAAAAAAA 127.0.0.1 9001 0 0\ns Exit Fast Guard Running Stable Valid\nw Bandwidth=1000\n"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Start Tor client with directory query instruction
        let client_config = NetGetConfig::new(format!(
            "Connect via Tor using directory 127.0.0.1:{} and query consensus metadata",
            server.port
        ))
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("query consensus")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": "unused:80",
                            "protocol": "Tor",
                            "instruction": "Query consensus after bootstrap",
                            "startup_params": {
                                "directory_server": format!("127.0.0.1:{}", server.port)
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
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
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify mocks
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        println!("✓ Directory query test completed successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
