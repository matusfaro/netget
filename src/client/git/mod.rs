//! Git client implementation
pub mod actions;

pub use actions::GitClientProtocol;

use anyhow::{Context, Result};
use git2::{
    BranchType, Cred, FetchOptions, ObjectType, RemoteCallbacks, Repository,
    StatusOptions,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::client::git::actions::{
    GIT_CLIENT_CONNECTED_EVENT, GIT_OPERATION_COMPLETED_EVENT, GIT_OPERATION_ERROR_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// Git client that performs Git operations
pub struct GitClient;

impl GitClient {
    /// Connect (initialize) a Git client with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For Git, remote_addr can be either:
        // 1. A repository URL (for cloning)
        // 2. A local path (for existing repo)
        // We'll determine this based on the instruction

        info!("Git client {} initializing with target: {}", client_id, remote_addr);

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] Git client {} initialized", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Create a dummy socket address since Git doesn't use network sockets
        // We use a placeholder address to satisfy the return type
        let dummy_addr: SocketAddr = "127.0.0.1:0".parse()?;

        // Get initial instruction
        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_default();

        // Spawn task to handle LLM-driven Git operations
        tokio::spawn(async move {
            let protocol = Arc::new(GitClientProtocol::new());
            let mut repo_path: Option<PathBuf> = None;
            let mut username: Option<String> = None;
            let mut password: Option<String> = None;

            // Send connected event
            let event = Event::new(
                &GIT_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "repository_path": remote_addr,
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            // Initial LLM call
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute initial actions
                    for action in actions {
                        if let Err(e) = Self::execute_git_action(
                            &action,
                            &protocol,
                            &mut repo_path,
                            &mut username,
                            &mut password,
                            client_id,
                            &llm_client,
                            &app_state,
                            &status_tx,
                        )
                        .await
                        {
                            error!("Git client {} action error: {}", client_id, e);
                            let _ = status_tx.send(format!(
                                "[CLIENT] Git client {} error: {}",
                                client_id, e
                            ));
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for Git client {}: {}", client_id, e);
                    app_state
                        .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                        .await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                }
            }

            // Git client doesn't have a persistent connection, so we just mark it as done
            debug!("Git client {} operations completed", client_id);
        });

        Ok(dummy_addr)
    }

    /// Execute a Git action based on LLM decision
    async fn execute_git_action(
        action: &serde_json::Value,
        protocol: &Arc<GitClientProtocol>,
        repo_path: &mut Option<PathBuf>,
        username: &mut Option<String>,
        password: &mut Option<String>,
        client_id: ClientId,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match protocol.execute_action(action.clone())? {
            crate::llm::actions::client_trait::ClientActionResult::Custom { name, data } => {
                match name.as_str() {
                    "git_clone" => {
                        let url = data
                            .get("url")
                            .and_then(|v| v.as_str())
                            .context("Missing url")?;
                        let path = data
                            .get("path")
                            .and_then(|v| v.as_str())
                            .context("Missing path")?;

                        info!("Git client {} cloning {} to {}", client_id, url, path);
                        let _ = status_tx.send(format!(
                            "[CLIENT] Git client {} cloning {} to {}",
                            client_id, url, path
                        ));

                        match Self::git_clone(url, path, username.as_deref(), password.as_deref())
                        {
                            Ok(_repo) => {
                                *repo_path = Some(PathBuf::from(path));
                                info!("Git client {} clone successful", client_id);
                                let _ = status_tx.send(format!(
                                    "[CLIENT] Git client {} clone successful",
                                    client_id
                                ));
                            }
                            Err(e) => {
                                error!("Git client {} clone failed: {}", client_id, e);
                                let _ = status_tx.send(format!(
                                    "[CLIENT] Git client {} clone failed: {}",
                                    client_id, e
                                ));
                            }
                        }
                    }
                    "git_fetch" => {
                        let remote_name = data
                            .get("remote")
                            .and_then(|v| v.as_str())
                            .unwrap_or("origin");

                        if let Some(ref path) = repo_path {
                            info!(
                                "Git client {} fetching from remote {}",
                                client_id, remote_name
                            );
                            match Self::git_fetch(
                                path,
                                remote_name,
                                username.as_deref(),
                                password.as_deref(),
                            ) {
                                Ok(_) => {
                                    info!("Git client {} fetch successful", client_id);
                                }
                                Err(e) => {
                                    error!("Git client {} fetch failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_status" => {
                        if let Some(ref path) = repo_path {
                            info!("Git client {} getting status", client_id);
                            match Self::git_status(path) {
                                Ok(status_text) => {
                                    info!("Git client {} status: {}", client_id, status_text);
                                }
                                Err(e) => {
                                    error!("Git client {} status failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_list_branches" => {
                        let include_remote = data
                            .get("remote")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        if let Some(ref path) = repo_path {
                            info!("Git client {} listing branches", client_id);
                            match Self::git_list_branches(path, include_remote) {
                                Ok(branches) => {
                                    info!("Git client {} branches: {}", client_id, branches.join(", "));
                                }
                                Err(e) => {
                                    error!("Git client {} list branches failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_log" => {
                        let max_count = data
                            .get("max_count")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(10) as usize;

                        if let Some(ref path) = repo_path {
                            info!("Git client {} getting log (max {})", client_id, max_count);
                            match Self::git_log(path, max_count) {
                                Ok(log_text) => {
                                    info!("Git client {} log retrieved", client_id);
                                    debug!("Log:\n{}", log_text);
                                }
                                Err(e) => {
                                    error!("Git client {} log failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    _ => {
                        debug!("Unhandled Git action: {}", name);
                    }
                }
            }
            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                info!("Git client {} disconnecting", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            _ => {}
        }

        Ok(())
    }


    /// Clone a Git repository
    fn git_clone(
        url: &str,
        path: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Repository> {
        let mut callbacks = RemoteCallbacks::new();

        // Set up authentication callback
        if let (Some(user), Some(pass)) = (username, password) {
            let user = user.to_string();
            let pass = pass.to_string();
            callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext(&user, &pass)
            });
        }

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_options);

        let repo = builder.clone(url, std::path::Path::new(path))?;
        Ok(repo)
    }

    /// Fetch from a remote
    fn git_fetch(
        path: &PathBuf,
        remote_name: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        let repo = Repository::open(path)?;
        let mut remote = repo.find_remote(remote_name)?;

        let mut callbacks = RemoteCallbacks::new();
        if let (Some(user), Some(pass)) = (username, password) {
            let user = user.to_string();
            let pass = pass.to_string();
            callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext(&user, &pass)
            });
        }

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        remote.fetch(&["refs/heads/*:refs/remotes/origin/*"], Some(&mut fetch_options), None)?;
        Ok(())
    }

    /// Get repository status
    fn git_status(path: &PathBuf) -> Result<String> {
        let repo = Repository::open(path)?;
        let statuses = repo.statuses(Some(StatusOptions::new().include_untracked(true)))?;

        let mut result = String::new();
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                let status = entry.status();
                result.push_str(&format!(
                    "{:?} - {}\n",
                    status,
                    path
                ));
            }
        }

        if result.is_empty() {
            result = "Working tree clean".to_string();
        }

        Ok(result)
    }

    /// List branches
    fn git_list_branches(path: &PathBuf, include_remote: bool) -> Result<Vec<String>> {
        let repo = Repository::open(path)?;
        let mut branches = Vec::new();

        let local_branches = repo.branches(Some(BranchType::Local))?;
        for branch in local_branches {
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
                branches.push(name.to_string());
            }
        }

        if include_remote {
            let remote_branches = repo.branches(Some(BranchType::Remote))?;
            for branch in remote_branches {
                let (branch, _) = branch?;
                if let Some(name) = branch.name()? {
                    branches.push(name.to_string());
                }
            }
        }

        Ok(branches)
    }

    /// Get commit log
    fn git_log(path: &PathBuf, max_count: usize) -> Result<String> {
        let repo = Repository::open(path)?;
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;

        let mut result = String::new();
        let mut count = 0;

        for oid in revwalk {
            if count >= max_count {
                break;
            }

            let oid = oid?;
            let commit = repo.find_object(oid, Some(ObjectType::Commit))?;
            let commit = commit.as_commit().context("Not a commit")?;

            let time = commit.time();
            let datetime = chrono::DateTime::from_timestamp(time.seconds(), 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default();

            result.push_str(&format!(
                "commit {}\nAuthor: {}\nDate: {}\n\n    {}\n\n",
                oid,
                commit.author(),
                datetime.format("%Y-%m-%d %H:%M:%S"),
                commit.message().unwrap_or("")
            ));

            count += 1;
        }

        Ok(result)
    }
}
