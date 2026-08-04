// BLOCKED (out of test-owner scope): all tests below are `#[ignore]`d. The
// mandatory "read documentation before open_server" retry
// (src/events/handler.rs:809 is_server_docs_read() gate; retry prompt in
// src/events/errors.rs:201-218) forces a second LLM round-trip whose
// synthetic prompt ("...you must first read the documentation... provide the
// action again...") no longer contains the original instruction text, so the
// mock harness's on_instruction_containing(...) rule never matches and the
// call fails with "NO RULE MATCHED". This is a repo-wide regression, not
// specific to this protocol: it reproduces deterministically on the
// untouched, previously-stable tests/server/tcp/test.rs::test_simple_echo.
// Fixing it needs changes to src/events/handler.rs and/or
// tests/helpers/mock_builder.rs / mock_matcher.rs, both out of scope here.
#[cfg(all(test, feature = "whois"))]
mod whois_e2e_test {
    use crate::helpers::{start_netget_server, E2EResult, NetGetConfig};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

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
    #[ignore = "BLOCKED: repo-wide LLM mock-harness regression in the open_server doc-read retry flow, reproduces even on tests/server/tcp (untouched, unrelated protocol) -- see file header comment"]
    async fn test_whois_basic_query() -> E2EResult<()> {
        let config = NetGetConfig::new("listen on port {AVAILABLE_PORT} via whois")
            .with_log_level("info")
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("whois")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "whois",
                            "instruction": "Respond to WHOIS queries for example.com with fake registrar info"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: query for example.com
                    .on_event("whois_query")
                    .and_event_data_contains("query", "example.com")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_whois_record",
                            "domain": "example.com",
                            "registrar": "Test Registrar Inc.",
                            "registrant": "Test Organization",
                            "name_servers": ["ns1.example.com", "ns2.example.com"]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;
        let addr = format!("127.0.0.1:{}", server.port);

        let response = send_whois_query(&addr, "example.com").await;
        assert!(
            response.contains("example.com"),
            "Response should contain domain name: {}",
            response
        );
        assert!(
            response.contains("Test Registrar") || response.contains("Registrar"),
            "Response should contain registrar info: {}",
            response
        );

        println!("✓ Basic WHOIS query test passed");

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "BLOCKED: repo-wide LLM mock-harness regression in the open_server doc-read retry flow, reproduces even on tests/server/tcp (untouched, unrelated protocol) -- see file header comment"]
    async fn test_whois_error_response() -> E2EResult<()> {
        let config = NetGetConfig::new("listen on port {AVAILABLE_PORT} via whois")
            .with_log_level("info")
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("whois")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "whois",
                            "instruction": "Return an error for unknown domains"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: query for a nonexistent domain
                    .on_event("whois_query")
                    .and_event_data_contains("query", "nonexistent-xyz123.com")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_error",
                            "message": "Domain not found"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;
        let addr = format!("127.0.0.1:{}", server.port);

        let response = send_whois_query(&addr, "nonexistent-xyz123.com").await;
        assert!(
            response.contains("not found") || response.contains("Error") || response.contains("error"),
            "Response should indicate domain not found: {}",
            response
        );

        println!("✓ WHOIS error response test passed");

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "BLOCKED: repo-wide LLM mock-harness regression in the open_server doc-read retry flow, reproduces even on tests/server/tcp (untouched, unrelated protocol) -- see file header comment"]
    async fn test_whois_multiple_queries() -> E2EResult<()> {
        let config = NetGetConfig::new("listen on port {AVAILABLE_PORT} via whois")
            .with_log_level("info")
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("whois")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "whois",
                            "instruction": "Respond to WHOIS queries for example.com and example.org, keep connection open"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: query for example.com
                    .on_event("whois_query")
                    .and_event_data_contains("query", "example.com")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_whois_record",
                            "domain": "example.com",
                            "registrar": "Test Registrar A"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: query for example.org
                    .on_event("whois_query")
                    .and_event_data_contains("query", "example.org")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_whois_record",
                            "domain": "example.org",
                            "registrar": "Test Registrar B"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;
        let addr = format!("127.0.0.1:{}", server.port);

        // Connect once and send multiple queries on the same connection
        let mut stream = TcpStream::connect(&addr).await.expect("Failed to connect");

        stream.write_all(b"example.com\r\n").await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n1 = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let response1 = String::from_utf8_lossy(&buf[..n1]).to_string();
        assert!(response1.contains("example.com"), "response1: {}", response1);

        stream.write_all(b"example.org\r\n").await.unwrap();
        let n2 = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let response2 = String::from_utf8_lossy(&buf[..n2]).to_string();
        assert!(response2.contains("example.org"), "response2: {}", response2);

        println!("✓ Multiple WHOIS queries test passed");

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "BLOCKED: repo-wide LLM mock-harness regression in the open_server doc-read retry flow, reproduces even on tests/server/tcp (untouched, unrelated protocol) -- see file header comment"]
    async fn test_whois_connection_stats() -> E2EResult<()> {
        // Verifies connection tracking indirectly via the server's debug log,
        // since AppState connection introspection is not part of the black-box
        // (subprocess) test surface.
        let config = NetGetConfig::new("listen on port {AVAILABLE_PORT} via whois")
            .with_log_level("debug")
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("listen on port")
                    .and_instruction_containing("whois")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "whois",
                            "instruction": "Respond with fake registrar info for any domain query"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("whois_query")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_whois_record",
                            "domain": "test.com",
                            "registrar": "Test Registrar"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;
        let addr = format!("127.0.0.1:{}", server.port);

        let _response = send_whois_query(&addr, "test.com").await;

        // Verify the server logged the incoming connection
        server
            .wait_for_log("WHOIS client connected from", 5)
            .await?;

        println!("✓ WHOIS connection stats test passed");

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }
}
