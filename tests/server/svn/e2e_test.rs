#[cfg(all(test, feature = "svn"))]
mod svn_e2e_test {
    use netget::llm::ollama_client::OllamaClient;
    use netget::state::app_state::AppState;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    const TEST_MODEL: &str = "qwen2.5-coder:0.5b";

    async fn start_svn_server(instruction: &str) -> (Arc<AppState>, String) {
        let state = Arc::new(AppState::new());
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();

        // Use Ollama lock for concurrent test safety
        let ollama_lock = std::env::var("OLLAMA_LOCK_PATH").ok();
        let ollama_client = OllamaClient::new(
            "http://localhost:11434",
            TEST_MODEL,
            0.7,
            ollama_lock.as_deref(),
        )
        .expect("Failed to create Ollama client");

        // Open SVN server via user input
        let user_input = format!(
            "listen on port {{{{AVAILABLE_PORT}}}} via svn\n{}",
            instruction
        );
        state
            .handle_user_input(&user_input, &ollama_client, status_tx.clone())
            .await
            .expect("Failed to start server");

        // Wait for server to start and extract address
        let mut server_addr = None;
        let timeout = tokio::time::sleep(Duration::from_secs(10));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(msg) = status_rx.recv() => {
                    if msg.contains("listening on") {
                        // Extract address from message like "SVN server (action-based) listening on 127.0.0.1:12345"
                        if let Some(addr_start) = msg.rfind("127.0.0.1:") {
                            let addr_str = &msg[addr_start..];
                            if let Some(addr_end) = addr_str.find(|c: char| !c.is_ascii_digit() && c != '.' && c != ':') {
                                server_addr = Some(addr_str[..addr_end].to_string());
                            } else {
                                server_addr = Some(addr_str.to_string());
                            }
                            break;
                        }
                    }
                }
                _ = &mut timeout => {
                    panic!("Timeout waiting for SVN server to start");
                }
            }
        }

        let addr = server_addr.expect("Failed to extract server address");
        (state, addr)
    }

    async fn send_svn_command(addr: &str, command: &str) -> String {
        let mut stream = TcpStream::connect(addr)
            .await
            .expect("Failed to connect to SVN server");

        let mut reader = BufReader::new(&mut stream);

        // Read greeting from server
        let mut greeting = String::new();
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut greeting))
            .await
            .expect("Timeout reading greeting")
            .expect("Failed to read greeting");

        // Send command
        let command_with_newline = format!("{}\n", command.trim());
        stream
            .write_all(command_with_newline.as_bytes())
            .await
            .expect("Failed to send command");

        // Read response
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut response))
            .await
            .expect("Timeout reading response")
            .expect("Failed to read response");

        response
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_svn_greeting() {
        let instruction = r#"
When client connects, send protocol greeting with:
  - Protocol version 2 (min and max)
  - ANONYMOUS authentication mechanism
  - edit-pipeline and svndiff1 capabilities
"#;

        let (_state, addr) = start_svn_server(instruction).await;

        // Connect and read greeting
        let stream = TcpStream::connect(&addr).await.expect("Failed to connect");
        let mut reader = BufReader::new(stream);

        let mut greeting = String::new();
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut greeting))
            .await
            .expect("Timeout reading greeting")
            .expect("Failed to read greeting");

        println!("Received greeting: {}", greeting);

        assert!(
            greeting.contains("success") || greeting.contains("2"),
            "Greeting should contain success and version 2: {}",
            greeting
        );

        println!("✓ SVN greeting test passed");
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_svn_get_latest_rev() {
        let instruction = r#"
Send standard greeting on connect.
For get-latest-rev command, respond with revision number 42.
Use send_svn_success action with data: "42"
"#;

        let (_state, addr) = start_svn_server(instruction).await;

        let response = send_svn_command(&addr, "( get-latest-rev )").await;
        println!("Received response: {}", response);

        assert!(
            response.contains("success") || response.contains("42"),
            "Response should contain success or revision 42: {}",
            response
        );

        println!("✓ SVN get-latest-rev test passed");
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_svn_get_dir() {
        let instruction = r#"
Send standard greeting on connect.
For get-dir command, respond with directory listing containing:
  - trunk (dir)
  - branches (dir)
  - tags (dir)
Use send_svn_list action.
"#;

        let (_state, addr) = start_svn_server(instruction).await;

        let response = send_svn_command(&addr, "( get-dir )").await;
        println!("Received response: {}", response);

        assert!(
            response.contains("success") || response.contains("trunk") || response.contains("dir"),
            "Response should contain success or directory listing: {}",
            response
        );

        println!("✓ SVN get-dir test passed");
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_svn_error_response() {
        let instruction = r#"
Send standard greeting on connect.
For any command, respond with failure:
  - error code 210005
  - message "Path not found"
Use send_svn_failure action.
"#;

        let (_state, addr) = start_svn_server(instruction).await;

        let response = send_svn_command(&addr, "( stat /nonexistent )").await;
        println!("Received response: {}", response);

        assert!(
            response.contains("failure")
                || response.contains("error")
                || response.contains("not found"),
            "Response should indicate failure: {}",
            response
        );

        println!("✓ SVN error response test passed");
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_svn_connection_stats() {
        let instruction = "Send standard greeting. For any command, respond with success.";

        let (state, addr) = start_svn_server(instruction).await;

        // Send command
        let _response = send_svn_command(&addr, "( get-latest-rev )").await;

        // Give server time to update stats
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Verify connection was tracked
        let servers = state.list_servers().await;
        assert!(!servers.is_empty(), "Should have at least one server");

        let server = &servers[0];
        assert!(
            !server.connections.is_empty(),
            "Server should have tracked connections"
        );

        println!("✓ SVN connection stats test passed");
    }
}
