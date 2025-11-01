//! End-to-end DNS-over-HTTPS (DoH) tests for NetGet
//!
//! This test spawns a single NetGet DoH server with a Python script
//! and validates multiple query types against the same server instance.

#![cfg(feature = "e2e-tests")]

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use hickory_proto::op::{Message as DnsMessage, Query};
use hickory_proto::rr::{Name, RecordType};
use reqwest::Client;
use std::str::FromStr;
use std::time::Duration;

/// Helper to query DoH server using GET method (base64url encoded)
async fn query_doh_get(client: &Client, port: u16, domain: &str, record_type: RecordType) -> E2EResult<DnsMessage> {
    let url = format!("https://127.0.0.1:{}/dns-query", port);

    // Build DNS query message
    let name = Name::from_str(domain)?;
    let mut query_msg = DnsMessage::new();
    query_msg.add_query(Query::query(name, record_type));
    query_msg.set_recursion_desired(true);

    // Serialize to wire format
    let query_bytes = query_msg.to_vec()?;

    // Encode as base64url
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let encoded = URL_SAFE_NO_PAD.encode(&query_bytes);

    // Send GET request
    let response = client
        .get(&url)
        .query(&[("dns", encoded)])
        .send()
        .await?;

    let response_bytes = response.bytes().await?;

    // Parse DNS response
    let dns_response = DnsMessage::from_vec(&response_bytes)?;

    Ok(dns_response)
}

/// Helper to query DoH server using POST method (binary DNS message)
async fn query_doh_post(client: &Client, port: u16, domain: &str, record_type: RecordType) -> E2EResult<DnsMessage> {
    let url = format!("https://127.0.0.1:{}/dns-query", port);

    // Build DNS query message
    let name = Name::from_str(domain)?;
    let mut query_msg = DnsMessage::new();
    query_msg.add_query(Query::query(name, record_type));
    query_msg.set_recursion_desired(true);

    // Serialize to wire format
    let query_bytes = query_msg.to_vec()?;

    // Send POST request
    let response = client
        .post(&url)
        .header("Content-Type", "application/dns-message")
        .body(query_bytes)
        .send()
        .await?;

    let response_bytes = response.bytes().await?;

    // Parse DNS response
    let dns_response = DnsMessage::from_vec(&response_bytes)?;

    Ok(dns_response)
}

/// Create an HTTP client that accepts self-signed certificates (for testing)
fn create_insecure_client() -> E2EResult<Client> {
    // Initialize rustls crypto provider (required for rustls 0.23+)
    use rustls::crypto::CryptoProvider;
    let _ = CryptoProvider::install_default(rustls::crypto::ring::default_provider());

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .http2_prior_knowledge()
        .timeout(Duration::from_secs(10))
        .build()?;
    Ok(client)
}

#[tokio::test]
async fn test_doh_server() -> E2EResult<()> {
    println!("\n=== E2E Test: DNS-over-HTTPS Server with Script ===");

    // Create a prompt with a simple Python script
    // Keep it short to avoid LLM confusion with long prompts
    let prompt = r#"listen on port {AVAILABLE_PORT} via doh. Respond to all A record queries for example.com with IP 93.184.216.34 and TTL 300."#;

    // Start server (no scripting, pure LLM mode)
    let server = helpers::start_netget_server(
        ServerConfig::new(prompt)
            .with_log_level("info")
            .with_no_scripts(true)  // Disable scripting
    ).await?;

    println!("DoH server started on port {}", server.port);

    // Wait for server to fully initialize
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Create HTTP client
    let client = create_insecure_client()?;

    // Test both GET and POST methods against the same server
    println!("\n[Test 1] Querying via GET method...");
    let response1 = query_doh_get(&client, server.port, "example.com.", RecordType::A).await?;
    assert!(!response1.answers().is_empty(), "Expected answer via GET");
    println!("✓ GET response: {:?}", response1.answers()[0]);

    println!("\n[Test 2] Querying via POST method...");
    let response2 = query_doh_post(&client, server.port, "example.com.", RecordType::A).await?;
    assert!(!response2.answers().is_empty(), "Expected answer via POST");
    println!("✓ POST response: {:?}", response2.answers()[0]);

    println!("\n[Test 3] Another GET query - different domain...");
    let response3 = query_doh_get(&client, server.port, "test.com.", RecordType::A).await?;
    assert!(!response3.answers().is_empty(), "Expected answer (script returns same for all)");
    println!("✓ GET response: {:?}", response3.answers()[0]);

    println!("\n=== All DoH tests passed! ===");

    Ok(())
}
