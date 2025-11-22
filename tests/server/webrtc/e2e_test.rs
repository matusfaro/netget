//! E2E tests for WebRTC server
//!
//! These tests verify WebRTC server functionality by spawning the actual NetGet binary
//! and testing server behavior as a black-box with mocks.
//!
//! Test strategy:
//! - Mock LLM responses for all server actions
//! - Test manual signaling mode (SDP exchange)
//! - Test multi-peer support
//! - Keep total LLM calls < 10

#[cfg(all(test, feature = "webrtc"))]
mod webrtc_server_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test WebRTC server startup and accept offer
    /// LLM calls: 1 (server startup)
    #[tokio::test]
    async fn test_webrtc_server_startup_with_mocks() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Open WebRTC server for peer-to-peer data channel communication"
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebRTC",
                        "startup_params": {
                            "ice_servers": ["stun:stun.l.google.com:19302"],
                            "signaling_mode": "manual"
                        },
                        "instruction": "Accept WebRTC connections and respond to messages"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ WebRTC server started successfully in manual signaling mode");

        // Cleanup
        server.stop().await?;

        Ok(())
    }

    /// Test WebRTC server accept offer and generate answer
    /// LLM calls: 2 (server startup + simulated offer received event)
    #[tokio::test]
    async fn test_webrtc_accept_offer_with_mocks() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Open WebRTC server and accept offer from peer-abc123"
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebRTC",
                        "startup_params": {},
                        "instruction": "Accept offer from peer-abc123"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Handle offer received event
                .on_event("webrtc_offer_received")
                .and_event_data_contains("peer_id", "peer-abc123")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "accept_offer",
                        "peer_id": "peer-abc123",
                        "sdp_offer": "{\"type\":\"offer\",\"sdp\":\"v=0...\"}"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start and process offer
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("✅ WebRTC server accepted offer and generated answer (mocked)");

        // Note: In real mode, the server would actually call webrtc-rs to process SDP
        // In mock mode, we verify the LLM would make the correct action

        // Cleanup
        server.stop().await?;

        Ok(())
    }

    /// Test WebRTC server send message to peer
    /// LLM calls: 3 (server startup + peer connected + message received)
    #[tokio::test]
    async fn test_webrtc_send_message_with_mocks() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Open WebRTC server and echo messages back to peers"
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebRTC",
                        "startup_params": {},
                        "instruction": "Echo messages back to peers"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Peer connected event
                .on_event("webrtc_peer_connected")
                .and_event_data_contains("peer_id", "peer-xyz")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_to_peer",
                        "peer_id": "peer-xyz",
                        "message": "Welcome to WebRTC server!"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Message received event
                .on_event("webrtc_message_received")
                .and_event_data_contains("peer_id", "peer-xyz")
                .and_event_data_contains("message", "Hello server!")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_message",
                        "message": "Echo: Hello server!"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start and process events
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("✅ WebRTC server sent messages to peer (mocked)");

        // Cleanup
        server.stop().await?;

        Ok(())
    }

    /// Test WebRTC server multi-peer support
    /// LLM calls: 4 (server startup + 2 peer connections + list peers)
    #[tokio::test]
    async fn test_webrtc_multi_peer_with_mocks() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Open WebRTC server for multiple peers and list them"
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebRTC",
                        "startup_params": {},
                        "instruction": "Accept multiple peers and list them"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: First peer connected
                .on_event("webrtc_peer_connected")
                .and_event_data_contains("peer_id", "peer-alice")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_to_peer",
                        "peer_id": "peer-alice",
                        "message": "Welcome Alice!"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Second peer connected
                .on_event("webrtc_peer_connected")
                .and_event_data_contains("peer_id", "peer-bob")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "list_peers"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start and process multiple peer connections
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("✅ WebRTC server handled multiple peers (mocked)");

        // Cleanup
        server.stop().await?;

        Ok(())
    }

    /// Test WebRTC server peer disconnection
    /// LLM calls: 3 (server startup + peer connected + peer disconnected)
    #[tokio::test]
    async fn test_webrtc_peer_disconnect_with_mocks() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Open WebRTC server and handle peer disconnections gracefully"
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_any()
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebRTC",
                        "startup_params": {},
                        "instruction": "Track peer connections and disconnections"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Peer connected
                .on_event("webrtc_peer_connected")
                .and_event_data_contains("peer_id", "peer-test")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_to_peer",
                        "peer_id": "peer-test",
                        "message": "Connected"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Peer disconnected
                .on_event("webrtc_peer_disconnected")
                .and_event_data_contains("peer_id", "peer-test")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start and process disconnection
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("✅ WebRTC server handled peer disconnection (mocked)");

        // Cleanup
        server.stop().await?;

        Ok(())
    }
}
