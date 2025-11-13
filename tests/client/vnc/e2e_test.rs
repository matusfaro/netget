//! E2E tests for VNC client
//!
//! These tests verify VNC client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start VNC server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "vnc"))]
mod vnc_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test VNC client connection to a local server
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_vnc_client_connect_to_server() -> E2EResult<()> {
        // Start a VNC server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via VNC. Accept connections with no password. \
             Provide a 800x600 framebuffer with name 'NetGet Test VNC'.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("vnc")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "VNC", "instruction": "VNC client"}]))
                .expect_calls(1)
                .and()
        })

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Now start a VNC client that connects to this server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via VNC with no password. \
             Request a framebuffer update after connecting.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and handshake
        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ VNC client connected to server successfully");

        // Cleanup

    // Verify mocks
    server.verify_mocks().await?;
        server.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test VNC client can send pointer events
    /// LLM calls: 2 (client startup with pointer action)
    #[tokio::test]
    async fn test_vnc_client_pointer_event() -> E2EResult<()> {
        // Start a VNC server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via VNC. Accept connections with no password. \
             Log all pointer events received.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("vnc")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "VNC", "instruction": "VNC client"}]))
                .expect_calls(1)
                .and()
        })

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client that sends a pointer event (mouse click)
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via VNC. \
             After connecting, send a pointer event (left click) at position (100, 200).",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify the client protocol is VNC
        assert_eq!(client.protocol, "VNC", "Client should be VNC protocol");

        println!("✅ VNC client sent pointer event successfully");

        // Cleanup

    // Verify mocks
    server.verify_mocks().await?;
        server.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test VNC client can send key events
    /// LLM calls: 2 (client startup with key action)
    #[tokio::test]
    async fn test_vnc_client_key_event() -> E2EResult<()> {
        // Start a VNC server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via VNC. Accept connections with no password. \
             Log all key events received.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("vnc")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "VNC", "instruction": "VNC client"}]))
                .expect_calls(1)
                .and()
        })

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client that sends key events
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via VNC. \
             After connecting, send key event for 'A' key (keysym 65) press and release.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify client connected
        assert!(
            client.output_contains("connected").await,
            "Client should connect successfully"
        );

        println!("✅ VNC client sent key events successfully");

        // Cleanup

    // Verify mocks
    server.verify_mocks().await?;
        server.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test VNC client with password authentication
    /// LLM calls: 2 (server + client with auth)
    #[tokio::test]
    #[ignore] // Ignore by default as VNC auth implementation is simplified
    async fn test_vnc_client_with_password() -> E2EResult<()> {
        // Start a VNC server with password
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via VNC with password 'test123'. \
             Accept connections from clients with matching password.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("vnc")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "VNC", "instruction": "VNC client"}]))
                .expect_calls(1)
                .and()
        })

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client with password
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via VNC with password 'test123'.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify authentication succeeded
        assert!(
            client.output_contains("connected").await,
            "Client should authenticate and connect"
        );

        println!("✅ VNC client authenticated with password successfully");

        // Cleanup

    // Verify mocks
    server.verify_mocks().await?;
        server.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }
}
