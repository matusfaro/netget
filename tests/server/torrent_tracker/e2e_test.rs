//! BitTorrent Tracker E2E tests with mocks

#![cfg(all(test, feature = "torrent-tracker"))]

use crate::helpers::*;
use serde_json::json;
use std::time::Duration;

/// Test tracker announce and scrape requests with mocks
///
/// LLM calls: 3 total
/// - 1 server startup
/// - 1 announce request
/// - 1 scrape request
#[tokio::test]
async fn test_tracker_announce_and_scrape() -> E2EResult<()> {
    // Start tracker server with mocks
    let server_config = NetGetConfig::new(
        "Listen on port {AVAILABLE_PORT} via torrent-tracker. Return peer lists for announce requests with 30-minute interval."
    )
    .with_mock(|mock| {
        mock
            // Mock 1: Server startup
            .on_instruction_containing("Listen on port")
            .and_instruction_containing("torrent-tracker")
            .respond_with_actions(json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "Torrent-Tracker",
                    "instruction": "BitTorrent tracker with peer coordination"
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 2: Announce request
            .on_event("tracker_announce_request")
            .respond_with_actions(json!([
                {
                    "type": "send_announce_response",
                    "interval": 1800,
                    "complete": 10,
                    "incomplete": 5,
                    "peers": [
                        {
                            "peer_id": "-TR2940-xxxxxxxxxxxx",
                            "ip": "192.168.1.100",
                            "port": 51413
                        },
                        {
                            "peer_id": "-UT2210-yyyyyyyyyyyy",
                            "ip": "192.168.1.101",
                            "port": 6881
                        }
                    ]
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 3: Scrape request
            .on_event("tracker_scrape_request")
            .respond_with_actions(json!([
                {
                    "type": "send_scrape_response",
                    "files": [
                        {
                            "info_hash": "0123456789abcdef0123456789abcdef01234567",
                            "complete": 10,
                            "incomplete": 5,
                            "downloaded": 100
                        }
                    ]
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let mut server = start_netget_server(server_config).await?;

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Test announce request
    let client = reqwest::Client::new();
    let announce_url = format!(
        "http://127.0.0.1:{}/announce?info_hash=%01%23%45%67%89%AB%CD%EF%01%23%45%67%89%AB%CD%EF%01%23%45%67&peer_id=TESTPEER12345678901&port=6881&uploaded=0&downloaded=0&left=1000000&event=started&compact=0",
        server.port
    );

    println!("Sending announce request to {}", announce_url);

    let response = client
        .get(&announce_url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    assert!(
        response.status().is_success(),
        "Announce request failed: {}",
        response.status()
    );

    let body = response.bytes().await?;
    println!("Announce response: {} bytes", body.len());

    // Parse bencode response
    let value: serde_bencode::value::Value = serde_bencode::from_bytes(&body)?;
    let dict = match value { serde_bencode::value::Value::Dict(d) => d, _ => panic!("Expected Dict") };

    // Verify interval
    let interval = dict
        .get(b"interval" as &[u8])
        .and_then(|v| match v {
            serde_bencode::value::Value::Int(i) => Some(i),
            _ => None,
        })
        .expect("Missing interval");
    assert_eq!(*interval, 1800, "Interval should be 1800");

    // Verify peers exist
    assert!(
        dict.contains_key::<[u8]>(b"peers".as_ref()),
        "Response should contain peers"
    );

    println!("✅ Announce request successful");

    // Test scrape request
    let scrape_url = format!(
        "http://127.0.0.1:{}/scrape?info_hash=%01%23%45%67%89%AB%CD%EF%01%23%45%67%89%AB%CD%EF%01%23%45%67",
        server.port
    );

    println!("Sending scrape request to {}", scrape_url);

    let response = client
        .get(&scrape_url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    assert!(
        response.status().is_success(),
        "Scrape request failed: {}",
        response.status()
    );

    let body = response.bytes().await?;
    println!("Scrape response: {} bytes", body.len());

    // Parse bencode response
    let value: serde_bencode::value::Value = serde_bencode::from_bytes(&body)?;
    let dict = match value { serde_bencode::value::Value::Dict(d) => d, _ => panic!("Expected Dict") };

    // Verify files dictionary exists
    assert!(
        dict.contains_key::<[u8]>(b"files".as_ref()),
        "Response should contain files"
    );

    println!("✅ Scrape request successful");

    // Verify all mocks were called
    server.verify_mocks().await?;

    // Cleanup
    server.stop().await?;

    Ok(())
}

/// Test tracker error response with mocks
///
/// LLM calls: 2 total
/// - 1 server startup
/// - 1 error response
#[tokio::test]
async fn test_tracker_error_response() -> E2EResult<()> {
    // Start tracker server with mocks
    let server_config = NetGetConfig::new(
        "Listen on port {AVAILABLE_PORT} via torrent-tracker. Return errors for invalid requests."
    )
    .with_mock(|mock| {
        mock
            // Mock: Server startup
            .on_instruction_containing("Listen on port")
            .and_instruction_containing("torrent-tracker")
            .respond_with_actions(json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "Torrent-Tracker",
                    "instruction": "Tracker with error handling"
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock: Error response for missing parameters
            .on_event("tracker_announce_request")
            .respond_with_actions(json!([
                {
                    "type": "send_error_response",
                    "failure_reason": "Missing required parameter: info_hash"
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let mut server = start_netget_server(server_config).await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send malformed request (missing info_hash)
    let client = reqwest::Client::new();
    let announce_url = format!(
        "http://127.0.0.1:{}/announce?peer_id=TESTPEER12345678901&port=6881",
        server.port
    );

    let response = client
        .get(&announce_url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    assert!(response.status().is_success(), "Should return HTTP 200");

    let body = response.bytes().await?;
    let value: serde_bencode::value::Value = serde_bencode::from_bytes(&body)?;
    let dict = match value { serde_bencode::value::Value::Dict(d) => d, _ => panic!("Expected Dict") };

    // Verify error response
    assert!(
        dict.contains_key(b"failure reason" as &[u8]),
        "Response should contain failure reason"
    );

    println!("✅ Error response test successful");

    // Verify mocks
    server.verify_mocks().await?;
    server.stop().await?;

    Ok(())
}
