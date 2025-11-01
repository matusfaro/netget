//! End-to-end IPP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with IPP prompts
//! and validate the responses using HTTP clients (IPP runs over HTTP).

#![cfg(feature = "e2e-tests")]

// Helper module imported from parent

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use std::time::Duration;

#[tokio::test]
async fn test_ipp_get_printer_attributes() -> E2EResult<()> {
    println!("\n=== E2E Test: IPP Get-Printer-Attributes ===");

    // PROMPT: Tell the LLM to act as an IPP printer
    let prompt = "Open IPP on port {AVAILABLE_PORT}. When clients send Get-Printer-Attributes IPP requests, \
        use ipp_printer_attributes action with attributes={\"printer-name\":\"NetGet Printer\",\
        \"printer-state\":\"idle\",\"printer-uri-supported\":\"ipp://localhost:{AVAILABLE_PORT}/printers/netget\"}.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Send HTTP POST request to IPP endpoint
    println!("Sending Get-Printer-Attributes request...");

    let client = reqwest::Client::new();

    // Build a minimal IPP Get-Printer-Attributes request
    // IPP format: version(2) + operation-id(2) + request-id(4) + attributes
    let mut body = Vec::new();

    // Version 2.0
    body.extend_from_slice(&[0x02, 0x00]);

    // Operation ID: Get-Printer-Attributes (0x000B)
    body.extend_from_slice(&[0x00, 0x0B]);

    // Request ID
    body.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

    // Operation attributes tag
    body.push(0x01);

    // attributes-charset (required)
    body.push(0x47); // charset type
    body.extend_from_slice(&[0x00, 0x12]); // name length
    body.extend_from_slice(b"attributes-charset");
    body.extend_from_slice(&[0x00, 0x05]); // value length
    body.extend_from_slice(b"utf-8");

    // attributes-natural-language (required)
    body.push(0x48); // natural-language type
    body.extend_from_slice(&[0x00, 0x1b]); // name length
    body.extend_from_slice(b"attributes-natural-language");
    body.extend_from_slice(&[0x00, 0x05]); // value length
    body.extend_from_slice(b"en-us");

    // printer-uri (required)
    body.push(0x45); // uri type
    body.extend_from_slice(&[0x00, 0x0b]); // name length
    body.extend_from_slice(b"printer-uri");
    let uri = format!("ipp://localhost:{}/printers/netget", server.port);
    let uri_bytes = uri.as_bytes();
    body.extend_from_slice(&[(uri_bytes.len() >> 8) as u8, uri_bytes.len() as u8]);
    body.extend_from_slice(uri_bytes);

    // End-of-attributes tag
    body.push(0x03);

    let response = match tokio::time::timeout(
        Duration::from_secs(10),
        client
            .post(format!("http://127.0.0.1:{}/printers/netget", server.port))
            .header("Content-Type", "application/ipp")
            .body(body)
            .send()
    ).await {
        Ok(Ok(resp)) => {
            println!("✓ Received HTTP response: {}", resp.status());
            resp
        }
        Ok(Err(e)) => {
            println!("✗ HTTP request error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ HTTP request timeout");
            return Err("Request timeout".into());
        }
    };

    assert_eq!(response.status(), 200, "Expected HTTP 200 OK");

    // Try to parse response body
    let response_body = response.bytes().await?;
    println!("Received IPP response: {} bytes", response_body.len());

    if response_body.len() >= 8 {
        // Check IPP version (first 2 bytes should be 0x02 0x00 for v2.0)
        let version = &response_body[0..2];
        println!("IPP version: 0x{:02x}{:02x}", version[0], version[1]);

        // Check status code (bytes 2-3)
        let status = u16::from_be_bytes([response_body[2], response_body[3]]);
        println!("IPP status code: 0x{:04x} ({})", status, if status == 0 { "successful-ok" } else { "error" });
    }

    println!("✓ IPP Get-Printer-Attributes test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_ipp_print_job() -> E2EResult<()> {
    println!("\n=== E2E Test: IPP Print-Job ===");

    let prompt = "Open IPP on port {AVAILABLE_PORT}. When clients send Print-Job IPP requests, \
        use ipp_job_attributes action with attributes={\"job-id\":1,\"job-state\":\"processing\",\
        \"job-name\":\"test\"}.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    println!("Sending Print-Job request...");

    let client = reqwest::Client::new();

    // Build a minimal IPP Print-Job request
    let mut body = Vec::new();

    // Version 2.0
    body.extend_from_slice(&[0x02, 0x00]);

    // Operation ID: Print-Job (0x0002)
    body.extend_from_slice(&[0x00, 0x02]);

    // Request ID
    body.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

    // Operation attributes tag
    body.push(0x01);

    // attributes-charset
    body.push(0x47);
    body.extend_from_slice(&[0x00, 0x12]);
    body.extend_from_slice(b"attributes-charset");
    body.extend_from_slice(&[0x00, 0x05]);
    body.extend_from_slice(b"utf-8");

    // attributes-natural-language
    body.push(0x48);
    body.extend_from_slice(&[0x00, 0x1b]);
    body.extend_from_slice(b"attributes-natural-language");
    body.extend_from_slice(&[0x00, 0x05]);
    body.extend_from_slice(b"en-us");

    // printer-uri
    body.push(0x45);
    body.extend_from_slice(&[0x00, 0x0b]);
    body.extend_from_slice(b"printer-uri");
    let uri = format!("ipp://localhost:{}/printers/netget", server.port);
    let uri_bytes = uri.as_bytes();
    body.extend_from_slice(&[(uri_bytes.len() >> 8) as u8, uri_bytes.len() as u8]);
    body.extend_from_slice(uri_bytes);

    // document-format
    body.push(0x49); // mimeMediaType
    body.extend_from_slice(&[0x00, 0x0f]); // name length
    body.extend_from_slice(b"document-format");
    body.extend_from_slice(&[0x00, 0x0a]); // value length
    body.extend_from_slice(b"text/plain");

    // End-of-attributes tag
    body.push(0x03);

    // Document data (simple text)
    body.extend_from_slice(b"Test print job");

    let response = client
        .post(format!("http://127.0.0.1:{}/printers/netget", server.port))
        .header("Content-Type", "application/ipp")
        .body(body)
        .send()
        .await?;

    println!("✓ Received HTTP response: {}", response.status());

    assert_eq!(response.status(), 200, "Expected HTTP 200 OK");

    println!("✓ IPP Print-Job test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_ipp_basic_http() -> E2EResult<()> {
    println!("\n=== E2E Test: IPP Basic HTTP Communication ===");

    let prompt = "Open IPP on port {AVAILABLE_PORT}. For all IPP requests, use ipp_response action with status=200.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    println!("Sending basic HTTP request...");

    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://127.0.0.1:{}/", server.port))
        .send()
        .await?;

    println!("✓ Received HTTP response: {}", response.status());

    // IPP servers typically return 200 for GET requests
    assert!(
        response.status().is_success() || response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "Expected successful response or method not allowed"
    );

    println!("✓ IPP basic HTTP test completed\n");
    Ok(())
}
