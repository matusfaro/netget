//! Mercurial HTTP protocol actions
//!
//! Defines the action system for Mercurial protocol server.
//! The LLM controls repository discovery, capabilities, branch information, and bundle generation.

use crate::llm::actions::protocol_trait::{ActionResult, Protocol, Server};
use crate::llm::actions::{ActionDefinition, Parameter, ParameterDefinition};
use crate::protocol::{EventType, SpawnContext};
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;

/// Mercurial HTTP protocol implementation
#[derive(Clone)]
pub struct MercurialProtocol {
    _phantom: (),
}

impl MercurialProtocol {
    /// Create a new Mercurial protocol instance
    pub fn new() -> Self {
        Self { _phantom: () }
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for MercurialProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "default_branch".to_string(),
                type_hint: "string".to_string(),
                description: "Default branch name for repositories (e.g., 'default', 'stable')"
                    .to_string(),
                required: false,
                example: json!("default"),
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
                name: "create_hg_repository".to_string(),
                description: "Create a new virtual Mercurial repository".to_string(),
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
                    "type": "create_hg_repository",
                    "name": "my-project",
                    "description": "My Mercurial project",
                    "default_branch": "default"
                }),
            },
            ActionDefinition {
                name: "delete_hg_repository".to_string(),
                description: "Delete a virtual Mercurial repository".to_string(),
                parameters: vec![Parameter {
                    name: "name".to_string(),
                    type_hint: "string".to_string(),
                    description: "Repository name to delete".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "delete_hg_repository",
                    "name": "old-project"
                }),
            },
            ActionDefinition {
                name: "list_hg_repositories".to_string(),
                description: "List all virtual Mercurial repositories".to_string(),
                parameters: vec![],
                example: json!({"type": "list_hg_repositories"}),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "hg_capabilities".to_string(),
                description: "Advertise Mercurial server capabilities".to_string(),
                parameters: vec![Parameter {
                    name: "capabilities".to_string(),
                    type_hint: "array".to_string(),
                    description: "Array of capability strings".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "hg_capabilities",
                    "capabilities": ["batch", "branchmap", "getbundle", "httpheader=1024", "known", "lookup", "pushkey", "unbundle=HG10GZ,HG10BZ,HG10UN"]
                }),
            },
            ActionDefinition {
                name: "hg_heads".to_string(),
                description: "Provide repository heads (changeset node IDs)".to_string(),
                parameters: vec![Parameter {
                    name: "heads".to_string(),
                    type_hint: "array".to_string(),
                    description: "Array of 40-character hex node IDs".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "hg_heads",
                    "heads": ["abc123...", "def456..."]
                }),
            },
            ActionDefinition {
                name: "hg_branchmap".to_string(),
                description: "Provide branch name to node ID mappings".to_string(),
                parameters: vec![Parameter {
                    name: "branches".to_string(),
                    type_hint: "object".to_string(),
                    description: "Object mapping branch names to arrays of node IDs".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "hg_branchmap",
                    "branches": {
                        "default": ["abc123..."],
                        "stable": ["def456..."]
                    }
                }),
            },
            ActionDefinition {
                name: "hg_listkeys".to_string(),
                description: "Provide key-value mappings for a namespace (bookmarks, tags, etc.)"
                    .to_string(),
                parameters: vec![Parameter {
                    name: "keys".to_string(),
                    type_hint: "object".to_string(),
                    description: "Object mapping keys to values".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "hg_listkeys",
                    "keys": {
                        "master": "abc123...",
                        "develop": "def456..."
                    }
                }),
            },
            ActionDefinition {
                name: "hg_send_bundle".to_string(),
                description: "Send a Mercurial bundle (changegroup) for clone/pull operations"
                    .to_string(),
                parameters: vec![
                    Parameter {
                        name: "bundle_type".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bundle type: HG10UN, HG10GZ, HG10BZ".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "bundle_data".to_string(),
                        type_hint: "string".to_string(),
                        description: "Bundle data (empty string for empty bundle)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "hg_send_bundle",
                    "bundle_type": "HG10UN",
                    "bundle_data": ""
                }),
            },
            ActionDefinition {
                name: "hg_error".to_string(),
                description: "Send a Mercurial protocol error response".to_string(),
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
                    "type": "hg_error",
                    "message": "Repository not found",
                    "code": 404
                }),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Mercurial"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        // Event types define the triggers for LLM calls or script execution
        // For now, returning empty - Mercurial protocol uses simple request-response pattern
        vec![]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>Mercurial"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["mercurial", "hg", "hg server", "via mercurial", "via hg"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual Mercurial HTTP wire protocol, hyper")
            .llm_control("Capabilities, heads, branches, bookmarks, bundle generation")
            .e2e_testing("hg clone / hg pull")
            .notes("Read-only (clone/pull), virtual repositories, no push")
            .build()
    }
    fn description(&self) -> &'static str {
        "Mercurial HTTP server for serving virtual repositories"
    }
    fn example_prompt(&self) -> &'static str {
        "listen on port 8000 via mercurial. Create repository 'hello-world' with default branch."
    }
    fn group_name(&self) -> &'static str {
        "Web & File"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for MercurialProtocol {
    fn spawn(&self, ctx: SpawnContext) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::server::mercurial::MercurialServer::spawn_with_llm_actions(
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
    fn execute_action(&self, action: Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "create_hg_repository" => {
                let name = action
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing repository name"))?;

                // For now, just log the action
                // In a real implementation, we'd store repository metadata in AppState
                Ok(ActionResult::Custom {
                    name: "hg_repository_created".to_string(),
                    data: serde_json::json!({
                        "repository": name,
                        "success": true
                    }),
                })
            }
            "delete_hg_repository" => {
                let name = action
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing repository name"))?;

                Ok(ActionResult::Custom {
                    name: "hg_repository_deleted".to_string(),
                    data: serde_json::json!({
                        "repository": name,
                        "success": true
                    }),
                })
            }
            "list_hg_repositories" => Ok(ActionResult::Custom {
                name: "hg_repositories_listed".to_string(),
                data: serde_json::json!({
                    "repositories": [],
                    "success": true
                }),
            }),
            "hg_capabilities" => {
                let capabilities = action
                    .get("capabilities")
                    .ok_or_else(|| anyhow!("Missing capabilities"))?;

                Ok(ActionResult::Custom {
                    name: "hg_capabilities_response".to_string(),
                    data: serde_json::json!({
                        "capabilities": capabilities
                    }),
                })
            }
            "hg_heads" => {
                let heads = action
                    .get("heads")
                    .ok_or_else(|| anyhow!("Missing heads"))?;

                Ok(ActionResult::Custom {
                    name: "hg_heads_response".to_string(),
                    data: serde_json::json!({
                        "heads": heads
                    }),
                })
            }
            "hg_branchmap" => {
                let branches = action
                    .get("branches")
                    .ok_or_else(|| anyhow!("Missing branches"))?;

                Ok(ActionResult::Custom {
                    name: "hg_branchmap_response".to_string(),
                    data: serde_json::json!({
                        "branches": branches
                    }),
                })
            }
            "hg_listkeys" => {
                let keys = action.get("keys").ok_or_else(|| anyhow!("Missing keys"))?;

                Ok(ActionResult::Custom {
                    name: "hg_listkeys_response".to_string(),
                    data: serde_json::json!({
                        "keys": keys
                    }),
                })
            }
            "hg_send_bundle" => {
                let bundle_data = action
                    .get("bundle_data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing bundle_data"))?;

                Ok(ActionResult::Custom {
                    name: "hg_bundle_response".to_string(),
                    data: serde_json::json!({
                        "bundle_type": action.get("bundle_type").and_then(|v| v.as_str()).unwrap_or("HG10UN"),
                        "bundle_data": bundle_data
                    }),
                })
            }
            "hg_error" => {
                let message = action
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing error message"))?;
                let code = action.get("code").and_then(|v| v.as_u64()).unwrap_or(500);

                Ok(ActionResult::Custom {
                    name: "hg_error_response".to_string(),
                    data: serde_json::json!({
                        "message": message,
                        "code": code
                    }),
                })
            }
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }
}
