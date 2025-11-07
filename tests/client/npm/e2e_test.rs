//! End-to-end tests for NPM Registry client
//!
//! These tests verify that the NPM client can:
//! 1. Search for packages
//! 2. Get package information
//! 3. Download package tarballs
//!
//! Target: < 10 LLM calls per test suite
//! Runtime: ~60 seconds

#![cfg(all(test, feature = "npm"))]

use netget::client::npm::NpmClient;
use netget::llm::OllamaClient;
use netget::state::app_state::AppState;
use netget::state::{ClientId, ClientStatus};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Helper to create test environment
async fn setup_test() -> (Arc<AppState>, OllamaClient, mpsc::UnboundedSender<String>) {
    let state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new("http://localhost:11434".to_string());
    let (status_tx, _status_rx) = mpsc::unbounded_channel();

    (state, llm_client, status_tx)
}

#[tokio::test]
#[ignore] // Requires Ollama and network access
async fn test_npm_client_get_package_info() {
    let (app_state, llm_client, status_tx) = setup_test().await;

    // Register client
    let client_id = ClientId::new(1);
    app_state.register_client(
        client_id,
        "NPM".to_string(),
        "https://registry.npmjs.org".to_string(),
        "Get information about the lodash package".to_string(),
        None,
    ).await;

    // Connect to NPM registry
    let result = NpmClient::connect_with_llm_actions(
        "https://registry.npmjs.org".to_string(),
        llm_client.clone(),
        app_state.clone(),
        status_tx.clone(),
        client_id,
    ).await;

    assert!(result.is_ok(), "Failed to connect NPM client: {:?}", result);

    // Verify client is connected
    let client = app_state.get_client(client_id).await;
    assert!(client.is_some(), "Client not found in state");
    assert_eq!(client.unwrap().status, ClientStatus::Connected);

    // Get package info
    let get_result = NpmClient::get_package_info(
        client_id,
        "lodash".to_string(),
        "latest".to_string(),
        app_state.clone(),
        llm_client.clone(),
        status_tx.clone(),
    ).await;

    assert!(get_result.is_ok(), "Failed to get package info: {:?}", get_result);
}

#[tokio::test]
#[ignore] // Requires Ollama and network access
async fn test_npm_client_search_packages() {
    let (app_state, llm_client, status_tx) = setup_test().await;

    // Register client
    let client_id = ClientId::new(2);
    app_state.register_client(
        client_id,
        "NPM".to_string(),
        "https://registry.npmjs.org".to_string(),
        "Search for http server packages".to_string(),
        None,
    ).await;

    // Connect to NPM registry
    let result = NpmClient::connect_with_llm_actions(
        "https://registry.npmjs.org".to_string(),
        llm_client.clone(),
        app_state.clone(),
        status_tx.clone(),
        client_id,
    ).await;

    assert!(result.is_ok(), "Failed to connect NPM client: {:?}", result);

    // Search for packages
    let search_result = NpmClient::search_packages(
        client_id,
        "http server".to_string(),
        10,
        app_state.clone(),
        llm_client.clone(),
        status_tx.clone(),
    ).await;

    assert!(search_result.is_ok(), "Failed to search packages: {:?}", search_result);
}

#[tokio::test]
#[ignore] // Requires Ollama, network access, and writes to filesystem
async fn test_npm_client_download_tarball() {
    let (app_state, llm_client, status_tx) = setup_test().await;

    // Register client
    let client_id = ClientId::new(3);
    app_state.register_client(
        client_id,
        "NPM".to_string(),
        "https://registry.npmjs.org".to_string(),
        "Download the latest lodash package".to_string(),
        None,
    ).await;

    // Connect to NPM registry
    let result = NpmClient::connect_with_llm_actions(
        "https://registry.npmjs.org".to_string(),
        llm_client.clone(),
        app_state.clone(),
        status_tx.clone(),
        client_id,
    ).await;

    assert!(result.is_ok(), "Failed to connect NPM client: {:?}", result);

    // Download tarball to temp directory
    let output_path = std::env::temp_dir().join("lodash-test.tgz");
    let output_path_str = output_path.to_str().unwrap().to_string();

    let download_result = NpmClient::download_tarball(
        client_id,
        "lodash".to_string(),
        "latest".to_string(),
        output_path_str.clone(),
        app_state.clone(),
        status_tx.clone(),
    ).await;

    assert!(download_result.is_ok(), "Failed to download tarball: {:?}", download_result);

    // Verify file exists
    assert!(output_path.exists(), "Tarball file was not created");

    // Verify file has content
    let metadata = std::fs::metadata(&output_path).unwrap();
    assert!(metadata.len() > 0, "Tarball file is empty");

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

#[tokio::test]
#[ignore] // Requires Ollama and network access
async fn test_npm_client_scoped_package() {
    let (app_state, llm_client, status_tx) = setup_test().await;

    // Register client
    let client_id = ClientId::new(4);
    app_state.register_client(
        client_id,
        "NPM".to_string(),
        "https://registry.npmjs.org".to_string(),
        "Get information about @types/node package".to_string(),
        None,
    ).await;

    // Connect to NPM registry
    let result = NpmClient::connect_with_llm_actions(
        "https://registry.npmjs.org".to_string(),
        llm_client.clone(),
        app_state.clone(),
        status_tx.clone(),
        client_id,
    ).await;

    assert!(result.is_ok(), "Failed to connect NPM client: {:?}", result);

    // Get info for scoped package
    let get_result = NpmClient::get_package_info(
        client_id,
        "@types/node".to_string(),
        "latest".to_string(),
        app_state.clone(),
        llm_client.clone(),
        status_tx.clone(),
    ).await;

    assert!(get_result.is_ok(), "Failed to get scoped package info: {:?}", get_result);
}
