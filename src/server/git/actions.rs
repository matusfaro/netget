//! Git Smart HTTP protocol actions
//!
//! Defines the action system for Git protocol server.
//! The LLM controls repository discovery, reference advertisement, and pack file generation.

use crate::llm::actions::protocol_trait::{ActionResult, Server};
use crate::llm::actions::{ActionDefinition, Parameter, ParameterDefinition};
use crate::protocol::{EventType, SpawnContext};
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;

/// Git Smart HTTP protocol implementation
#[derive(Clone)]
pub struct GitProtocol {
    _phantom: (),
}

impl GitProtocol {
    /// Create a new Git protocol instance
    pub fn new() -> Self {
        Self { _phantom: () }
    }
}

impl Server for GitProtocol {
    fn spawn(&self, ctx: SpawnContext) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::server::git::GitServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                false,
                ctx.server_id,
            )
            .await
        })
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "default_branch".to_string(),
                type_hint: "string".to_string(),
                description: "Default branch name for repositories (e.g., 'main', 'master')"
                    .to_string(),
                required: false,
                example: json!("main"),
            },
            ParameterDefinition {
                name: "allow_push".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether to allow push operations (true/false)".to_string(),
                required: false,
                example: json!(false),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "create_git_repository".to_string(),
                description: "Create a new virtual Git repository".to_string(),
                parameters: vec![
                    Parameter {
                        name: "name".to_string(),
                        type_hint: "string".to_string(),
                        description: "Repository name (e.g., 'my-project')".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "description".to_string(),
                        type_hint: "string".to_string(),
                        description: "Repository description".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "default_branch".to_string(),
                        type_hint: "string".to_string(),
                        description: "Default branch name".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "create_git_repository",
                    "name": "my-project",
                    "description": "My project",
                    "default_branch": "main"
                }),
            },
            ActionDefinition {
                name: "delete_git_repository".to_string(),
                description: "Delete a virtual Git repository".to_string(),
                parameters: vec![Parameter {
                    name: "name".to_string(),
                    type_hint: "string".to_string(),
                    description: "Repository name to delete".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "delete_git_repository",
                    "name": "old-project"
                }),
            },
            ActionDefinition {
                name: "list_git_repositories".to_string(),
                description: "List all virtual Git repositories".to_string(),
                parameters: vec![],
                example: json!({"type": "list_git_repositories"}),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "git_advertise_refs".to_string(),
                description: "Advertise Git references (branches, tags) for a repository"
                    .to_string(),
                parameters: vec![
                    Parameter {
                        name: "refs".to_string(),
                        type_hint: "array".to_string(),
                        description: "Array of reference objects with 'name' and 'sha' fields"
                            .to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "capabilities".to_string(),
                        type_hint: "array".to_string(),
                        description: "Array of Git capabilities to advertise".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "git_advertise_refs",
                    "refs": [{"name": "refs/heads/main", "sha": "abc123"}],
                    "capabilities": ["multi_ack"]
                }),
            },
            ActionDefinition {
                name: "git_send_pack".to_string(),
                description: "Send a Git pack file for clone/fetch operations".to_string(),
                parameters: vec![Parameter {
                    name: "pack_data".to_string(),
                    type_hint: "string".to_string(),
                    description: "Base64-encoded pack file data".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "git_send_pack",
                    "pack_data": "PACK..."
                }),
            },
            ActionDefinition {
                name: "git_error".to_string(),
                description: "Send a Git protocol error response".to_string(),
                parameters: vec![
                    Parameter {
                        name: "message".to_string(),
                        type_hint: "string".to_string(),
                        description: "Error message".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "code".to_string(),
                        type_hint: "number".to_string(),
                        description: "HTTP status code".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "git_error",
                    "message": "Repository not found",
                    "code": 404
                }),
            },
        ]
    }

    fn execute_action(&self, action: Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "create_git_repository" => {
                let name = action
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing repository name"))?;

                // For now, just log the action
                // In a real implementation, we'd store repository metadata in AppState
                Ok(ActionResult::Custom {
                    name: "git_repository_created".to_string(),
                    data: serde_json::json!({
                        "repository": name,
                        "success": true
                    }),
                })
            }
            "delete_git_repository" => {
                let name = action
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing repository name"))?;

                Ok(ActionResult::Custom {
                    name: "git_repository_deleted".to_string(),
                    data: serde_json::json!({
                        "repository": name,
                        "success": true
                    }),
                })
            }
            "list_git_repositories" => Ok(ActionResult::Custom {
                name: "git_repositories_listed".to_string(),
                data: serde_json::json!({
                    "repositories": [],
                    "success": true
                }),
            }),
            "git_advertise_refs" => {
                let refs = action.get("refs").ok_or_else(|| anyhow!("Missing refs"))?;

                Ok(ActionResult::Custom {
                    name: "git_refs_response".to_string(),
                    data: serde_json::json!({
                        "refs": refs,
                        "capabilities": action.get("capabilities").unwrap_or(&serde_json::json!([]))
                    }),
                })
            }
            "git_send_pack" => {
                let pack_data = action
                    .get("pack_data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing pack_data"))?;

                Ok(ActionResult::Custom {
                    name: "git_pack_response".to_string(),
                    data: serde_json::json!({
                        "pack_data": pack_data
                    }),
                })
            }
            "git_error" => {
                let message = action
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing error message"))?;
                let code = action.get("code").and_then(|v| v.as_u64()).unwrap_or(500);

                Ok(ActionResult::Custom {
                    name: "git_error_response".to_string(),
                    data: serde_json::json!({
                        "message": message,
                        "code": code
                    }),
                })
            }
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Git"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        // Event types define the triggers for LLM calls or script execution
        // For now, returning empty - Git protocol uses simple request-response pattern
        vec![]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>Git"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["git", "git server", "via git"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual Git Smart HTTP (pkt-line format), hyper")
            .llm_control("References, pack files, repository discovery")
            .e2e_testing("git clone / git fetch")
            .notes("Read-only (clone/fetch), virtual repositories, no push")
            .build()
    }

    fn description(&self) -> &'static str {
        "Git Smart HTTP server for serving virtual repositories"
    }

    fn example_prompt(&self) -> &'static str {
        "listen on port 9418 via git. Create repository 'hello-world' with main branch. README.md contains: '# Hello World'"
    }

    fn group_name(&self) -> &'static str {
        "Web & File"
    }
}
