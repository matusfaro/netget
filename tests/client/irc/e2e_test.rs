//! E2E tests for IRC client
//!
//! These tests verify IRC client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start IRC server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "irc"))]
mod irc_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test IRC client connection and registration
    /// LLM calls: 3 (server startup, client connection, connected event)
    #[tokio::test]
    async fn test_irc_client_connect_and_register() -> E2EResult<()> {
        // Start an IRC server listening on an available port with mocks
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IRC. Accept client connections and log all messages."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("IRC")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "IRC",
                        "instruction": "Accept IRC clients and log messages"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: IRC client sends NICK
                .on_event("irc_data_received")
                .and_event_data_contains("data", "NICK")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: IRC client sends USER
                .on_event("irc_data_received")
                .and_event_data_contains("data", "USER")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_irc_message",
                        "message": ":testserver 001 testbot :Welcome to the IRC Network"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start an IRC client that connects to this server with mocks
        let client_config = NetGetConfig::new(format!(
            "Connect to IRC at 127.0.0.1:{} with nickname testbot, wait for registration to complete.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect to IRC")
                .and_instruction_containing("testbot")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "IRC",
                        "instruction": "Register with nickname testbot"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected event
                .on_event("irc_client_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "register",
                        "nickname": "testbot",
                        "username": "testbot",
                        "realname": "Test Bot"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Client receives welcome (001)
                .on_event("irc_client_message_received")
                .and_event_data_contains("message", "001")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and register
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await
                || client.output_contains("registration").await,
            "Client should show connection or registration message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ IRC client connected and registered successfully");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test IRC client can join channel and send message
    /// LLM calls: 4 (server startup, client connection, connected event, message sending)
    #[tokio::test]
    async fn test_irc_client_join_and_message() -> E2EResult<()> {
        // Start an IRC server with mocks
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IRC. Accept all channel joins and log PRIVMSG commands."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("IRC")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "IRC",
                        "instruction": "Accept channel joins and log PRIVMSG"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: NICK command
                .on_event("irc_data_received")
                .and_event_data_contains("data", "NICK")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: USER command
                .on_event("irc_data_received")
                .and_event_data_contains("data", "USER")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_irc_message",
                        "message": ":testserver 001 testbot :Welcome"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: JOIN command
                .on_event("irc_data_received")
                .and_event_data_contains("data", "JOIN")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_irc_message",
                        "message": ":testbot JOIN #test"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 5: PRIVMSG command
                .on_event("irc_data_received")
                .and_event_data_contains("data", "PRIVMSG")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that joins a channel and sends a message with mocks
        let client_config = NetGetConfig::new(format!(
            "Connect to IRC at 127.0.0.1:{} with nickname testbot. After connecting, join #test and say 'Hello, channel!'",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect to IRC")
                .and_instruction_containing("testbot")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "IRC",
                        "instruction": "Register, join #test, and send message"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected
                .on_event("irc_client_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "register",
                        "nickname": "testbot",
                        "username": "testbot",
                        "realname": "Test Bot"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Client receives welcome (001)
                .on_event("irc_client_message_received")
                .and_event_data_contains("message", "001")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "join_channel",
                        "channel": "#test"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: Client receives JOIN confirmation
                .on_event("irc_client_message_received")
                .and_event_data_contains("message", "JOIN")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_privmsg",
                        "target": "#test",
                        "message": "Hello, channel!"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect, register, join, and send message
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify the client is IRC protocol
        assert_eq!(client.protocol, "IRC", "Client should be IRC protocol");

        println!("✅ IRC client joined channel and sent message");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test IRC client responds to server messages
    /// LLM calls: 5 (server startup, client connection, connected event, server message, client response)
    #[tokio::test]
    async fn test_irc_client_responds_to_messages() -> E2EResult<()> {
        // Start an IRC server that will send a message to the client with mocks
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IRC. When a client joins #bot, send them a PRIVMSG saying 'Welcome bot!'"
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("IRC")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "IRC",
                        "instruction": "Send welcome PRIVMSG when client joins #bot"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: NICK command
                .on_event("irc_data_received")
                .and_event_data_contains("data", "NICK")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: USER command
                .on_event("irc_data_received")
                .and_event_data_contains("data", "USER")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_irc_message",
                        "message": ":testserver 001 responsebot :Welcome"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: JOIN #bot command
                .on_event("irc_data_received")
                .and_event_data_contains("data", "JOIN")
                .and_event_data_contains("data", "#bot")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_irc_message",
                        "message": ":responsebot JOIN #bot"
                    },
                    {
                        "type": "send_irc_message",
                        "message": ":testserver PRIVMSG responsebot :Welcome bot!"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 5: Client responds with PRIVMSG Thanks
                .on_event("irc_data_received")
                .and_event_data_contains("data", "PRIVMSG")
                .and_event_data_contains("data", "Thanks")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that joins and responds to messages with mocks
        let client_config = NetGetConfig::new(format!(
            "Connect to IRC at 127.0.0.1:{} with nickname responsebot. Join #bot and respond to any messages with 'Thanks!'",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect to IRC")
                .and_instruction_containing("responsebot")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "IRC",
                        "instruction": "Join #bot and respond to messages"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected
                .on_event("irc_client_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "register",
                        "nickname": "responsebot",
                        "username": "responsebot",
                        "realname": "Response Bot"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Client receives welcome (001)
                .on_event("irc_client_message_received")
                .and_event_data_contains("message", "001")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "join_channel",
                        "channel": "#bot"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: Client receives JOIN confirmation
                .on_event("irc_client_message_received")
                .and_event_data_contains("message", "JOIN")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 5: Client receives PRIVMSG "Welcome bot!"
                .on_event("irc_client_message_received")
                .and_event_data_contains("message", "PRIVMSG")
                .and_event_data_contains("message", "Welcome bot!")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_privmsg",
                        "target": "#bot",
                        "message": "Thanks!"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give time for connection, join, server message, and client response
        tokio::time::sleep(Duration::from_secs(4)).await;

        // Verify client received and processed messages
        let output = client.get_output().await;
        assert!(
            output.contains("PRIVMSG") || output.contains("message"),
            "Client should show received message. Output: {:?}",
            output
        );

        println!("✅ IRC client responded to server messages");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
