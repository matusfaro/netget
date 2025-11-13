//! E2E tests for BGP client
//!
//! These tests verify BGP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start BGP server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "bgp"))]
mod bgp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test BGP client connection to BGP server
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_bgp_client_connect_to_server() -> E2EResult<()> {
        // Start a BGP server on port 179 (or available port)
        let server_config = NetGetConfig::new(
            "Start BGP server on port {AVAILABLE_PORT} with AS 65000 and router ID 192.168.1.1. Accept connections and respond to OPEN messages."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_instruction_containing("Start BGP server")
                .and_instruction_containing("AS 65000")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "BGP",
                        "instruction": "BGP router AS 65000, router ID 192.168.1.1"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: OPEN received from client - respond with OPEN
                .on_event("bgp_open_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_open",
                        "my_as": 65000,
                        "hold_time": 180,
                        "router_id": "192.168.1.1"
                    },
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: KEEPALIVE from client - respond with KEEPALIVE
                .on_event("bgp_keepalive_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_keepalive"
                    },
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_at_most(1)  // Client may or may not send KEEPALIVE in this test
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Start a BGP client that connects to this server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via BGP with AS 65001 and router ID 192.168.1.100. Establish BGP session.",
            server.port
        ))
        .with_startup_params(serde_json::json!({
            "local_as": 65001,
            "router_id": "192.168.1.100",
            "hold_time": 180
        }))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect to")
                .and_instruction_containing("BGP")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "BGP",
                        "instruction": "Establish BGP session with AS 65001"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Connected - send OPEN
                .on_event("bgp_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_open",
                        "my_as": 65001,
                        "hold_time": 180,
                        "router_id": "192.168.1.100"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: OPEN received from server
                .on_event("bgp_open_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_at_most(1)  // May receive server's OPEN
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and establish session
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        let output = client.get_output().await;
        assert!(
            client.output_contains("connected").await || client.output_contains("OPEN").await,
            "Client should show BGP connection or OPEN message. Output: {:?}",
            output
        );

        println!("✅ BGP client connected to server successfully");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test BGP client session establishment
    /// LLM calls: 3 (server startup, client connection, session handling)
    #[tokio::test]
    async fn test_bgp_client_session_establishment() -> E2EResult<()> {
        // Start BGP server
        let server_config = NetGetConfig::new(
            "Start BGP server on port {AVAILABLE_PORT} with AS 65000 and router ID 192.168.1.1. Complete OPEN handshake."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Start BGP server")
                .and_instruction_containing("AS 65000")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "BGP",
                        "instruction": "BGP router AS 65000, router ID 192.168.1.1"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: OPEN received - respond with OPEN
                .on_event("bgp_open_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_open",
                        "my_as": 65000,
                        "hold_time": 180,
                        "router_id": "192.168.1.1"
                    },
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: KEEPALIVE received - respond with KEEPALIVE
                .on_event("bgp_keepalive_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_keepalive"
                    },
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_at_most(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client connects and establishes session
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via BGP. Establish session with AS 65001 and router ID 192.168.1.100. Wait for session to be established.",
            server.port
        ))
        .with_startup_params(serde_json::json!({
            "local_as": 65001,
            "router_id": "192.168.1.100"
        }))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect to")
                .and_instruction_containing("BGP")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "BGP",
                        "instruction": "Establish BGP session with AS 65001"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Connected - send OPEN
                .on_event("bgp_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_open",
                        "my_as": 65001,
                        "hold_time": 180,
                        "router_id": "192.168.1.100"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: OPEN received - send KEEPALIVE
                .on_event("bgp_open_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_keepalive"
                    },
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: KEEPALIVE received from server
                .on_event("bgp_keepalive_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_at_most(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Wait for session establishment (OPEN + KEEPALIVE exchange)
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify protocol
        assert_eq!(client.protocol, "BGP", "Client should be BGP protocol");

        // Verify output shows session activity
        let output = client.get_output().await;
        assert!(
            client.output_contains("BGP").await,
            "Client should show BGP activity. Output: {:?}",
            output
        );

        println!("✅ BGP client session established");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test BGP client with custom AS and router ID
    /// LLM calls: 2 (server startup, client with params)
    #[tokio::test]
    async fn test_bgp_client_custom_params() -> E2EResult<()> {
        // Start BGP server
        let server_config = NetGetConfig::new(
            "Start BGP server on port {AVAILABLE_PORT} with AS 64512. Accept BGP connections.",
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Start BGP server")
                .and_instruction_containing("AS 64512")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "BGP",
                        "instruction": "BGP router AS 64512"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: OPEN received - respond with OPEN
                .on_event("bgp_open_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_open",
                        "my_as": 64512,
                        "hold_time": 180,
                        "router_id": "192.168.1.1"
                    },
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_at_most(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client with custom AS and router ID
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via BGP. Use AS 64513 and router ID 10.0.0.1.",
            server.port
        ))
        .with_startup_params(serde_json::json!({
            "local_as": 64513,
            "router_id": "10.0.0.1",
            "hold_time": 120
        }))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect to")
                .and_instruction_containing("BGP")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "BGP",
                        "instruction": "Connect with AS 64513"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Connected - send OPEN with custom params
                .on_event("bgp_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_bgp_open",
                        "my_as": 64513,
                        "hold_time": 120,
                        "router_id": "10.0.0.1"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify connection
        assert!(
            client.output_contains("BGP").await || client.output_contains("connected").await,
            "Client should show BGP connection with custom params"
        );

        println!("✅ BGP client with custom AS/router ID");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
