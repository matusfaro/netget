//! End-to-end WebDAV tests for NetGet
//!
//! These tests spawn the actual NetGet binary with WebDAV prompts
//! and validate file operations using real WebDAV clients.

#![cfg(feature = "webdav")]

// Helper module imported from parent

use super::super::super::helpers::{self, E2EResult};

/// Count the `<D:response>` elements in a WebDAV multistatus body, whatever namespace
/// prefix the server chose.
///
/// These tests used to assert only on the HTTP status, which is why WebDAV could be rated
/// Experimental for a server that never consults the model: a status code says the
/// `dav-server` library answered, not that anything was stored or listed.
fn count_multistatus_responses(body: &str) -> usize {
    body.split('<')
        .filter(|token| {
            // Skip closing tags, then drop any namespace prefix ("D:response>" -> "response>").
            let tag = token.split(':').next_back().unwrap_or(token);
            !token.starts_with('/') && tag == "response>"
        })
        .count()
}

/// True if a multistatus body lists `href` as one of its resources.
fn multistatus_lists(body: &str, href: &str) -> bool {
    body.contains(&format!(">{href}<"))
}

#[tokio::test]
async fn test_webdav_server_start() -> E2EResult<()> {
    println!("\n=== E2E Test: WebDAV Server Start ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: Basic WebDAV server
    let prompt = "listen on port {AVAILABLE_PORT} using webdav stack. Provide a virtual filesystem with directory /documents";

    let config = NetGetConfig::new(prompt).with_mock(|mock| {
        mock.on_instruction_containing("webdav")
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

    let config = NetGetConfig::new(prompt).with_mock(|mock| {
        mock.on_instruction_containing("webdav")
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

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", server.port);

    // Put a file in place first, so the listing has something to list. Without this the
    // test could not distinguish a working PROPFIND from one that returns an empty
    // multistatus.
    let put = client
        .put(format!("{base}/listed.txt"))
        .body("listed")
        .send()
        .await?;
    assert_eq!(put.status().as_u16(), 201, "PUT must create the file");

    // Depth: 1 on the root must return the collection itself plus its one member.
    let response = client
        .request(
            reqwest::Method::from_bytes(b"PROPFIND")?,
            format!("{base}/"),
        )
        .header("Depth", "1")
        .send()
        .await?;

    println!("PROPFIND response status: {}", response.status());
    assert_eq!(
        response.status().as_u16(),
        207,
        "RFC 4918 §9.1: PROPFIND answers 207 Multi-Status"
    );
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.contains("xml"),
        "a multistatus body must be XML, got content-type {content_type:?}"
    );

    let body = response.text().await?;
    println!("PROPFIND body:\n{body}");
    assert!(
        body.contains("multistatus"),
        "body must be a DAV:multistatus document"
    );
    assert_eq!(
        count_multistatus_responses(&body),
        2,
        "Depth: 1 on the root must list the collection and its one member; body was:\n{body}"
    );
    assert!(
        multistatus_lists(&body, "/"),
        "the collection itself must appear in a Depth: 1 listing"
    );
    assert!(
        multistatus_lists(&body, "/listed.txt"),
        "the file just created must appear in the listing"
    );

    println!("✓ PROPFIND returned a well-formed multistatus listing");

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

    let config = NetGetConfig::new(prompt).with_mock(|mock| {
        mock.on_instruction_containing("webdav")
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

    // VALIDATION: PUT then GET, so the test proves the bytes were stored rather than that
    // the request was merely acknowledged.
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/test.txt", server.port);

    let response = client.put(&url).body("Hello WebDAV!").send().await?;

    println!("PUT response status: {}", response.status());
    assert_eq!(
        response.status().as_u16(),
        201,
        "RFC 4918 §9.7.1: creating a new resource answers 201 Created"
    );

    let fetched = client.get(&url).send().await?;
    assert_eq!(
        fetched.status().as_u16(),
        200,
        "the new file must be GETtable"
    );
    assert_eq!(
        fetched.text().await?,
        "Hello WebDAV!",
        "GET must return exactly the bytes PUT stored"
    );

    // Overwriting an existing resource is 204 No Content, not another 201.
    let overwrite = client.put(&url).body("Overwritten").send().await?;
    assert_eq!(
        overwrite.status().as_u16(),
        204,
        "RFC 4918 §9.7.1: overwriting an existing resource answers 204 No Content"
    );
    assert_eq!(
        client.get(&url).send().await?.text().await?,
        "Overwritten",
        "the overwrite must be visible to a subsequent GET"
    );

    println!("✓ PUT/GET round-trip and overwrite semantics verified");

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

    let config = NetGetConfig::new(prompt).with_mock(|mock| {
        mock.on_instruction_containing("webdav")
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

    // VALIDATION: MKCOL, then prove the collection exists and behaves like one.
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", server.port);
    let url = format!("{base}/newdir/");

    let response = client
        .request(reqwest::Method::from_bytes(b"MKCOL")?, &url)
        .send()
        .await?;

    println!("MKCOL response status: {}", response.status());
    assert_eq!(
        response.status().as_u16(),
        201,
        "RFC 4918 §9.3.1: a successful MKCOL answers 201 Created"
    );

    // A second MKCOL on the same path must be refused: the resource already exists.
    let again = client
        .request(reqwest::Method::from_bytes(b"MKCOL")?, &url)
        .send()
        .await?;
    assert_eq!(
        again.status().as_u16(),
        405,
        "RFC 4918 §9.3.1: MKCOL on an existing resource answers 405 Method Not Allowed"
    );

    // And the collection must actually be there: a file placed inside it must be listed.
    let put = client
        .put(format!("{base}/newdir/inside.txt"))
        .body("inside")
        .send()
        .await?;
    assert_eq!(
        put.status().as_u16(),
        201,
        "PUT into the new collection must succeed, which it cannot if MKCOL did nothing"
    );

    let listing = client
        .request(reqwest::Method::from_bytes(b"PROPFIND")?, &url)
        .header("Depth", "1")
        .send()
        .await?;
    assert_eq!(listing.status().as_u16(), 207);
    let body = listing.text().await?;
    assert!(
        multistatus_lists(&body, "/newdir/inside.txt"),
        "the new collection must list the file placed in it; body was:\n{body}"
    );

    println!("✓ MKCOL created a real, usable collection");

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
