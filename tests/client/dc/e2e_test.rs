//! E2E tests for DC (Direct Connect) client
//!
//! These tests verify DC client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "dc"))]
mod dc_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test DC client connection and authentication with mocks
    /// LLM calls: 6 (server startup, server welcome, client startup, connection, auth, first message)
    #[tokio::test]
    async fn test_dc_client_connect_and_auth_with_mocks() -> E2EResult<()> {
        // Start a DC server listening on an available port with mocks
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via DC. Accept all connections.")
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Listen on port")
                    .and_instruction_containing("via DC")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "DC",
                            "instruction": "Accept all connections and welcome users"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: DC connection received
                    .on_event("dc_connection")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_lock",
                            "lock": "EXTENDEDPROTOCOLABCABCABCABCABCABC",
                            "pk": "TestHub"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: ValidateNick received
                    .on_event("dc_validate_nick")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_hello"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a DC client that connects
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via DC as 'testuser'. Say hello after connecting.",
            server.port
        ))
            .with_startup_params(serde_json::json!({
                "nickname": "testuser"
            }))
            .with_mock(|mock| {
                mock
                    // Mock 1: Client startup
                    .on_instruction_containing("Connect to")
                    .and_instruction_containing("DC")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": format!("127.0.0.1:{}", server.port),
                            "protocol": "DC",
                            "instruction": "Authenticate and say hello",
                            "startup_params": {
                                "nickname": "testuser"
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: DC connected event (received Lock)
                    .on_event("dc_client_connected")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: DC authenticated event (received Hello)
                    .on_event("dc_client_authenticated")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_dc_chat",
                            "message": "Hello everyone!"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        // Give client time to connect and authenticate
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ DC client connected and authenticated successfully");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test DC client can send chat messages with mocks
    /// LLM calls: 7 (server startup, connection, validate_nick, client startup, connected, auth, chat received)
    #[tokio::test]
    async fn test_dc_client_send_chat_with_mocks() -> E2EResult<()> {
        // Start a DC server with mocks
        let server_config =
            NetGetConfig::new("Listen on port {AVAILABLE_PORT} via DC. Echo all chat messages.")
                .with_mock(|mock| {
                    mock
                        .on_instruction_containing("Listen on port")
                        .and_instruction_containing("via DC")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "DC",
                                "instruction": "Echo chat messages"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        .on_event("dc_connection")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "send_lock",
                                "lock": "EXTENDEDPROTOCOLABCABCABCABCABCABC",
                                "pk": "EchoHub"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        .on_event("dc_validate_nick")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "send_hello"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        .on_event("dc_chat")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "broadcast_chat",
                                "source": "server",
                                "message": "Message received!"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                });

        let server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that sends a chat message
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via DC as 'chatter'. Send 'Hello Hub!' in chat.",
            server.port
        ))
            .with_startup_params(serde_json::json!({
                "nickname": "chatter"
            }))
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("DC")
                    .and_instruction_containing("chatter")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": format!("127.0.0.1:{}", server.port),
                            "protocol": "DC",
                            "instruction": "Send hello message",
                            "startup_params": {
                                "nickname": "chatter"
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_client_connected")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_client_authenticated")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_dc_chat",
                            "message": "Hello Hub!"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_client_message_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify client authenticated
        assert!(
            client.output_contains("authenticated").await
                || client.output_contains("connected").await,
            "Client should show authentication. Output: {:?}",
            client.get_output().await
        );

        println!("✅ DC client sent chat message successfully");

        // Verify mock expectations
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test DC client can request user list with mocks
    /// LLM calls: 7 (similar to chat test)
    #[tokio::test]
    async fn test_dc_client_request_userlist_with_mocks() -> E2EResult<()> {
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via DC. Provide user list on request.")
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("Listen on port")
                    .and_instruction_containing("via DC")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "DC",
                            "instruction": "Provide user list"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_connection")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_lock",
                            "lock": "EXTENDEDPROTOCOLABCABCABCABCABCABC",
                            "pk": "ListHub"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_validate_nick")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_hello"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_get_nicklist")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_nicklist",
                            "users": ["alice", "bob", "charlie"]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via DC as 'lister'. Request user list after connecting.",
            server.port
        ))
            .with_startup_params(serde_json::json!({
                "nickname": "lister"
            }))
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("DC")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_client",
                            "remote_addr": format!("127.0.0.1:{}", server.port),
                            "protocol": "DC",
                            "instruction": "Request user list",
                            "startup_params": {
                                "nickname": "lister"
                            }
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_client_connected")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_client_authenticated")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_dc_get_nicklist"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("dc_client_userlist_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "wait_for_more"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let client = start_netget_client(client_config).await?;
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("✅ DC client requested and received user list");

        server.verify_mocks().await?;
        client.verify_mocks().await?;

        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test DC client connection and authentication (real LLM, ignored by default)
    /// LLM calls: 4-6 (server/client startup, connection events, auth)
    #[tokio::test]
    #[ignore]
    async fn test_dc_client_connect_real_llm() -> E2EResult<()> {
        // Start a DC server
        let server_config =
            NetGetConfig::new("Listen on port {AVAILABLE_PORT} via DC. Accept all connections.");

        let server = start_netget_server(server_config).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start a DC client
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via DC as 'testbot'. Say 'Hello from NetGet!' after connecting.",
            server.port
        ))
        .with_startup_params(serde_json::json!({
            "nickname": "testbot",
            "description": "NetGet Test Bot"
        }));

        let client = start_netget_client(client_config).await?;
        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify connection
        assert!(
            client.output_contains("connected").await,
            "Client should connect. Output: {:?}",
            client.get_output().await
        );

        println!("✅ DC client connected with real LLM");

        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
