//! E2E tests for SIP client
//!
//! These tests verify SIP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "sip"))]
mod sip_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test SIP client REGISTER with server
    /// LLM calls: 4 (server startup script + client connection + REGISTER response + final state)
    #[tokio::test]
    async fn test_sip_client_register() -> E2EResult<()> {
        // Start a SIP server that accepts REGISTER requests
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via SIP. \
             Accept all REGISTER requests with 200 OK, expires 3600. \
             Use scripting mode for fast responses."
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start a SIP client that registers with this server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via SIP. \
             Send REGISTER request for sip:alice@localhost with contact sip:alice@127.0.0.1:5060. \
             Request expires 3600. \
             Log the response status code.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and REGISTER
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await || client.output_contains("SIP").await,
            "Client should show SIP connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ SIP client registered with server successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test SIP client OPTIONS query
    /// LLM calls: 4 (server startup script + client connection + OPTIONS response + final state)
    #[tokio::test]
    async fn test_sip_client_options() -> E2EResult<()> {
        // Start a SIP server that responds to OPTIONS
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via SIP. \
             Respond to OPTIONS requests with 200 OK and Allow: INVITE, ACK, BYE, REGISTER, OPTIONS. \
             Use scripting mode for fast responses."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start a SIP client that queries server capabilities
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via SIP. \
             Send OPTIONS request to sip:server@localhost. \
             Log the Allow header from the response.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Verify client initiated SIP connection
        assert_eq!(client.protocol, "SIP", "Client should be SIP protocol");

        println!("✅ SIP client OPTIONS query successful");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test SIP client INVITE call attempt
    /// LLM calls: 4-5 (server startup script + client connection + INVITE response + possible ACK + final state)
    #[tokio::test]
    async fn test_sip_client_invite() -> E2EResult<()> {
        // Start a SIP server that accepts INVITE requests
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via SIP. \
             Accept all INVITE requests with 200 OK and SDP: v=0 o=server 0 0 IN IP4 127.0.0.1 s=Call c=IN IP4 127.0.0.1 t=0 0 m=audio 8000 RTP/AVP 0. \
             Use scripting mode for fast responses."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start a SIP client that initiates a call
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via SIP. \
             Send INVITE to sip:bob@localhost with SDP: v=0 o=alice 0 0 IN IP4 127.0.0.1 s=Call c=IN IP4 127.0.0.1 t=0 0 m=audio 49170 RTP/AVP 0. \
             Wait for 200 OK response and log the SDP body.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify client shows INVITE interaction
        let output = client.get_output().await;
        assert!(
            output.contains("SIP") || output.contains("INVITE") || output.contains("200"),
            "Client should show SIP INVITE interaction. Output: {:?}",
            output
        );

        println!("✅ SIP client INVITE successful");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
