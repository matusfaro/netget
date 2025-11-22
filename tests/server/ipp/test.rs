//! End-to-end IPP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with IPP prompts
//! and validate the responses using HTTP clients (IPP runs over HTTP).

#![cfg(feature = "ipp")]

// Helper module imported from parent

use super::super::super::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;

#[tokio::test]
async fn test_ipp_get_printer_attributes() -> E2EResult<()> {
    println!("\n=== E2E Test: IPP Get-Printer-Attributes ===");

    // PROMPT: Tell the LLM to act as an IPP printer
    let prompt = "Open IPP on port {AVAILABLE_PORT}. When clients send Get-Printer-Attributes IPP requests, \
        use ipp_printer_attributes action with attributes={\"printer-name\":\"NetGet Printer\",\
        \"printer-state\":\"idle\",\"printer-uri-supported\":\"ipp://localhost:{AVAILABLE_PORT}/printers/netget\"}.";

    // Start the server with mocks
    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_instruction_containing("Open IPP")
                .and_instruction_containing("port")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "IPP",
                        "instruction": "IPP printer responding to Get-Printer-Attributes with printer-name='NetGet Printer', printer-state='idle'"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: IPP request received (ipp_request_received event)
                .on_event("ipp_request_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "ipp_printer_attributes",
                        "attributes": {
                            "printer-name": "NetGet Printer",
                            "printer-state": "idle",
                            "printer-uri-supported": format!("ipp://localhost:{{AVAILABLE_PORT}}/printers/netget")
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let mut server = helpers::start_netget_server(config).await?;
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
            .send(),
    )
    .await
    {
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
        println!(
            "IPP status code: 0x{:04x} ({})",
            status,
            if status == 0 {
                "successful-ok"
            } else {
                "error"
            }
        );
    }

    println!("✓ IPP Get-Printer-Attributes test completed\n");

    // Verify mock expectations were met
    server.verify_mocks().await?;

    Ok(())
}

#[tokio::test]
async fn test_ipp_print_job() -> E2EResult<()> {
    println!("\n=== E2E Test: IPP Print-Job ===");

    let prompt = "Open IPP on port {AVAILABLE_PORT}. When clients send Print-Job IPP requests, \
        use ipp_job_attributes action with attributes={\"job-id\":1,\"job-state\":\"processing\",\
        \"job-name\":\"test\"}.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_instruction_containing("Open IPP")
                .and_instruction_containing("port")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "IPP",
                        "instruction": "IPP printer accepting Print-Job requests with job-id=1, job-state='processing'"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: IPP Print-Job request received
                .on_event("ipp_request_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "ipp_job_attributes",
                        "attributes": {
                            "job-id": 1,
                            "job-state": "processing",
                            "job-name": "test"
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let mut server = helpers::start_netget_server(config).await?;
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

    // Verify mock expectations were met
    server.verify_mocks().await?;

    Ok(())
}

#[tokio::test]
async fn test_ipp_basic_http() -> E2EResult<()> {
    println!("\n=== E2E Test: IPP Basic HTTP Communication ===");

    let prompt = "Open IPP on port {AVAILABLE_PORT}. For all IPP requests, use ipp_response action with status=200.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_instruction_containing("Open IPP")
                .and_instruction_containing("port")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "IPP",
                        "instruction": "IPP server responding to all requests with status 200"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: HTTP GET request (not typical IPP but tests HTTP layer)
                .on_event("ipp_request_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "http_response",
                        "status": 200,
                        "body": ""
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let mut server = helpers::start_netget_server(config).await?;
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
        response.status().is_success()
            || response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "Expected successful response or method not allowed"
    );

    println!("✓ IPP basic HTTP test completed\n");

    // Verify mock expectations were met
    server.verify_mocks().await?;

    Ok(())
}
