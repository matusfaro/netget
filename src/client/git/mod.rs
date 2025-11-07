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

use crate::client::git::actions::GIT_CLIENT_CONNECTED_EVENT;
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
        app_state: &Arc<AppState>,
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
                    "git_pull" => {
                        let remote_name = data
                            .get("remote")
                            .and_then(|v| v.as_str())
                            .unwrap_or("origin");
                        let branch = data
                            .get("branch")
                            .and_then(|v| v.as_str());

                        if let Some(ref path) = repo_path {
                            info!("Git client {} pulling from {}", client_id, remote_name);
                            match Self::git_pull(
                                path,
                                remote_name,
                                branch,
                                username.as_deref(),
                                password.as_deref(),
                            ) {
                                Ok(result) => {
                                    info!("Git client {} pull: {}", client_id, result);
                                }
                                Err(e) => {
                                    error!("Git client {} pull failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_push" => {
                        let remote_name = data
                            .get("remote")
                            .and_then(|v| v.as_str())
                            .unwrap_or("origin");
                        let branch = data
                            .get("branch")
                            .and_then(|v| v.as_str());

                        if let Some(ref path) = repo_path {
                            info!("Git client {} pushing to {}", client_id, remote_name);
                            match Self::git_push(
                                path,
                                remote_name,
                                branch,
                                username.as_deref(),
                                password.as_deref(),
                            ) {
                                Ok(result) => {
                                    info!("Git client {} push: {}", client_id, result);
                                }
                                Err(e) => {
                                    error!("Git client {} push failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_checkout" => {
                        let target = data
                            .get("target")
                            .and_then(|v| v.as_str())
                            .context("Missing 'target' field")?;
                        let create = data
                            .get("create")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        if let Some(ref path) = repo_path {
                            info!("Git client {} checking out {}", client_id, target);
                            match Self::git_checkout(path, target, create) {
                                Ok(result) => {
                                    info!("Git client {} checkout: {}", client_id, result);
                                }
                                Err(e) => {
                                    error!("Git client {} checkout failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_delete_branch" => {
                        let branch = data
                            .get("branch")
                            .and_then(|v| v.as_str())
                            .context("Missing 'branch' field")?;
                        let force = data
                            .get("force")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let remote = data
                            .get("remote")
                            .and_then(|v| v.as_str());

                        if let Some(ref path) = repo_path {
                            info!("Git client {} deleting branch {}", client_id, branch);
                            match Self::git_delete_branch(
                                path,
                                branch,
                                force,
                                remote,
                                username.as_deref(),
                                password.as_deref(),
                            ) {
                                Ok(result) => {
                                    info!("Git client {} delete branch: {}", client_id, result);
                                }
                                Err(e) => {
                                    error!("Git client {} delete branch failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_list_tags" => {
                        if let Some(ref path) = repo_path {
                            info!("Git client {} listing tags", client_id);
                            match Self::git_list_tags(path) {
                                Ok(tags) => {
                                    info!("Git client {} tags: {}", client_id, tags);
                                }
                                Err(e) => {
                                    error!("Git client {} list tags failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_create_tag" => {
                        let tag_name = data
                            .get("name")
                            .and_then(|v| v.as_str())
                            .context("Missing 'name' field")?;
                        let target = data
                            .get("target")
                            .and_then(|v| v.as_str());
                        let message = data
                            .get("message")
                            .and_then(|v| v.as_str());

                        if let Some(ref path) = repo_path {
                            info!("Git client {} creating tag {}", client_id, tag_name);
                            match Self::git_create_tag(path, tag_name, target, message) {
                                Ok(result) => {
                                    info!("Git client {} create tag: {}", client_id, result);
                                }
                                Err(e) => {
                                    error!("Git client {} create tag failed: {}", client_id, e);
                                }
                            }
                        }
                    }
                    "git_diff" => {
                        let target = data
                            .get("target")
                            .and_then(|v| v.as_str());
                        let staged = data
                            .get("staged")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        if let Some(ref path) = repo_path {
                            info!("Git client {} getting diff", client_id);
                            match Self::git_diff(path, target, staged) {
                                Ok(diff_text) => {
                                    info!("Git client {} diff: {}", client_id, diff_text);
                                }
                                Err(e) => {
                                    error!("Git client {} diff failed: {}", client_id, e);
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

    /// Pull updates from remote (fetch + merge)
    fn git_pull(
        path: &PathBuf,
        remote_name: &str,
        branch_name: Option<&str>,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<String> {
        let repo = Repository::open(path)?;

        // Get current branch if not specified
        let current_branch_name = if let Some(branch) = branch_name {
            branch.to_string()
        } else {
            let head = repo.head()?;
            head.shorthand()
                .context("Could not get current branch name")?
                .to_string()
        };

        // Fetch first
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

        remote.fetch(
            &[format!("refs/heads/{}:refs/remotes/{}/{}", current_branch_name, remote_name, current_branch_name)],
            Some(&mut fetch_options),
            None
        )?;

        // Now merge the fetched changes
        let fetch_head = repo.find_reference("FETCH_HEAD")?;
        let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

        // Perform the merge analysis
        let (analysis, _) = repo.merge_analysis(&[&fetch_commit])?;

        if analysis.is_up_to_date() {
            Ok("Already up to date".to_string())
        } else if analysis.is_fast_forward() {
            // Fast-forward merge
            let refname = format!("refs/heads/{}", current_branch_name);
            let mut reference = repo.find_reference(&refname)?;
            reference.set_target(fetch_commit.id(), "pull: Fast-forward")?;
            repo.set_head(&refname)?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
            Ok(format!("Fast-forward merge completed"))
        } else if analysis.is_normal() {
            // Normal merge (requires commit)
            Ok("Merge required but auto-merge not implemented. Please manually merge.".to_string())
        } else {
            Ok("Unknown merge analysis result".to_string())
        }
    }

    /// Push commits to remote
    fn git_push(
        path: &PathBuf,
        remote_name: &str,
        branch_name: Option<&str>,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<String> {
        let repo = Repository::open(path)?;

        // Get current branch if not specified
        let current_branch_name = if let Some(branch) = branch_name {
            branch.to_string()
        } else {
            let head = repo.head()?;
            head.shorthand()
                .context("Could not get current branch name")?
                .to_string()
        };

        let mut remote = repo.find_remote(remote_name)?;
        let mut callbacks = RemoteCallbacks::new();

        if let (Some(user), Some(pass)) = (username, password) {
            let user = user.to_string();
            let pass = pass.to_string();
            callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext(&user, &pass)
            });
        }

        let mut push_options = git2::PushOptions::new();
        push_options.remote_callbacks(callbacks);

        // Push the branch
        let refspec = format!("refs/heads/{}:refs/heads/{}", current_branch_name, current_branch_name);
        remote.push(&[&refspec], Some(&mut push_options))?;

        Ok(format!("Successfully pushed {} to {}", current_branch_name, remote_name))
    }

    /// Checkout a branch or create a new branch
    fn git_checkout(
        path: &PathBuf,
        target: &str,
        create: bool,
    ) -> Result<String> {
        let repo = Repository::open(path)?;

        if create {
            // Create and checkout new branch
            let head = repo.head()?;
            let oid = head.target().context("Could not get HEAD target")?;
            let commit = repo.find_commit(oid)?;

            repo.branch(target, &commit, false)?;

            let obj = repo.revparse_single(&format!("refs/heads/{}", target))?;
            repo.checkout_tree(&obj, None)?;
            repo.set_head(&format!("refs/heads/{}", target))?;

            Ok(format!("Created and checked out new branch: {}", target))
        } else {
            // Checkout existing branch or commit
            let obj = repo.revparse_single(target)?;
            repo.checkout_tree(&obj, None)?;

            // Try to set HEAD to the branch reference if it exists
            let refname = format!("refs/heads/{}", target);
            if repo.find_reference(&refname).is_ok() {
                repo.set_head(&refname)?;
                Ok(format!("Checked out branch: {}", target))
            } else {
                // Detached HEAD for commit
                repo.set_head_detached(obj.id())?;
                Ok(format!("Checked out commit: {} (detached HEAD)", target))
            }
        }
    }

    /// Delete a local or remote branch
    fn git_delete_branch(
        path: &PathBuf,
        branch_name: &str,
        force: bool,
        remote_name: Option<&str>,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<String> {
        let repo = Repository::open(path)?;
        let mut result_msgs = Vec::new();

        // Delete local branch if no remote specified, or always delete local
        if remote_name.is_none() {
            let mut branch = repo.find_branch(branch_name, git2::BranchType::Local)?;

            // Check if branch is fully merged (unless force is true)
            if !force {
                let head = repo.head()?;
                let head_commit = head.peel_to_commit()?;

                let branch_ref = branch.get();
                let branch_commit = branch_ref.peel_to_commit()?;

                // Check if branch is merged into HEAD
                let merge_base = repo.merge_base(head_commit.id(), branch_commit.id())?;
                if merge_base != branch_commit.id() {
                    anyhow::bail!("Branch '{}' is not fully merged. Use force=true to delete anyway.", branch_name);
                }
            }

            branch.delete()?;
            result_msgs.push(format!("Deleted local branch: {}", branch_name));
        }

        // Delete remote branch if specified
        if let Some(remote) = remote_name {
            let mut remote_obj = repo.find_remote(remote)?;

            let mut callbacks = RemoteCallbacks::new();
            if let (Some(user), Some(pass)) = (username, password) {
                let user = user.to_string();
                let pass = pass.to_string();
                callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                    Cred::userpass_plaintext(&user, &pass)
                });
            }

            let mut push_options = git2::PushOptions::new();
            push_options.remote_callbacks(callbacks);

            // Push empty refspec to delete remote branch
            let refspec = format!(":refs/heads/{}", branch_name);
            remote_obj.push(&[&refspec], Some(&mut push_options))?;

            result_msgs.push(format!("Deleted remote branch: {}/{}", remote, branch_name));
        }

        Ok(result_msgs.join("; "))
    }

    /// List all tags in the repository
    fn git_list_tags(path: &PathBuf) -> Result<String> {
        let repo = Repository::open(path)?;
        let tag_names = repo.tag_names(None)?;

        let mut tags = Vec::new();
        for tag_name in tag_names.iter() {
            if let Some(name) = tag_name {
                tags.push(name.to_string());
            }
        }

        if tags.is_empty() {
            Ok("No tags found".to_string())
        } else {
            Ok(format!("Tags ({}): {}", tags.len(), tags.join(", ")))
        }
    }

    /// Create a new tag
    fn git_create_tag(
        path: &PathBuf,
        tag_name: &str,
        target: Option<&str>,
        message: Option<&str>,
    ) -> Result<String> {
        let repo = Repository::open(path)?;

        // Resolve target (default to HEAD)
        let target_str = target.unwrap_or("HEAD");
        let obj = repo.revparse_single(target_str)?;
        let target_commit = obj.peel_to_commit()?;

        // Get git signature for annotated tags
        let sig = repo.signature().or_else(|_| {
            // Fallback signature if not configured
            git2::Signature::now("NetGet", "netget@localhost")
        })?;

        if let Some(msg) = message {
            // Create annotated tag
            repo.tag(tag_name, &obj, &sig, msg, false)?;
            Ok(format!("Created annotated tag '{}' at {} with message: {}", tag_name, target_commit.id(), msg))
        } else {
            // Create lightweight tag
            repo.tag_lightweight(tag_name, &obj, false)?;
            Ok(format!("Created lightweight tag '{}' at {}", tag_name, target_commit.id()))
        }
    }

    /// View differences in the repository
    fn git_diff(
        path: &PathBuf,
        target: Option<&str>,
        staged: bool,
    ) -> Result<String> {
        let repo = Repository::open(path)?;

        let diff = if staged {
            // Show staged changes (index vs HEAD)
            let head_tree = repo.head()?.peel_to_tree()?;
            let mut index = repo.index()?;
            let index_tree = repo.find_tree(index.write_tree()?)?;
            repo.diff_tree_to_tree(Some(&head_tree), Some(&index_tree), None)?
        } else if let Some(target_ref) = target {
            // Show diff against specific target
            let target_obj = repo.revparse_single(target_ref)?;
            let target_tree = target_obj.peel_to_tree()?;
            let head_tree = repo.head()?.peel_to_tree()?;
            repo.diff_tree_to_tree(Some(&target_tree), Some(&head_tree), None)?
        } else {
            // Show working directory changes (working dir vs index)
            repo.diff_index_to_workdir(None, None)?
        };

        // Format diff statistics
        let stats = diff.stats()?;
        let files_changed = stats.files_changed();
        let insertions = stats.insertions();
        let deletions = stats.deletions();

        // Get patch text
        let mut patch_text = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            let content = std::str::from_utf8(line.content()).unwrap_or("");

            match origin {
                '+' | '-' | ' ' => {
                    patch_text.push(origin);
                    patch_text.push_str(content);
                }
                _ => {
                    patch_text.push_str(content);
                }
            }
            true
        })?;

        if patch_text.is_empty() {
            Ok("No differences found".to_string())
        } else {
            Ok(format!(
                "Diff: {} file(s) changed, {} insertion(s), {} deletion(s)\n\n{}",
                files_changed, insertions, deletions,
                patch_text.lines().take(50).collect::<Vec<_>>().join("\n")
            ))
        }
    }
}
