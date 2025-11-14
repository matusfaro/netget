//! E2E tests for PyPI protocol with mocks
//!
//! These tests verify PyPI server functionality using mock LLM responses.
//! Test strategy: Mock HTTP responses for PEP 503 endpoints, < 10 LLM calls total.

#[cfg(all(test, feature = "pypi"))]
mod pypi_server_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test PyPI package index endpoint with mocks
    /// LLM calls: 2 (server startup, http_request for /simple/)
    #[tokio::test]
    async fn test_pypi_package_index_with_mocks() -> E2EResult<()> {
        // Start a PyPI server with mocks
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via pypi. Serve package index with hello-world package."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("pypi")
                .and_instruction_containing("package index")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP",
                        "instruction": "PyPI server - serve package index"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: PyPI request for /simple/
                .on_event("pypi_request")
                .and_event_data_contains("path", "/simple")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_pypi_response",
                        "status": 200,
                        "headers": {
                            "Content-Type": "text/html"
                        },
                        "body": "<!DOCTYPE html><html><body><a href=\"hello-world/\">hello-world</a></body></html>"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Make HTTP request to trigger pypi_request event
        let url = format!("http://127.0.0.1:{}/simple/", server.port);
        let _ = std::process::Command::new("curl")
            .arg("-s")
            .arg(&url)
            .output();

        tokio::time::sleep(Duration::from_millis(100)).await;

        println!("✅ PyPI server served package index with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test PyPI package page endpoint with mocks
    /// LLM calls: 2 (server startup, http_request for /simple/hello-world/)
    #[tokio::test]
    async fn test_pypi_package_page_with_mocks() -> E2EResult<()> {
        // Start a PyPI server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via pypi. Serve hello-world package page with wheel file."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("pypi")
                .and_instruction_containing("hello-world")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP",
                        "instruction": "PyPI server - serve hello-world package"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: PyPI request for package page
                .on_event("pypi_request")
                .and_event_data_contains("path", "hello-world")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_pypi_response",
                        "status": 200,
                        "headers": {
                            "Content-Type": "text/html"
                        },
                        "body": "<!DOCTYPE html><html><body><a href=\"../../packages/h/hello-world/hello_world-1.0.0-py3-none-any.whl#sha256=abc123\">hello_world-1.0.0-py3-none-any.whl</a></body></html>"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Make HTTP request to trigger pypi_request event
        let url = format!("http://127.0.0.1:{}/simple/hello-world/", server.port);
        let _ = std::process::Command::new("curl")
            .arg("-s")
            .arg(&url)
            .output();

        tokio::time::sleep(Duration::from_millis(100)).await;

        println!("✅ PyPI server served package page with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test PyPI 404 for non-existent package with mocks
    /// LLM calls: 2 (server startup, http_request for unknown package)
    #[tokio::test]
    async fn test_pypi_package_not_found_with_mocks() -> E2EResult<()> {
        // Start a PyPI server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via pypi. Return 404 for non-existent packages."
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("pypi")
                .and_instruction_containing("404")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP",
                        "instruction": "PyPI server - return 404 for unknown packages"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: PyPI request for non-existent package
                .on_event("pypi_request")
                .and_event_data_contains("path", "nonexistent")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_pypi_response",
                        "status": 404,
                        "body": "Not Found"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Make HTTP request to trigger pypi_request event
        let url = format!("http://127.0.0.1:{}/simple/nonexistent-package/", server.port);
        let _ = std::process::Command::new("curl")
            .arg("-s")
            .arg(&url)
            .output();

        tokio::time::sleep(Duration::from_millis(100)).await;

        println!("✅ PyPI server returned 404 for non-existent package with mocks");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
