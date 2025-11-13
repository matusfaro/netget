//! End-to-end WebDAV tests for NetGet
//!
//! These tests spawn the actual NetGet binary with WebDAV prompts
//! and validate file operations using real WebDAV clients.

#![cfg(feature = "webdav")]

// Helper module imported from parent

use super::super::super::helpers::{self, E2EResult};

#[tokio::test]
async fn test_webdav_server_start() -> E2EResult<()> {
    println!("\n=== E2E Test: WebDAV Server Start ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: Basic WebDAV server
    let prompt = "listen on port {AVAILABLE_PORT} using webdav stack. Provide a virtual filesystem with directory /documents";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("webdav")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebDAV",
                        "instruction": "WebDAV server with /documents directory"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the WebDAV server
    let mut server = helpers::start_netget_server(config).await?;
    println!("WebDAV server started on port {}", server.port);

    // Verify it's a WebDAV server
    assert_eq!(
        server.stack, "WebDAV",
        "Expected WebDAV server but got {}",
        server.stack
    );

    println!("✓ WebDAV server initialized successfully");

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_webdav_propfind() -> E2EResult<()> {
    println!("\n=== E2E Test: WebDAV PROPFIND ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: WebDAV server with file listings
    let prompt = "listen on port {AVAILABLE_PORT} using webdav stack. For PROPFIND requests on /, return directory listing showing 'documents' folder";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("webdav")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebDAV",
                        "instruction": "WebDAV server with directory listings"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the WebDAV server
    let mut server = helpers::start_netget_server(config).await?;
    println!("WebDAV server started on port {}", server.port);

    // VALIDATION: Make PROPFIND request using reqwest
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client
        .request(reqwest::Method::from_bytes(b"PROPFIND")?, &url)
        .header("Depth", "1")
        .send()
        .await?;

    // WebDAV PROPFIND typically returns 207 Multi-Status
    println!("PROPFIND response status: {}", response.status());

    // For now, just verify we got a response (207 or 200 are both acceptable)
    assert!(
        response.status().is_success() || response.status().as_u16() == 207,
        "Expected successful WebDAV response, got {}",
        response.status()
    );

    println!("✓ PROPFIND request handled");

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_webdav_put_file() -> E2EResult<()> {
    println!("\n=== E2E Test: WebDAV PUT File ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: WebDAV server with file creation
    let prompt = "listen on port {AVAILABLE_PORT} using webdav stack. Accept PUT requests to create files. Return status 201 Created";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("webdav")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebDAV",
                        "instruction": "WebDAV server accepting PUT requests"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the WebDAV server
    let mut server = helpers::start_netget_server(config).await?;
    println!("WebDAV server started on port {}", server.port);

    // VALIDATION: Make PUT request to create a file
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/test.txt", server.port);

    let response = client.put(&url).body("Hello WebDAV!").send().await?;

    println!("PUT response status: {}", response.status());

    // Accept 201 Created or 204 No Content as success
    assert!(
        response.status().as_u16() == 201
            || response.status().as_u16() == 204
            || response.status().is_success(),
        "Expected 201/204 for file creation, got {}",
        response.status()
    );

    println!("✓ File creation request handled");

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_webdav_mkcol() -> E2EResult<()> {
    println!("\n=== E2E Test: WebDAV MKCOL (Create Collection) ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: WebDAV server with directory creation
    let prompt = "listen on port {AVAILABLE_PORT} using webdav stack. Accept MKCOL requests to create directories. Return status 201 Created";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("webdav")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "WebDAV",
                        "instruction": "WebDAV server accepting MKCOL requests"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the WebDAV server
    let mut server = helpers::start_netget_server(config).await?;
    println!("WebDAV server started on port {}", server.port);

    // VALIDATION: Make MKCOL request to create a directory
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/newdir/", server.port);

    let response = client
        .request(reqwest::Method::from_bytes(b"MKCOL")?, &url)
        .send()
        .await?;

    println!("MKCOL response status: {}", response.status());

    // Accept 201 Created or other success codes
    assert!(
        response.status().as_u16() == 201 || response.status().is_success(),
        "Expected 201 for directory creation, got {}",
        response.status()
    );

    println!("✓ Directory creation request handled");

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
