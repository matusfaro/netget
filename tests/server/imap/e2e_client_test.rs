//! E2E tests for IMAP server using real async-imap client
//!
//! These tests verify IMAP protocol implementation by:
//! - Starting NetGet in non-interactive mode with IMAP prompts
//! - Using the async-imap Rust client library (same used by email clients)
//! - Testing realistic email client operations
//!
//! This complements the raw TCP tests in tests/server/imap/test.rs
//! by using an actual IMAP client implementation.

#[cfg(all(test, feature = "imap", feature = "imap"))]
mod e2e_imap_client {
    use crate::server::helpers::*;
    use futures::StreamExt;
    // For collecting Streams
    use tokio::net::TcpStream;
    use tokio_util::compat::TokioAsyncReadCompatExt;

    /// Helper to create an IMAP client connected to the server
    async fn connect_imap_client(
        port: u16,
    ) -> E2EResult<async_imap::Client<tokio_util::compat::Compat<TcpStream>>> {
        let addr = format!("127.0.0.1:{}", port);
        let tcp_stream = TcpStream::connect(&addr).await?;
        // Convert from tokio::io to futures::io using compat layer
        let compat_stream = tcp_stream.compat();
        let client = async_imap::Client::new(compat_stream);

        // Note: Don't read greeting here - Client::new does it automatically

        Ok(client)
    }

    #[tokio::test]
    async fn test_imap_login_success() -> E2EResult<()> {
        println!("\n=== Test: IMAP Login Success with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for username 'alice' with password 'secret123'. \
                     Greet with: * OK IMAP4rev1 NetGet Server Ready";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect using async-imap client
        let client = connect_imap_client(server.port).await?;
        println!("  [TEST] Connected to IMAP server");

        // Attempt login - handle tuple error type
        let mut session = match client.login("alice", "secret123").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Login successful");

        // Logout
        session
            .logout()
            .await
            .map_err(|e| format!("Logout failed: {}", e))?;
        println!("  [TEST] ✓ Logout successful");

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_login_failure() -> E2EResult<()> {
        println!("\n=== Test: IMAP Login Failure with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Only allow LOGIN for username 'alice' with password 'secret123'. \
                     Deny all other credentials.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect using async-imap client
        let client = connect_imap_client(server.port).await?;
        println!("  [TEST] Connected to IMAP server");

        // Attempt login with wrong password - should return Err tuple
        let result = client.login("alice", "wrongpassword").await;

        // Should fail
        match result {
            Err((err, _client)) => {
                println!("  [TEST] ✓ Login correctly rejected: {}", err);
            }
            Ok(_) => {
                return Err("Login should have failed with wrong password".into());
            }
        }

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_list_mailboxes() -> E2EResult<()> {
        println!("\n=== Test: IMAP LIST Mailboxes with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user. \
                     When listing mailboxes, return: INBOX, Sent, Drafts, Trash.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // List mailboxes - collect Stream into Vec
        let mailboxes: Vec<_> = session
            .list(Some(""), Some("*"))
            .await?
            .collect::<Vec<_>>()
            .await;
        println!("  [TEST] Found {} mailboxes", mailboxes.len());

        // Verify we got at least INBOX - handle Result values
        let mailbox_names: Vec<String> = mailboxes
            .into_iter()
            .filter_map(|mb_result| mb_result.ok())
            .map(|mb| mb.name().to_string())
            .collect();

        println!("  [TEST] Mailboxes: {:?}", mailbox_names);
        assert!(
            !mailbox_names.is_empty(),
            "Should have at least one mailbox"
        );
        println!("  [TEST] ✓ LIST command successful");

        session.logout().await?;
        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_select_mailbox() -> E2EResult<()> {
        println!("\n=== Test: IMAP SELECT Mailbox with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user. \
                     INBOX has 5 messages, 2 are recent. \
                     First unseen message is #3.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // Select INBOX
        let mailbox = session.select("INBOX").await?;
        println!("  [TEST] Selected INBOX");
        println!("  [TEST]   EXISTS: {}", mailbox.exists);
        println!("  [TEST]   RECENT: {:?}", mailbox.recent);
        println!("  [TEST]   UNSEEN: {:?}", mailbox.unseen);

        // Verify mailbox info - exists is u32, not Option
        assert!(mailbox.exists > 0, "Should have messages in INBOX");
        println!("  [TEST] ✓ SELECT command successful");

        session.logout().await?;
        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_fetch_messages() -> E2EResult<()> {
        println!("\n=== Test: IMAP FETCH Messages with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user. \
                     INBOX has 3 messages: \
                     1. From: alice@example.com, Subject: Hello, Body: Test message 1 \
                     2. From: bob@example.com, Subject: Meeting, Body: Test message 2 \
                     3. From: charlie@example.com, Subject: Report, Body: Test message 3";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // Select INBOX
        session.select("INBOX").await?;
        println!("  [TEST] ✓ Selected INBOX");

        // Fetch message 1 - collect Stream into Vec
        let messages: Vec<_> = session
            .fetch("1", "RFC822")
            .await?
            .collect::<Vec<_>>()
            .await;
        println!("  [TEST] Fetched {} message(s)", messages.len());

        assert!(!messages.is_empty(), "Should fetch at least one message");

        // Handle Result values from Stream
        if let Some(Ok(msg)) = messages.into_iter().next() {
            println!("  [TEST]   Message UID: {:?}", msg.uid);
            if let Some(body) = msg.body() {
                let body_str = String::from_utf8_lossy(body);
                println!(
                    "  [TEST]   Body preview: {}...",
                    body_str.chars().take(50).collect::<String>()
                );
            }
        }
        println!("  [TEST] ✓ FETCH command successful");

        session.logout().await?;
        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_search_messages() -> E2EResult<()> {
        println!("\n=== Test: IMAP SEARCH Messages with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user. \
                     INBOX has 5 messages. Messages 1, 3, 5 are from alice@example.com. \
                     When searching FROM alice, return message numbers 1, 3, 5.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // Select INBOX
        session.select("INBOX").await?;
        println!("  [TEST] ✓ Selected INBOX");

        // Search for messages from alice
        let message_ids = session.search("FROM alice@example.com").await?;
        println!("  [TEST] Search found {} message(s)", message_ids.len());
        println!("  [TEST]   Message IDs: {:?}", message_ids);

        // Verify we got results
        assert!(
            !message_ids.is_empty(),
            "Search should return at least one message"
        );
        println!("  [TEST] ✓ SEARCH command successful");

        session.logout().await?;
        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_capability() -> E2EResult<()> {
        println!("\n=== Test: IMAP CAPABILITY with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Support IMAP4rev1, IDLE, NAMESPACE capabilities. \
                     Allow LOGIN for any user.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        println!("  [TEST] Connected to IMAP server");

        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // Get capabilities after login
        let caps = session.capabilities().await?;
        println!("  [TEST] Retrieved capabilities from server");

        // Verify IMAP4rev1 is supported
        assert!(
            caps.has_str("IMAP4rev1") || caps.has_str("IMAP4REV1"),
            "Server should support IMAP4rev1"
        );
        println!("  [TEST] ✓ CAPABILITY command successful");

        session.logout().await?;
        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_examine_readonly() -> E2EResult<()> {
        println!("\n=== Test: IMAP EXAMINE (readonly) with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user. \
                     INBOX has 10 messages. \
                     Support EXAMINE command for read-only access.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // EXAMINE INBOX (read-only)
        let mailbox = session.examine("INBOX").await?;
        println!("  [TEST] Examined INBOX (read-only)");
        println!("  [TEST]   EXISTS: {}", mailbox.exists);
        println!("  [TEST]   FLAGS: {:?}", mailbox.flags);

        assert!(mailbox.exists > 0, "Should have messages in INBOX");
        println!("  [TEST] ✓ EXAMINE command successful");

        session.logout().await?;
        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_status_command() -> E2EResult<()> {
        println!("\n=== Test: IMAP STATUS Command with async-imap ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user. \
                     Mailbox 'Sent' has 20 messages, 5 unseen. \
                     Support STATUS command.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // Get status of Sent mailbox without selecting it
        let status = session.status("Sent", "(MESSAGES UNSEEN)").await?;
        println!("  [TEST] STATUS for 'Sent' mailbox:");
        println!("  [TEST]   EXISTS: {}", status.exists);
        println!("  [TEST]   UNSEEN: {:?}", status.unseen);

        // Verify we got status info - exists is always present (u32)
        assert!(
            status.exists > 0 || status.unseen.is_some(),
            "Should have at least one status attribute"
        );
        println!("  [TEST] ✓ STATUS command successful");

        session.logout().await?;
        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_concurrent_connections() -> E2EResult<()> {
        println!("\n=== Test: Multiple Concurrent IMAP Connections ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user with any password. \
                     INBOX has 5 messages. \
                     Support multiple concurrent connections.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Create 3 concurrent clients
        let port = server.port;
        let handles: Vec<_> = (0..3)
            .map(|i| {
                tokio::spawn(async move {
                    let client = connect_imap_client(port)
                        .await
                        .map_err(|e| format!("Connect failed: {}", e))?;

                    let mut session = match client.login(&format!("user{}", i), "password").await {
                        Ok(s) => s,
                        Err((err, _)) => return Err(format!("Login failed: {}", err).into()),
                    };

                    // Each client selects INBOX
                    let mailbox = session
                        .select("INBOX")
                        .await
                        .map_err(|e| format!("Select failed: {}", e))?;
                    println!(
                        "  [TEST] Client {} selected INBOX with {} messages",
                        i, mailbox.exists
                    );

                    session
                        .logout()
                        .await
                        .map_err(|e| format!("Logout failed: {}", e))?;
                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                })
            })
            .collect();

        // Wait for all clients to complete
        for (i, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(())) => println!("  [TEST] ✓ Client {} completed successfully", i),
                Ok(Err(e)) => return Err(format!("Client {} failed: {}", i, e).into()),
                Err(e) => return Err(format!("Client {} join error: {}", i, e).into()),
            }
        }

        println!("  [TEST] ✓ All concurrent connections successful");

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_imap_noop_and_logout() -> E2EResult<()> {
        println!("\n=== Test: IMAP NOOP and LOGOUT Commands ===");

        let prompt = "listen on port 0 via imap. \
                     Allow LOGIN for any user. \
                     Support NOOP command.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;
        println!("  [TEST] Server started on port {}", server.port);

        // Connect and login
        let client = connect_imap_client(server.port).await?;
        let mut session = match client.login("testuser", "testpass").await {
            Ok(session) => session,
            Err((err, _client)) => return Err(format!("Login failed: {}", err).into()),
        };
        println!("  [TEST] ✓ Logged in");

        // Send NOOP (no operation - keeps connection alive)
        session.noop().await?;
        println!("  [TEST] ✓ NOOP command successful");

        // Send another NOOP
        session.noop().await?;
        println!("  [TEST] ✓ Second NOOP successful");

        // Logout
        session.logout().await?;
        println!("  [TEST] ✓ LOGOUT successful");

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }
}
