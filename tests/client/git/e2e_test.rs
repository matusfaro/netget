//! Git client E2E tests
//!
//! Tests Git client protocol implementation with real Git operations.
//! Uses public GitHub repositories for testing (no authentication required).

#![cfg(all(test, feature = "git"))]

use anyhow::Result;
use netget::cli::CliArgs;
use netget::llm::ollama_client::OllamaClient;
use netget::protocol::Event;
use netget::state::app_state::AppState;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

/// Helper function to create test Git client
async fn setup_git_client() -> Result<(Arc<AppState>, OllamaClient, mpsc::UnboundedSender<String>)>
{
    let args = CliArgs {
        model: "qwen3-coder:30b".to_string(),
        ollama_host: "http://localhost:11434".to_string(),
        ollama_lock: true,
        ..Default::default()
    };

    let app_state = Arc::new(AppState::new());
    let llm_client = OllamaClient::new(&args.ollama_host, &args.model);
    let (status_tx, _status_rx) = mpsc::unbounded_channel();

    Ok((app_state, llm_client, status_tx))
}

/// Test Git clone operation with LLM
#[tokio::test]
#[ignore] // Ignore by default, run with --ignored flag
async fn test_git_clone() -> Result<()> {
    use netget::client::git::GitClientProtocol;
    use netget::llm::actions::client_trait::Client;
    use netget::protocol::ConnectContext;
    use tempfile::TempDir;

    let (app_state, llm_client, status_tx) = setup_git_client().await?;

    // Create temporary directory for clone
    let temp_dir = TempDir::new()?;
    let clone_path = temp_dir.path().join("test-repo");

    // Create client ID
    let client_id = app_state.generate_client_id().await;

    // Set instruction
    let instruction = format!(
        "Clone the repository https://github.com/rust-lang/rustlings.git to {}",
        clone_path.display()
    );
    app_state
        .set_instruction_for_client(client_id, instruction.clone())
        .await;

    // Initialize Git client protocol
    let protocol = GitClientProtocol::new();

    // Connect (this will trigger LLM and start operations)
    let ctx = ConnectContext {
        remote_addr: "https://github.com/rust-lang/rustlings.git".to_string(),
        llm_client: llm_client.clone(),
        state: app_state.clone(),
        status_tx: status_tx.clone(),
        client_id,
    };

    let result = protocol.connect(ctx).await;
    assert!(
        result.is_ok(),
        "Git client connection failed: {:?}",
        result.err()
    );

    // Wait for clone operation to complete
    sleep(Duration::from_secs(30)).await;

    // Verify repository was cloned
    assert!(clone_path.exists(), "Clone path does not exist");
    assert!(
        clone_path.join(".git").exists(),
        "Not a valid Git repository"
    );

    Ok(())
}

/// Test Git list branches operation
#[tokio::test]
#[ignore] // Ignore by default
async fn test_git_list_branches() -> Result<()> {
    use netget::client::git::GitClientProtocol;
    use netget::llm::actions::client_trait::Client;
    use netget::protocol::ConnectContext;
    use tempfile::TempDir;

    let (app_state, llm_client, status_tx) = setup_git_client().await?;

    // Clone a small repository first
    let temp_dir = TempDir::new()?;
    let clone_path = temp_dir.path().join("test-repo");

    // Use git2 directly to clone (for test setup)
    use git2::Repository;
    let repo = Repository::clone("https://github.com/rust-lang/rustlings.git", &clone_path)?;

    // Create client ID
    let client_id = app_state.generate_client_id().await;

    // Set instruction to list branches
    let instruction = "List all branches in the repository, including remote branches";
    app_state
        .set_instruction_for_client(client_id, instruction.to_string())
        .await;

    // Initialize Git client protocol
    let protocol = GitClientProtocol::new();

    // Connect
    let ctx = ConnectContext {
        remote_addr: clone_path.to_string_lossy().to_string(),
        llm_client: llm_client.clone(),
        state: app_state.clone(),
        status_tx: status_tx.clone(),
        client_id,
    };

    let result = protocol.connect(ctx).await;
    assert!(
        result.is_ok(),
        "Git client connection failed: {:?}",
        result.err()
    );

    // Wait for operation to complete
    sleep(Duration::from_secs(10)).await;

    Ok(())
}

/// Test Git log operation
#[tokio::test]
#[ignore] // Ignore by default
async fn test_git_log() -> Result<()> {
    use netget::client::git::GitClientProtocol;
    use netget::llm::actions::client_trait::Client;
    use netget::protocol::ConnectContext;
    use tempfile::TempDir;

    let (app_state, llm_client, status_tx) = setup_git_client().await?;

    // Clone a small repository first
    let temp_dir = TempDir::new()?;
    let clone_path = temp_dir.path().join("test-repo");

    // Use git2 directly to clone (for test setup)
    use git2::Repository;
    let repo = Repository::clone("https://github.com/rust-lang/rustlings.git", &clone_path)?;

    // Create client ID
    let client_id = app_state.generate_client_id().await;

    // Set instruction to show log
    let instruction = "Show me the last 5 commits in the repository";
    app_state
        .set_instruction_for_client(client_id, instruction.to_string())
        .await;

    // Initialize Git client protocol
    let protocol = GitClientProtocol::new();

    // Connect
    let ctx = ConnectContext {
        remote_addr: clone_path.to_string_lossy().to_string(),
        llm_client: llm_client.clone(),
        state: app_state.clone(),
        status_tx: status_tx.clone(),
        client_id,
    };

    let result = protocol.connect(ctx).await;
    assert!(
        result.is_ok(),
        "Git client connection failed: {:?}",
        result.err()
    );

    // Wait for operation to complete
    sleep(Duration::from_secs(10)).await;

    Ok(())
}

/// Test Git status operation
#[tokio::test]
#[ignore] // Ignore by default
async fn test_git_status() -> Result<()> {
    use netget::client::git::GitClientProtocol;
    use netget::llm::actions::client_trait::Client;
    use netget::protocol::ConnectContext;
    use tempfile::TempDir;

    let (app_state, llm_client, status_tx) = setup_git_client().await?;

    // Clone a small repository first
    let temp_dir = TempDir::new()?;
    let clone_path = temp_dir.path().join("test-repo");

    // Use git2 directly to clone (for test setup)
    use git2::Repository;
    let repo = Repository::clone("https://github.com/rust-lang/rustlings.git", &clone_path)?;

    // Create client ID
    let client_id = app_state.generate_client_id().await;

    // Set instruction to check status
    let instruction = "Check the repository status";
    app_state
        .set_instruction_for_client(client_id, instruction.to_string())
        .await;

    // Initialize Git client protocol
    let protocol = GitClientProtocol::new();

    // Connect
    let ctx = ConnectContext {
        remote_addr: clone_path.to_string_lossy().to_string(),
        llm_client: llm_client.clone(),
        state: app_state.clone(),
        status_tx: status_tx.clone(),
        client_id,
    };

    let result = protocol.connect(ctx).await;
    assert!(
        result.is_ok(),
        "Git client connection failed: {:?}",
        result.err()
    );

    // Wait for operation to complete
    sleep(Duration::from_secs(10)).await;

    Ok(())
}
