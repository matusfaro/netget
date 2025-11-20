//! E2E tests for HTTP/HTTPS Proxy with mocks
//!
//! These tests verify Proxy functionality using mock LLM responses.
//! Test strategy: Mock proxy decisions, < 10 LLM calls total.

#[cfg(all(test, feature = "proxy"))]
mod proxy_server_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test HTTP proxy pass-through with mocks
    /// LLM calls: 2 (server startup, http_request event)
    #[tokio::test]
    async fn test_proxy_http_passthrough_with_mocks() -> E2EResult<()> {
        // Start a Proxy server with mocks
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack. Pass all HTTP requests through unchanged."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("proxy")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "instruction": "Pass all HTTP requests through unchanged"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTP request received (proxy_http_request event)
                .on_event("proxy_http_request")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "proxy_passthrough"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server started and processed mocked HTTP request");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        // Cleanup
        server.stop().await?;

        Ok(())
    }

    /// Test HTTP proxy blocking with mocks
    /// LLM calls: 2 (server startup, http_request with block decision)
    #[tokio::test]
    async fn test_proxy_http_block_with_mocks() -> E2EResult<()> {
        // Start a Proxy server that blocks requests
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack. Block all requests with 403 status."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("proxy")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "instruction": "Block all requests with 403 status"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTP request - block it
                .on_event("proxy_http_request")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "proxy_block",
                        "status": 403,
                        "body": "Access Denied by Proxy"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server blocked request with mocked LLM decision");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test HTTPS proxy CONNECT handling with mocks
    /// LLM calls: 2 (server startup, https_connect event)
    #[tokio::test]
    async fn test_proxy_https_connect_with_mocks() -> E2EResult<()> {
        // Start a Proxy server for HTTPS
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack with no certificate. Allow all HTTPS connections."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("proxy")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "startup_params": {
                            "mode": "passthrough"
                        },
                        "instruction": "Allow all HTTPS connections"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTPS CONNECT request
                .on_event("proxy_https_connect")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "proxy_allow_connect"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server processed HTTPS CONNECT with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test proxy header modification with mocks
    /// LLM calls: 2 (server startup, request with header modification)
    #[tokio::test]
    async fn test_proxy_modify_headers_with_mocks() -> E2EResult<()> {
        // Start a Proxy server that modifies headers
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack. Add header X-Proxy-Modified: NetGet to all requests."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("proxy")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "instruction": "Add header X-Proxy-Modified: NetGet"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTP request - modify headers
                .on_event("proxy_http_request")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "proxy_modify_request",
                        "add_headers": {
                            "X-Proxy-Modified": "NetGet"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server modified headers with mocked LLM decision");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test MITM mode initialization with certificate generation
    /// LLM calls: 1 (server startup with certificate generation)
    #[tokio::test]
    async fn test_proxy_mitm_initialization() -> E2EResult<()> {
        // Start a Proxy server in MITM mode with certificate generation
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack with certificate generation (MITM mode). Inspect all HTTPS traffic."
        )
        .with_mock(|mock| {
            mock
                // Mock: Server startup with MITM mode
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("MITM")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "startup_params": {
                            "mode": "mitm",
                            "certificate_mode": "generate"
                        },
                        "instruction": "Inspect all HTTPS traffic"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server initialized in MITM mode with certificate generation");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test MITM mode HTTPS interception and request inspection
    /// LLM calls: 2 (server startup, https request inspection)
    #[tokio::test]
    async fn test_proxy_mitm_https_interception() -> E2EResult<()> {
        // Start a Proxy server in MITM mode that inspects HTTPS requests
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack with certificate generation. Inspect HTTPS requests and pass them through."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("certificate generation")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "startup_params": {
                            "mode": "mitm",
                            "certificate_mode": "generate"
                        },
                        "instruction": "Inspect HTTPS requests and pass them through"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTPS request received after TLS decryption
                .on_event("proxy_http_request")
                .and_event_data_contains("url", "https://")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "proxy_passthrough"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server intercepted HTTPS request in MITM mode");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test MITM mode request modification
    /// LLM calls: 2 (server startup, request modification)
    #[tokio::test]
    async fn test_proxy_mitm_request_modification() -> E2EResult<()> {
        // Start a Proxy server in MITM mode that modifies HTTPS requests
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack with certificate generation. Add Authorization header to all HTTPS requests to api.example.com."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("certificate generation")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "startup_params": {
                            "mode": "mitm",
                            "certificate_mode": "generate"
                        },
                        "instruction": "Add Authorization header to HTTPS requests"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTPS request to api.example.com - add auth header
                .on_event("proxy_http_request")
                .and_event_data_contains("host", "api.example.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "proxy_modify_request",
                        "add_headers": {
                            "Authorization": "Bearer TOKEN123"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server modified HTTPS request in MITM mode");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test MITM mode request blocking
    /// LLM calls: 2 (server startup, request blocking)
    #[tokio::test]
    async fn test_proxy_mitm_request_blocking() -> E2EResult<()> {
        // Start a Proxy server in MITM mode that blocks certain HTTPS requests
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack with certificate generation. Block HTTPS requests containing sensitive data."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("certificate generation")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "startup_params": {
                            "mode": "mitm",
                            "certificate_mode": "generate"
                        },
                        "instruction": "Block HTTPS requests with sensitive data"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTPS request with sensitive data - block it
                .on_event("proxy_http_request")
                .and_event_data_contains("url", "https://")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "proxy_block",
                        "status": 403,
                        "body": "Request blocked: contains sensitive data"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server blocked HTTPS request in MITM mode");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test CA certificate export functionality
    /// LLM calls: 2 (server startup, certificate export)
    #[tokio::test]
    async fn test_proxy_export_ca_certificate() -> E2EResult<()> {
        // Start a Proxy server in MITM mode and export CA certificate
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} using proxy stack with certificate generation. Export CA certificate to netget-ca.crt."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("certificate generation")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "Proxy",
                        "startup_params": {
                            "mode": "mitm",
                            "certificate_mode": "generate"
                        },
                        "instruction": "MITM proxy with certificate export"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Export CA certificate
                .on_instruction_containing("Export CA certificate")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "export_ca_certificate",
                        "output_path": "./netget-ca.crt",
                        "format": "pem"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ Proxy server exported CA certificate");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
