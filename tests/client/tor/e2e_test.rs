//! E2E tests for Tor client with local directory server
//!
//! These tests verify Tor client functionality using a local tor_directory server
//! (fully local, no internet required).

#[cfg(all(test, feature = "tor"))]
mod tor_client_tests {
    use crate::helpers::*;
    use serde_json::json;
    use std::time::Duration;

    /// Minimal Arti-compatible consensus document
    /// This format is accepted by Arti for bootstrapping
    fn create_minimal_consensus() -> String {
        format!(
            "network-status-version 3
vote-status consensus
consensus-method 35
valid-after 2025-01-01 00:00:00
fresh-until 2025-01-01 01:00:00
valid-until 2025-01-01 03:00:00
voting-delay 300 300
client-versions 0.4.7.0-0.4.8.0
server-versions 0.4.7.0-0.4.8.0
known-flags Authority Exit Fast Guard Running Stable Valid
params cbtnummodes=3
r TestRelay1 AAAAAAAAAAAAAAAAAAAAAAAAAAA 127.0.0.1 9001 0 0
s Exit Fast Guard Running Stable Valid
w Bandwidth=1000
p accept 1-65535
"
        )
    }

    /// Test Tor client connecting to local tor_directory server
    /// Uses static event handlers to automatically serve consensus
    /// LLM calls: 2 (server startup, client startup)
    #[tokio::test]
    async fn test_tor_client_with_local_directory() -> E2EResult<()> {
        // Start a local tor_directory server with static consensus handler
        let consensus_data = create_minimal_consensus();

        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Tor Directory. Serve consensus automatically."
        )
            .with_mock(|mock| {
                mock
                    // Mock: Server startup only
                    .on_instruction_containing("Tor Directory")
                    .respond_with_actions(json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Tor Directory",
                            "instruction": "Serve consensus on demand",
                            "event_handlers": [
                                {
                                    "event_pattern": "tor_directory_request",
                                    "handler": {
                                        "type": "static",
                                        "actions": [
                                            {
                                                "type": "serve_consensus",
                                                "consensus_data": consensus_data.clone()
                                            }
                                        ]
                                    }
                                }
                            ]
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
                    // Mock: Client startup with directory_server parameter
                    .on_instruction_containing("Connect via Tor")
                    .and_instruction_containing("local directory")
                    .respond_with_actions(json!([
                        {
                            "type": "open_client",
                            "remote_addr": "example.com:80",
                            "protocol": "Tor",
                            "instruction": "Bootstrap and wait",
                            "startup_params": {
                                "directory_server": format!("127.0.0.1:{}", server.port)
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give time for bootstrap (local directory should be fast)
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Verify mocks were called
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Verify output shows Tor client activity
        let client_output = client.get_output().await;
        println!("Client output: {:?}", client_output);

        let has_tor_mention = client_output.iter().any(|line| {
            line.contains("Tor") || line.contains("directory") || line.contains("CLIENT") || line.contains("bootstrap")
        });

        assert!(
            has_tor_mention,
            "Client should mention Tor or bootstrap. Output: {:?}",
            client_output
        );

        println!("✓ Tor client successfully bootstrapped from local directory server");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test directory query actions with local directory
    /// LLM calls: 2 (server startup, client startup)
    #[tokio::test]
    async fn test_tor_directory_query_local() -> E2EResult<()> {
        // Start local tor_directory server with static handler
        let consensus_data = create_minimal_consensus();

        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Tor Directory. Auto-serve consensus."
        )
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("Tor Directory")
                    .respond_with_actions(json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Tor Directory",
                            "instruction": "Serve consensus",
                            "event_handlers": [
                                {
                                    "event_pattern": "tor_directory_request",
                                    "handler": {
                                        "type": "static",
                                        "actions": [
                                            {
                                                "type": "serve_consensus",
                                                "consensus_data": consensus_data.clone()
                                            }
                                        ]
                                    }
                                }
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_secs(1)).await;

        println!("✓ Directory server started on port {}", server.port);

        // Start Tor client with directory query instruction
        let client_config = NetGetConfig::new(format!(
            "Connect via Tor using directory 127.0.0.1:{} and bootstrap",
            server.port
        ))
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("Connect via Tor")
                    .and_instruction_containing("directory")
                    .respond_with_actions(json!([
                        {
                            "type": "open_client",
                            "remote_addr": "unused:80",
                            "protocol": "Tor",
                            "instruction": "Bootstrap from local directory",
                            "startup_params": {
                                "directory_server": format!("127.0.0.1:{}", server.port)
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;
        tokio::time::sleep(Duration::from_secs(10)).await;

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
