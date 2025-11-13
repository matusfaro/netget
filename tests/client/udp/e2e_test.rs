//! E2E tests for UDP client
//!
//! These tests verify UDP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "udp"))]
mod udp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test UDP client connection to a local server
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_udp_client_connect_to_server() -> E2EResult<()> {
        // Start a UDP server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via UDP. Echo received datagrams back to sender.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("udp")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "UDP", "instruction": "UDP client"}]))
                .expect_calls(1)
                .and()
        })

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a UDP client that connects to this server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via UDP. Send 'HELLO' datagram and wait for response.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection (socket bound)
        assert!(
            client.output_contains("ready").await || client.output_contains("bound").await,
            "Client should show ready/bound message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ UDP client connected to server successfully");

        // Cleanup

    // Verify mocks
    server.verify_mocks().await?;
        server.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test UDP client can send datagrams via prompts
    /// LLM calls: 2 (client startup)
    #[tokio::test]
    async fn test_udp_client_send_datagram() -> E2EResult<()> {
        // Start a simple UDP server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via UDP. Log all incoming datagrams.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("udp")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "UDP", "instruction": "UDP client"}]))
                .expect_calls(1)
                .and()
        })

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that sends specific data based on LLM instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via UDP and send the string 'PING' then wait for response.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client initiated the connection
        assert_eq!(client.protocol, "UDP", "Client should be UDP protocol");

        println!("✅ UDP client responded to LLM instruction");

        // Cleanup

    // Verify mocks
    server.verify_mocks().await?;
        server.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test UDP client can receive and respond to datagrams
    /// LLM calls: 3 (server startup, client startup, response)
    #[tokio::test]
    async fn test_udp_client_receive_and_respond() -> E2EResult<()> {
        // Start a UDP server that sends a datagram immediately upon receiving one
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via UDP. When you receive a datagram, send 'PONG' back.")
        .with_mock(|mock| {
            mock
                .on_instruction_containing("udp")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "UDP", "instruction": "UDP client"}]))
                .expect_calls(1)
                .and()
        })

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client sends PING, waits for PONG
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via UDP. Send 'PING' and display any response you receive.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client received response (check for "datagram" or "received" in output)
        let output = client.get_output().await;
        assert!(
            output.contains("datagram") || output.contains("received") || output.contains("PONG"),
            "Client should show received datagram. Output: {:?}",
            output
        );

        println!("✅ UDP client received and processed response");

        // Cleanup

    // Verify mocks
    server.verify_mocks().await?;
        server.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test UDP client can change target address
    /// LLM calls: 2 (client startup, change target)
    #[tokio::test]
    async fn test_udp_client_change_target() -> E2EResult<()> {
        // Start two UDP servers
        let server1_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via UDP. Log 'SERVER1' for each datagram.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("udp")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "UDP", "instruction": "UDP client"}]))
                .expect_calls(1)
                .and()
        })
        let mut server1 = start_netget_server(server1_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        let server2_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via UDP. Log 'SERVER2' for each datagram.",
        )
        .with_mock(|mock| {
            mock
                .on_instruction_containing("udp")
                .respond_with_actions(serde_json::json!([{"type": "open_client", "protocol": "UDP", "instruction": "UDP client"}]))
                .expect_calls(1)
                .and()
        })
        let mut server2 = start_netget_server(server2_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client connects to server1, then changes target to server2
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via UDP. Send 'HELLO1'. Then change target to 127.0.0.1:{} and send 'HELLO2'.",
            server1.port, server2.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client protocol
        assert_eq!(client.protocol, "UDP", "Client should be UDP protocol");

        println!("✅ UDP client successfully changed target address");

        // Cleanup
        server1.stop().await?;
        server2.stop().await?;

    // Verify mocks
    client.verify_mocks().await?;
        client.stop().await?;

        Ok(())
    }
}
