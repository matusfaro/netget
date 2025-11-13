//! Git client E2E tests
//!
//! Tests Git client protocol implementation with mocked LLM responses.
//! Uses test helper infrastructure for consistent testing.

#![cfg(all(test, feature = "git"))]

use crate::helpers::*;
use std::time::Duration;

/// Test Git clone operation with mocked LLM
#[tokio::test]
async fn test_git_clone() -> E2EResult<()> {
    println!("\n=== E2E Test: Git Client Clone ===");

    // Use a temporary directory path for the clone
    let temp_dir = tempfile::tempdir()?;
    let clone_path = temp_dir.path().join("test-repo");

    let prompt = format!(
        "Connect via Git client to https://github.com/rust-lang/rustlings.git and clone to {}",
        clone_path.display()
    );

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect via Git")
                .and_instruction_containing("clone")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": "https://github.com/rust-lang/rustlings.git",
                        "protocol": "Git",
                        "instruction": format!("Clone repository to {}", clone_path.display())
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected (git_connected event)
                .on_event("git_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "git_clone",
                        "target_path": clone_path.display().to_string()
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Clone completed (git_operation_complete event)
                .on_event("git_operation_complete")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let mut client = start_netget_client(config).await?;

    // Give client time to execute operations
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify client output shows clone activity
    assert!(
        client.output_contains("clone").await || client.output_contains("Git").await,
        "Client should show clone activity. Output: {:?}",
        client.get_output().await
    );

    println!("✅ Git client clone operation validated");

    // Verify mock expectations
    client.verify_mocks().await?;

    // Cleanup
    client.stop().await?;

    Ok(())
}

/// Test Git log operation with mocked LLM
#[tokio::test]
async fn test_git_log() -> E2EResult<()> {
    println!("\n=== E2E Test: Git Client Log ===");

    // Use a local path (tests git operations on existing repo)
    let temp_dir = tempfile::tempdir()?;
    let repo_path = temp_dir.path().join("test-repo");

    let prompt = format!(
        "Connect via Git client to {} and show the last 5 commits",
        repo_path.display()
    );

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup
                .on_instruction_containing("Connect via Git")
                .and_instruction_containing("commits")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": repo_path.display().to_string(),
                        "protocol": "Git",
                        "instruction": "Show last 5 commits"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected
                .on_event("git_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "git_log",
                        "limit": 5
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Operation complete
                .on_event("git_operation_complete")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let mut client = start_netget_client(config).await?;

    // Give client time to execute
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify client started
    assert_eq!(client.protocol, "Git", "Client should be Git protocol");

    println!("✅ Git client log operation validated");

    // Verify mocks
    client.verify_mocks().await?;

    // Cleanup
    client.stop().await?;

    Ok(())
}
