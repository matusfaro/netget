//! NPM protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

// NPM event type constants (matching IDs used in get_npm_event_types)
pub static NPM_PACKAGE_REQUEST: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "NPM_PACKAGE_REQUEST",
        "NPM client requests package metadata",
        json!({
            "type": "npm_package_metadata",
            "metadata": {"name": "express", "version": "4.18.2", "description": "Fast web framework"}
        }),
    )
});

pub static NPM_TARBALL_REQUEST: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("NPM_TARBALL_REQUEST", "NPM client requests package tarball", json!({"type": "placeholder", "event_id": "NPM_TARBALL_REQUEST"})));

pub static NPM_LIST_REQUEST: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("NPM_LIST_REQUEST", "NPM client requests all packages list", json!({"type": "placeholder", "event_id": "NPM_LIST_REQUEST"})));

pub static NPM_SEARCH_REQUEST: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("NPM_SEARCH_REQUEST", "NPM client searches for packages", json!({"type": "placeholder", "event_id": "NPM_SEARCH_REQUEST"})));

/// NPM protocol action handler
pub struct NpmProtocol {}

impl NpmProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for NpmProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            package_metadata_action(),
            package_tarball_action(),
            package_list_action(),
            package_search_action(),
            npm_error_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "NPM"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_npm_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>NPM"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["npm"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("hyper HTTP server with NPM registry endpoints")
                .llm_control("LLM controls package metadata, tarballs, listings, and search results")
                .e2e_testing("Real npm CLI client")
                .notes("Implements NPM registry protocol: package metadata (GET /{package}), tarballs (GET /{package}/-/{tarball}), listing (GET /-/all), and search (GET /-/v1/search)")
                .build()
    }
    fn description(&self) -> &'static str {
        "NPM registry server with LLM-controlled package responses"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an NPM registry on port 4873 that serves express package"
    }
    fn group_name(&self) -> &'static str {
        "Package Management"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for NpmProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::npm::NpmServer;
            NpmServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "npm_package_metadata" => self.execute_package_metadata(action),
            "npm_package_tarball" => self.execute_package_tarball(action),
            "npm_package_list" => self.execute_package_list(action),
            "npm_package_search" => self.execute_package_search(action),
            "npm_error" => self.execute_npm_error(action),
            _ => Err(anyhow::anyhow!("Unknown NPM action: {}", action_type)),
        }
    }
}

impl NpmProtocol {
    fn execute_package_metadata(&self, action: serde_json::Value) -> Result<ActionResult> {
        let metadata = action.get("metadata").context("Missing 'metadata' field")?;

        debug!("NPM package metadata response");

        Ok(ActionResult::Custom {
            name: "npm_package_metadata".to_string(),
            data: json!({
                "metadata": metadata
            }),
        })
    }

    fn execute_package_tarball(&self, action: serde_json::Value) -> Result<ActionResult> {
        let tarball_data = action
            .get("tarball_data")
            .and_then(|v| v.as_str())
            .context("Missing 'tarball_data' field")?;

        debug!("NPM package tarball response");

        Ok(ActionResult::Custom {
            name: "npm_package_tarball".to_string(),
            data: json!({
                "tarball_data": tarball_data
            }),
        })
    }

    fn execute_package_list(&self, action: serde_json::Value) -> Result<ActionResult> {
        let packages = action.get("packages").context("Missing 'packages' field")?;

        debug!("NPM package list response");

        Ok(ActionResult::Custom {
            name: "npm_package_list".to_string(),
            data: json!({
                "packages": packages
            }),
        })
    }

    fn execute_package_search(&self, action: serde_json::Value) -> Result<ActionResult> {
        let results = action.get("results").context("Missing 'results' field")?;

        debug!("NPM package search response");

        Ok(ActionResult::Custom {
            name: "npm_package_search".to_string(),
            data: json!({
                "results": results
            }),
        })
    }

    fn execute_npm_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_message = action
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        let status_code = action
            .get("status_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(500) as u16;

        debug!("NPM error response: {} ({})", error_message, status_code);

        Ok(ActionResult::Custom {
            name: "npm_error".to_string(),
            data: json!({
                "error": error_message,
                "status_code": status_code
            }),
        })
    }
}

// Action definitions
fn package_metadata_action() -> ActionDefinition {
    ActionDefinition {
        name: "npm_package_metadata".to_string(),
        description: "Return NPM package metadata (package.json manifest)".to_string(),
        parameters: vec![Parameter {
            name: "metadata".to_string(),
            type_hint: "object".to_string(),
            description:
                "Package metadata JSON object with name, version, description, dependencies, etc."
                    .to_string(),
            required: true,
        }],
        example: json!({
            "type": "npm_package_metadata",
            "metadata": {
                "name": "express",
                "version": "4.18.2",
                "description": "Fast, unopinionated, minimalist web framework"
            }
        }),
    }
}

fn package_tarball_action() -> ActionDefinition {
    ActionDefinition {
        name: "npm_package_tarball".to_string(),
        description: "Return NPM package tarball (.tgz file)".to_string(),
        parameters: vec![Parameter {
            name: "tarball_data".to_string(),
            type_hint: "string".to_string(),
            description: "Base64-encoded tarball data (.tgz file contents)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "npm_package_tarball",
            "tarball_data": "H4sIAAAAAAAAA..."
        }),
    }
}

fn package_list_action() -> ActionDefinition {
    ActionDefinition {
        name: "npm_package_list".to_string(),
        description: "Return list of all available NPM packages".to_string(),
        parameters: vec![Parameter {
            name: "packages".to_string(),
            type_hint: "object".to_string(),
            description: "JSON object mapping package names to their metadata".to_string(),
            required: true,
        }],
        example: json!({
            "type": "npm_package_list",
            "packages": {
                "express": {"version": "4.18.2"},
                "lodash": {"version": "4.17.21"}
            }
        }),
    }
}

fn package_search_action() -> ActionDefinition {
    ActionDefinition {
        name: "npm_package_search".to_string(),
        description: "Return NPM package search results".to_string(),
        parameters: vec![Parameter {
            name: "results".to_string(),
            type_hint: "object".to_string(),
            description: "Search results JSON object with objects array and total count"
                .to_string(),
            required: true,
        }],
        example: json!({
            "type": "npm_package_search",
            "results": {
                "objects": [
                    {"package": {"name": "express", "version": "4.18.2"}}
                ],
                "total": 1
            }
        }),
    }
}

fn npm_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "npm_error".to_string(),
        description: "Return an NPM error response".to_string(),
        parameters: vec![
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 500)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "npm_error",
            "error": "Package not found",
            "status_code": 404
        }),
    }
}

/// Get NPM-specific event types
fn get_npm_event_types() -> Vec<EventType> {
    vec![
        EventType::new(
            "NPM_PACKAGE_REQUEST",
            "Triggered when a client requests package metadata (GET /{package})",
            json!({"type": "npm_package_metadata", "metadata": {"name": "example", "version": "1.0.0"}}),
        ),
        EventType::new(
            "NPM_TARBALL_REQUEST",
            "Triggered when a client requests package tarball (GET /{package}/-/{tarball})",
            json!({"type": "npm_package_tarball", "tarball_data": "base64data"}),
        ),
        EventType::new(
            "NPM_LIST_REQUEST",
            "Triggered when a client requests package listing (GET /-/all)",
            json!({"type": "npm_package_list", "packages": {}}),
        ),
        EventType::new(
            "NPM_SEARCH_REQUEST",
            "Triggered when a client requests package search (GET /-/v1/search)",
            json!({"type": "npm_package_search", "results": {"objects": [], "total": 0}}),
        ),
    ]
}
