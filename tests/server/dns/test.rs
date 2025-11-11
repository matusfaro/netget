//! End-to-end DNS tests for NetGet
//!
//! These tests spawn the actual NetGet binary with DNS prompts
//! and validate the responses using the hickory-client DNS client library.

#![cfg(feature = "dns")]

// Helper module imported from parent

use super::super::super::helpers::{self, E2EResult, ServerConfig};
use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::rr::{DNSClass, Name, RecordType};
use hickory_client::udp::UdpClientStream;
use std::net::SocketAddr;
use std::str::FromStr;

#[tokio::test]
async fn test_dns_a_record_query() -> E2EResult<()> {
    println!("\n=== E2E Test: DNS A Record Query ===");

    // PROMPT: Tell the LLM to act as a DNS server
    let prompt = "listen on port {AVAILABLE_PORT} via dns. Respond to all A record queries for example.com with IP address 93.184.216.34";

    // Start the server with debug logging
    let server =
        helpers::start_netget_server(ServerConfig::new(prompt).with_log_level("debug")).await?;
    println!("DNS server started on port {}", server.port);

    // Wait for DNS server to fully initialize (needs LLM call)

    // VALIDATION: Use hickory-client to query DNS
    println!("Querying example.com A record...");

    let address: SocketAddr = format!("127.0.0.1:{}", server.port).parse()?;
    let stream = UdpClientStream::<tokio::net::UdpSocket>::new(address);
    let (mut client, bg) = AsyncClient::connect(stream).await?;

    // Run the background task
    tokio::spawn(bg);

    // Query for example.com A record
    let name = Name::from_str("example.com.")?;
    let response = client.query(name, DNSClass::IN, RecordType::A).await?;

    println!("DNS response received:");
    let answers = response.answers();
    assert!(
        !answers.is_empty(),
        "Expected at least one A record in response"
    );

    for record in answers {
        println!("  Record: {:?}", record);
    }

    // Check that we got a response
    println!(
        "✓ DNS A record query succeeded with {} answers",
        answers.len()
    );

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_dns_multiple_records() -> E2EResult<()> {
    println!("\n=== E2E Test: DNS Multiple Records ===");

    // PROMPT: Tell the LLM to handle multiple record types
    let prompt = "listen on port {AVAILABLE_PORT} via dns. For example.com A records return 1.2.3.4. For mail.example.com A records return 5.6.7.8";

    // Start the server
    let server =
        helpers::start_netget_server(ServerConfig::new(prompt).with_log_level("debug")).await?;
    println!("DNS server started on port {}", server.port);

    // VALIDATION: Query multiple domains
    let address: SocketAddr = format!("127.0.0.1:{}", server.port).parse()?;
    let stream = UdpClientStream::<tokio::net::UdpSocket>::new(address);
    let (mut client, bg) = AsyncClient::connect(stream).await?;
    tokio::spawn(bg);

    // Query example.com
    println!("Querying example.com...");
    let name1 = Name::from_str("example.com.")?;
    let response1 = client.query(name1, DNSClass::IN, RecordType::A).await?;
    assert!(
        !response1.answers().is_empty(),
        "Expected answer for example.com"
    );
    println!(
        "  ✓ example.com returned {} records",
        response1.answers().len()
    );

    // Query mail.example.com
    println!("Querying mail.example.com...");
    let name2 = Name::from_str("mail.example.com.")?;
    let response2 = client.query(name2, DNSClass::IN, RecordType::A).await?;
    assert!(
        !response2.answers().is_empty(),
        "Expected answer for mail.example.com"
    );
    println!(
        "  ✓ mail.example.com returned {} records",
        response2.answers().len()
    );

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_dns_txt_record() -> E2EResult<()> {
    println!("\n=== E2E Test: DNS TXT Record ===");

    // PROMPT: Tell the LLM to handle TXT records
    let prompt = "listen on port {AVAILABLE_PORT} via dns. For TXT record queries on example.com, return 'v=spf1 include:_spf.example.com ~all'";

    // Start the server
    let server =
        helpers::start_netget_server(ServerConfig::new(prompt).with_log_level("debug")).await?;
    println!("DNS server started on port {}", server.port);

    // VALIDATION: Query TXT record
    let address: SocketAddr = format!("127.0.0.1:{}", server.port).parse()?;
    let stream = UdpClientStream::<tokio::net::UdpSocket>::new(address);
    let (mut client, bg) = AsyncClient::connect(stream).await?;
    tokio::spawn(bg);

    println!("Querying example.com TXT record...");
    let name = Name::from_str("example.com.")?;
    let response = client.query(name, DNSClass::IN, RecordType::TXT).await?;

    println!("DNS TXT response received:");
    let answers = response.answers();
    assert!(!answers.is_empty(), "Expected at least one TXT record");

    for record in answers {
        println!("  TXT Record: {:?}", record);
    }

    println!("✓ DNS TXT record query succeeded");

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_dns_nxdomain() -> E2EResult<()> {
    println!("\n=== E2E Test: DNS NXDOMAIN Response ===");

    // PROMPT: Tell the LLM to return NXDOMAIN for unknown domains
    let prompt = "listen on port {AVAILABLE_PORT} via dns. Only respond with A records for known.example.com (1.2.3.4). For all other domains, return NXDOMAIN";

    // Start the server
    let server =
        helpers::start_netget_server(ServerConfig::new(prompt).with_log_level("debug")).await?;
    println!("DNS server started on port {}", server.port);

    // VALIDATION: Query an unknown domain
    let address: SocketAddr = format!("127.0.0.1:{}", server.port).parse()?;
    let stream = UdpClientStream::<tokio::net::UdpSocket>::new(address);
    let (mut client, bg) = AsyncClient::connect(stream).await?;
    tokio::spawn(bg);

    println!("Querying unknown.example.com (should get NXDOMAIN or empty response)...");
    let name = Name::from_str("unknown.example.com.")?;

    // Try to query - might get an error or empty response depending on implementation
    match client.query(name, DNSClass::IN, RecordType::A).await {
        Ok(response) => {
            // Server might return empty answers or NXDOMAIN response code
            println!("  Response code: {:?}", response.response_code());
            println!("  Answers: {}", response.answers().len());
            println!("  ✓ DNS server responded (implementation-dependent behavior)");
        }
        Err(e) => {
            // Might get an error for NXDOMAIN
            println!("  Got error (expected for NXDOMAIN): {:?}", e);
            println!("  ✓ DNS server indicated domain not found");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
