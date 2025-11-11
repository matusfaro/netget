#[cfg(all(test, feature = "whois"))]
mod whois_e2e_test {
    use netget::llm::ollama_client::OllamaClient;
    use netget::state::app_state::AppState;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const TEST_MODEL: &str = "qwen2.5-coder:0.5b";

    async fn start_whois_server(instruction: &str) -> (Arc<AppState>, String) {
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

        // Open WHOIS server via user input
        let user_input = format!(
            "listen on port {{{{AVAILABLE_PORT}}}} via whois\n{}",
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
                        // Extract address from message like "WHOIS server (action-based) listening on 127.0.0.1:12345"
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
                    panic!("Timeout waiting for WHOIS server to start");
                }
            }
        }

        let addr = server_addr.expect("Failed to extract server address");
        (state, addr)
    }

    async fn send_whois_query(addr: &str, query: &str) -> String {
        let mut stream = TcpStream::connect(addr)
            .await
            .expect("Failed to connect to WHOIS server");

        // Send query with CRLF
        let query_with_crlf = format!("{}\r\n", query.trim());
        stream
            .write_all(query_with_crlf.as_bytes())
            .await
            .expect("Failed to send query");

        // Read response (up to 4KB)
        let mut response = vec![0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut response))
            .await
            .expect("Timeout reading response")
            .expect("Failed to read response");

        String::from_utf8_lossy(&response[..n]).to_string()
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_whois_basic_query() {
        let instruction = r#"
For example.com, respond with:
  Domain Name: example.com
  Registrar: Test Registrar Inc.
  Registrant Name: Test Organization
  Admin Name: admin@example.com
  Name Server: ns1.example.com
  Name Server: ns2.example.com

For any other domain, return "Domain not found" error.
Keep connections open for multiple queries.
"#;

        let (_state, addr) = start_whois_server(instruction).await;

        // Test successful query
        let response = send_whois_query(&addr, "example.com").await;
        assert!(
            response.contains("example.com"),
            "Response should contain domain name"
        );
        assert!(
            response.contains("Test Registrar") || response.contains("Registrar"),
            "Response should contain registrar info"
        );

        println!("✓ Basic WHOIS query test passed");
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_whois_error_response() {
        let instruction = r#"
For example.com, respond with fake registrar info.
For any other domain, return "Domain not found" error.
"#;

        let (_state, addr) = start_whois_server(instruction).await;

        // Test error response
        let response = send_whois_query(&addr, "nonexistent-xyz123.com").await;
        assert!(
            response.contains("not found")
                || response.contains("Error")
                || response.contains("error"),
            "Response should indicate domain not found: {}",
            response
        );

        println!("✓ WHOIS error response test passed");
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_whois_multiple_queries() {
        let instruction = r#"
For example.com: respond with registrar "Test Registrar A"
For example.org: respond with registrar "Test Registrar B"
Keep connections open for multiple queries.
"#;

        let (_state, addr) = start_whois_server(instruction).await;

        // Connect once and send multiple queries
        let mut stream = TcpStream::connect(&addr).await.expect("Failed to connect");

        // First query
        stream.write_all(b"example.com\r\n").await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n1 = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let response1 = String::from_utf8_lossy(&buf[..n1]);
        assert!(response1.contains("example.com"));

        // Second query on same connection
        stream.write_all(b"example.org\r\n").await.unwrap();
        let n2 = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let response2 = String::from_utf8_lossy(&buf[..n2]);
        assert!(response2.contains("example.org"));

        println!("✓ Multiple WHOIS queries test passed");
    }

    #[tokio::test]
    #[ignore = "requires ollama"]
    async fn test_whois_connection_stats() {
        let instruction = "Respond with fake registrar info for any domain query.";

        let (state, addr) = start_whois_server(instruction).await;

        // Send query
        let _response = send_whois_query(&addr, "test.com").await;

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

        println!("✓ WHOIS connection stats test passed");
    }
}
