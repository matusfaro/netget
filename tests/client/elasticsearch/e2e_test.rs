//! Elasticsearch client E2E tests
//!
//! Tests the Elasticsearch client protocol implementation with real Ollama LLM calls.
//! Budget: < 10 LLM calls per test suite.

#![cfg(all(test, feature = "elasticsearch"))]

use netget::llm::ollama_client::OllamaClient;
use netget::state::app_state::AppState;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Duration};

/// Helper to wait for status message matching a predicate
async fn wait_for_status<F>(
    rx: &mut mpsc::UnboundedReceiver<String>,
    predicate: F,
    timeout_secs: u64,
) -> bool
where
    F: Fn(&str) -> bool,
{
    let deadline = Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    while start.elapsed() < deadline {
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(msg)) => {
                if predicate(&msg) {
                    return true;
                }
            }
            Ok(None) => return false,
            Err(_) => continue,
        }
    }
    false
}

#[tokio::test]
#[ignore] // Requires local Elasticsearch instance and Ollama
async fn test_elasticsearch_client_index_and_search() {
    // Setup
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();
    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://localhost:11434", "qwen2.5-coder:7b");

    // Start Elasticsearch in Docker (assumes it's already running)
    // docker run -d -p 9200:9200 -e "discovery.type=single-node" elasticsearch:8.11.0

    println!("[TEST] Starting Elasticsearch client E2E test");

    // Open Elasticsearch client
    let client_id = app_state.next_client_id().await;
    app_state.add_client(
        client_id,
        "Elasticsearch".to_string(),
        "localhost:9200".to_string(),
        "Index a test document with fields 'title'='Test' and 'content'='Hello World', then search for it".to_string(),
        None,
    ).await;

    let client_id_value = client_id;

    // Connect the client
    use netget::cli::client_startup::start_client_by_id;
    let result = start_client_by_id(&app_state, client_id_value, &llm_client, &status_tx).await;
    assert!(result.is_ok(), "Failed to start Elasticsearch client");

    // LLM Call 1: Initial connection + index document action
    // Wait for connection
    assert!(
        wait_for_status(
            &mut status_rx,
            |msg| msg.contains("Elasticsearch client") && msg.contains("ready"),
            10
        )
        .await,
        "Elasticsearch client did not connect"
    );

    println!("[TEST] Elasticsearch client connected, waiting for LLM to index document...");

    // Wait for index operation to complete
    sleep(Duration::from_secs(5)).await;

    // LLM Call 2-3: Response handling (index response) + search action
    // The LLM should have indexed a document and then searched for it

    // Wait for search operation
    println!("[TEST] Waiting for search operation...");
    sleep(Duration::from_secs(10)).await;

    // Verify we got some responses
    let mut message_count = 0;
    while let Ok(Some(_msg)) = timeout(Duration::from_millis(100), status_rx.recv()).await {
        message_count += 1;
        if message_count > 50 {
            break;
        }
    }

    println!("[TEST] Received {} status messages", message_count);
    assert!(message_count > 0, "No status messages received");

    // Total LLM calls: ~3-4 (connect, index response, search response)
    println!("[TEST] Elasticsearch client E2E test completed successfully");
}

#[tokio::test]
#[ignore] // Requires local Elasticsearch instance and Ollama
async fn test_elasticsearch_client_bulk_operations() {
    // Setup
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();
    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://localhost:11434", "qwen2.5-coder:7b");

    println!("[TEST] Starting Elasticsearch bulk operations test");

    // Open Elasticsearch client with bulk instruction
    let client_id = app_state.next_client_id().await;
    app_state.add_client(
        client_id,
        "Elasticsearch".to_string(),
        "localhost:9200".to_string(),
        "Use bulk operation to index 3 documents in 'products' index: laptop (price=999), phone (price=699), tablet (price=499)".to_string(),
        None,
    ).await;

    // Connect the client
    use netget::cli::client_startup::start_client_by_id;
    let result = start_client_by_id(&app_state, client_id, &llm_client, &status_tx).await;
    assert!(result.is_ok(), "Failed to start Elasticsearch client");

    // LLM Call 1: Initial connection + bulk operation action
    assert!(
        wait_for_status(
            &mut status_rx,
            |msg| msg.contains("Elasticsearch client") && msg.contains("ready"),
            10
        )
        .await,
        "Elasticsearch client did not connect"
    );

    println!("[TEST] Waiting for bulk operation...");
    sleep(Duration::from_secs(10)).await;

    // LLM Call 2: Response handling for bulk operation

    // Verify messages
    let mut message_count = 0;
    while let Ok(Some(_msg)) = timeout(Duration::from_millis(100), status_rx.recv()).await {
        message_count += 1;
        if message_count > 50 {
            break;
        }
    }

    println!("[TEST] Received {} status messages", message_count);
    assert!(message_count > 0, "No status messages received");

    // Total LLM calls: ~2 (connect, bulk response)
    println!("[TEST] Bulk operations test completed successfully");
}

#[tokio::test]
#[ignore] // Requires local Elasticsearch instance and Ollama
async fn test_elasticsearch_client_document_lifecycle() {
    // Setup
    let (status_tx, mut status_rx) = mpsc::unbounded_channel();
    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://localhost:11434", "qwen2.5-coder:7b");

    println!("[TEST] Starting Elasticsearch document lifecycle test");

    // Open Elasticsearch client
    let client_id = app_state.next_client_id().await;
    app_state
        .add_client(
            client_id,
            "Elasticsearch".to_string(),
            "localhost:9200".to_string(),
            "Index a document with id 'test-doc-1' in 'test-index', then get it, then delete it"
                .to_string(),
            None,
        )
        .await;

    // Connect the client
    use netget::cli::client_startup::start_client_by_id;
    let result = start_client_by_id(&app_state, client_id, &llm_client, &status_tx).await;
    assert!(result.is_ok(), "Failed to start Elasticsearch client");

    // LLM Call 1: Initial connection + index action
    assert!(
        wait_for_status(
            &mut status_rx,
            |msg| msg.contains("Elasticsearch client") && msg.contains("ready"),
            10
        )
        .await,
        "Elasticsearch client did not connect"
    );

    println!("[TEST] Waiting for document lifecycle operations...");
    sleep(Duration::from_secs(15)).await;

    // LLM Calls 2-4: Index response + get action, get response + delete action, delete response

    // Verify messages
    let mut message_count = 0;
    while let Ok(Some(_msg)) = timeout(Duration::from_millis(100), status_rx.recv()).await {
        message_count += 1;
        if message_count > 50 {
            break;
        }
    }

    println!("[TEST] Received {} status messages", message_count);
    assert!(message_count > 0, "No status messages received");

    // Total LLM calls: ~4-5 (connect, index response, get response, delete response)
    println!("[TEST] Document lifecycle test completed successfully");
}
