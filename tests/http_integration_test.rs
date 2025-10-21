//! Integration tests for HTTP stack
//!
//! These tests require Ollama to be running with the configured model

use netget::events::{HttpResponse, NetworkEvent};
use netget::llm::{HttpLlmResponse, OllamaClient, PromptBuilder};
use netget::network::HttpServer;
use netget::protocol::BaseStack;
use netget::state::app_state::{AppState, Mode};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

/// Get an available port for testing
async fn get_available_port() -> u16 {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Test helper to start HTTP server
async fn start_http_server(instructions: String) -> (AppState, u16, tokio::task::JoinHandle<()>) {
    let state = AppState::new();

    // Set up as HTTP server
    state.set_mode(Mode::Server).await;
    state.set_base_stack(BaseStack::Http).await;
    state.add_instruction(instructions).await;

    let (network_tx, mut network_rx) = mpsc::unbounded_channel::<NetworkEvent>();
    let llm = OllamaClient::default();

    // Get an available port dynamically
    let port = get_available_port().await;

    // Create and start HTTP server
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    let http_server = HttpServer::new(listen_addr, network_tx.clone())
        .await
        .expect("Failed to create HTTP server");

    println!("Test HTTP server: Starting on port {}", port);

    // Spawn HTTP server accept loop
    tokio::spawn(async move {
        if let Err(e) = http_server.accept_loop().await {
            eprintln!("HTTP server error: {}", e);
        }
    });

    let state_for_events = state.clone();

    // Spawn event processing loop - handles HttpRequest events
    let handle = tokio::spawn(async move {
        while let Some(net_event) = network_rx.recv().await {
            println!("Test server: Processing event: {:?}", net_event);

            match net_event {
                NetworkEvent::HttpRequest {
                    connection_id,
                    method,
                    uri,
                    headers,
                    body,
                    response_tx,
                } => {
                    println!(
                        "Test server: HTTP {} {} from {}",
                        method, uri, connection_id
                    );

                    // Ask LLM to generate HTTP response
                    let model = state_for_events.get_ollama_model().await;
                    let prompt = PromptBuilder::build_http_request_prompt(
                        &state_for_events,
                        connection_id,
                        &method,
                        &uri,
                        &headers,
                        &body,
                    )
                    .await;

                    match llm.generate(&model, &prompt).await {
                        Ok(raw_response) => {
                            println!("Test server: LLM response: {}", raw_response);

                            // Parse HTTP response from LLM
                            match HttpLlmResponse::from_str(&raw_response) {
                                Ok(http_response) => {
                                    if let Some(msg) = &http_response.log_message {
                                        println!("Test server: LLM log: {}", msg);
                                    }

                                    // Convert to event HttpResponse and send back
                                    let _ = response_tx.send(http_response.to_event_response());
                                }
                                Err(e) => {
                                    eprintln!("Test server: Failed to parse HTTP response: {}", e);
                                    // Send a 500 error response
                                    let _ = response_tx.send(HttpResponse {
                                        status: 500,
                                        headers: std::collections::HashMap::new(),
                                        body: bytes::Bytes::from("Internal Server Error"),
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Test server: LLM error: {}", e);
                            // Send a 500 error response
                            let _ = response_tx.send(HttpResponse {
                                status: 500,
                                headers: std::collections::HashMap::new(),
                                body: bytes::Bytes::from("Internal Server Error"),
                            });
                        }
                    }
                }
                NetworkEvent::Connected { connection_id, .. } => {
                    println!("Test server: Connection {} established", connection_id);
                }
                NetworkEvent::Disconnected { connection_id } => {
                    println!("Test server: Connection {} closed", connection_id);
                }
                _ => {}
            }
        }
    });

    // Wait for server to be ready
    sleep(Duration::from_millis(500)).await;

    (state, port, handle)
}

#[tokio::test]
async fn test_http_server_post_json() {
    println!("\n=== Testing POST request with JSON response ===");

    // Start server with instructions to return JSON
    let (_state, port, _handle) = start_http_server(
        "For any POST request, return a JSON response with status 200, Content-Type: application/json, and body: {\"status\": \"success\", \"message\": \"Data received\"}".to_string()
    ).await;

    // Wait a bit more for full startup
    sleep(Duration::from_millis(500)).await;

    // Make POST request
    let url = format!("http://127.0.0.1:{}/api/data", port);
    let client = reqwest::Client::new();

    println!("Test client: Making POST request to {}", url);

    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "key": "value",
            "number": 42
        }))
        .send()
        .await
        .expect("Failed to make request");

    println!("Test client: Got response with status {}", response.status());

    // Verify status code
    assert_eq!(response.status(), 200, "Expected status 200");

    // Verify Content-Type header
    let content_type = response
        .headers()
        .get("content-type")
        .expect("Missing Content-Type header")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("application/json"),
        "Expected JSON content type, got: {}",
        content_type
    );

    // Verify response body
    let body = response.text().await.expect("Failed to read body");
    println!("Test client: Response body: {}", body);

    let json: serde_json::Value = serde_json::from_str(&body).expect("Invalid JSON response");
    assert_eq!(json["status"], "success");
    assert_eq!(json["message"], "Data received");

    println!("=== POST JSON test passed ===\n");
}

#[tokio::test]
async fn test_http_server_get_html() {
    println!("\n=== Testing GET request with HTML response ===");

    // Start server with instructions to return HTML
    let (_state, port, _handle) = start_http_server(
        "For any GET request, return an HTML page with status 200, Content-Type: text/html, and body: <html><body><h1>Hello from LLM!</h1></body></html>".to_string()
    ).await;

    sleep(Duration::from_millis(500)).await;

    // Make GET request
    let url = format!("http://127.0.0.1:{}/", port);
    let client = reqwest::Client::new();

    println!("Test client: Making GET request to {}", url);

    let response = client.get(&url).send().await.expect("Failed to make request");

    println!("Test client: Got response with status {}", response.status());

    // Verify status code
    assert_eq!(response.status(), 200, "Expected status 200");

    // Verify Content-Type header
    let content_type = response
        .headers()
        .get("content-type")
        .expect("Missing Content-Type header")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("text/html"),
        "Expected HTML content type, got: {}",
        content_type
    );

    // Verify response body
    let body = response.text().await.expect("Failed to read body");
    println!("Test client: Response body: {}", body);

    assert!(body.contains("<h1>Hello from LLM!</h1>"), "Expected HTML content");

    println!("=== GET HTML test passed ===\n");
}

#[tokio::test]
async fn test_http_server_custom_headers() {
    println!("\n=== Testing custom headers ===");

    // Start server with instructions to return custom headers
    let (_state, port, _handle) = start_http_server(
        "For any request to /custom, return status 201, with headers: X-Custom-Header: test-value and X-Request-ID: 12345, and body: Custom response".to_string()
    ).await;

    sleep(Duration::from_millis(500)).await;

    // Make GET request
    let url = format!("http://127.0.0.1:{}/custom", port);
    let client = reqwest::Client::new();

    println!("Test client: Making GET request to {}", url);

    let response = client.get(&url).send().await.expect("Failed to make request");

    println!("Test client: Got response with status {}", response.status());

    // Verify status code
    assert_eq!(response.status(), 201, "Expected status 201");

    // Verify custom headers
    let custom_header = response
        .headers()
        .get("x-custom-header")
        .expect("Missing X-Custom-Header")
        .to_str()
        .unwrap();
    assert_eq!(custom_header, "test-value");

    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("Missing X-Request-ID")
        .to_str()
        .unwrap();
    assert_eq!(request_id, "12345");

    // Verify response body
    let body = response.text().await.expect("Failed to read body");
    println!("Test client: Response body: {}", body);
    assert_eq!(body, "Custom response");

    println!("=== Custom headers test passed ===\n");
}

#[tokio::test]
async fn test_http_server_404() {
    println!("\n=== Testing 404 response ===");

    // Start server with instructions to return 404 for /notfound
    let (_state, port, _handle) = start_http_server(
        "For any request to /notfound, return status 404 with body: Page not found".to_string()
    ).await;

    sleep(Duration::from_millis(500)).await;

    // Make GET request
    let url = format!("http://127.0.0.1:{}/notfound", port);
    let client = reqwest::Client::new();

    println!("Test client: Making GET request to {}", url);

    let response = client.get(&url).send().await.expect("Failed to make request");

    println!("Test client: Got response with status {}", response.status());

    // Verify status code
    assert_eq!(response.status(), 404, "Expected status 404");

    // Verify response body
    let body = response.text().await.expect("Failed to read body");
    println!("Test client: Response body: {}", body);
    assert_eq!(body, "Page not found");

    println!("=== 404 test passed ===\n");
}
