//! Maven repository protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Maven protocol action handler
pub struct MavenProtocol;

impl MavenProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for MavenProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // Maven has no async actions - it's purely request-response
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_maven_artifact_action(),
            send_maven_metadata_action(),
            send_maven_error_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Maven"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_maven_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>Maven"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["maven", "maven repository", "maven repo", "via maven"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("hyper v1.0 HTTP server with Maven repository path parsing")
            .llm_control(
                "Artifact availability, content generation (POM, JAR, checksums), version metadata",
            )
            .e2e_testing("mvn CLI client - target < 10 LLM calls")
            .build()
    }
    fn description(&self) -> &'static str {
        "Maven repository server serving Java artifacts"
    }
    fn example_prompt(&self) -> &'static str {
        "Maven repository on port 8080 serving a simple library com.example:hello-world:1.0.0 with a JAR and POM file"
    }
    fn group_name(&self) -> &'static str {
        "Application"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "maven",
                "instruction": "Maven repository server. Serve artifact com.example:hello-world:1.0.0 with JAR and POM files."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "maven",
                "event_handlers": [{
                    "event_pattern": "maven_artifact_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<maven_handler>"
                    }
                }]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "maven",
                "event_handlers": [{
                    "event_pattern": "maven_artifact_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_maven_metadata",
                            "group_id": "com.example",
                            "artifact_id": "hello-world",
                            "versions": ["1.0.0"],
                            "latest": "1.0.0",
                            "release": "1.0.0"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for MavenProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::maven::MavenServer;
            MavenServer::spawn_with_llm_actions(
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
            "send_maven_artifact" => self.execute_send_maven_artifact(action),
            "send_maven_metadata" => self.execute_send_maven_metadata(action),
            "send_maven_error" => self.execute_send_maven_error(action),
            _ => Err(anyhow::anyhow!("Unknown Maven action: {action_type}")),
        }
    }
}

impl MavenProtocol {
    /// Execute send_maven_artifact sync action
    fn execute_send_maven_artifact(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;

        let content_type = action
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("application/octet-stream");

        let body = action
            .get("body")
            .and_then(|v| v.as_str())
            .context("Missing 'body' parameter")?;

        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), content_type.to_string());

        // Add optional custom headers
        if let Some(custom_headers) = action.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in custom_headers {
                if let Some(v_str) = v.as_str() {
                    headers.insert(k.clone(), v_str.to_string());
                }
            }
        }

        let response_data = json!({
            "status": status,
            "headers": headers,
            "body": body
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data)
                .context("Failed to serialize Maven artifact response")?,
        ))
    }

    /// Execute send_maven_metadata sync action
    fn execute_send_maven_metadata(&self, action: serde_json::Value) -> Result<ActionResult> {
        let group_id = action
            .get("group_id")
            .and_then(|v| v.as_str())
            .context("Missing 'group_id' parameter")?;

        let artifact_id = action
            .get("artifact_id")
            .and_then(|v| v.as_str())
            .context("Missing 'artifact_id' parameter")?;

        let versions = action
            .get("versions")
            .and_then(|v| v.as_array())
            .context("Missing 'versions' parameter")?;

        let latest = action.get("latest").and_then(|v| v.as_str());

        let release = action.get("release").and_then(|v| v.as_str());

        // Generate maven-metadata.xml
        let mut xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>{}</groupId>
  <artifactId>{}</artifactId>
  <versioning>
    <versions>
"#,
            group_id, artifact_id
        );

        for version in versions {
            if let Some(v) = version.as_str() {
                xml.push_str(&format!("      <version>{}</version>\n", v));
            }
        }

        xml.push_str("    </versions>\n");

        if let Some(latest_ver) = latest {
            xml.push_str(&format!("    <latest>{}</latest>\n", latest_ver));
        }

        if let Some(release_ver) = release {
            xml.push_str(&format!("    <release>{}</release>\n", release_ver));
        }

        xml.push_str("  </versioning>\n</metadata>\n");

        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/xml".to_string());

        let response_data = json!({
            "status": 200,
            "headers": headers,
            "body": xml
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data)
                .context("Failed to serialize Maven metadata response")?,
        ))
    }

    /// Execute send_maven_error sync action
    fn execute_send_maven_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action.get("status").and_then(|v| v.as_u64()).unwrap_or(404) as u16;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Not Found");

        let response_data = json!({
            "status": status,
            "headers": {
                "Content-Type": "text/plain"
            },
            "body": message
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data)
                .context("Failed to serialize Maven error response")?,
        ))
    }
}

/// Action definition for send_maven_artifact (sync)
fn send_maven_artifact_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_maven_artifact".to_string(),
        description: "Send a Maven artifact file (JAR, POM, checksum, etc.)".to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 200)".to_string(),
                required: false,
            },
            Parameter {
                name: "content_type".to_string(),
                type_hint: "string".to_string(),
                description: "Content-Type header (default: application/octet-stream)".to_string(),
                required: false,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Artifact content (for binary files, use base64 encoding)".to_string(),
                required: true,
            },
            Parameter {
                name: "headers".to_string(),
                type_hint: "object".to_string(),
                description: "Optional additional headers".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_maven_artifact",
            "status": 200,
            "content_type": "application/java-archive",
            "body": "UEsDBBQACAgIAAAAIQAAAAAAAAAAAAAAAAA..." // base64-encoded JAR
        }),
    }
}

/// Action definition for send_maven_metadata (sync)
fn send_maven_metadata_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_maven_metadata".to_string(),
        description: "Send Maven metadata XML listing available versions".to_string(),
        parameters: vec![
            Parameter {
                name: "group_id".to_string(),
                type_hint: "string".to_string(),
                description: "Maven group ID (e.g., 'com.example')".to_string(),
                required: true,
            },
            Parameter {
                name: "artifact_id".to_string(),
                type_hint: "string".to_string(),
                description: "Maven artifact ID".to_string(),
                required: true,
            },
            Parameter {
                name: "versions".to_string(),
                type_hint: "array".to_string(),
                description: "Array of available version strings".to_string(),
                required: true,
            },
            Parameter {
                name: "latest".to_string(),
                type_hint: "string".to_string(),
                description: "Latest version (optional)".to_string(),
                required: false,
            },
            Parameter {
                name: "release".to_string(),
                type_hint: "string".to_string(),
                description: "Latest release version (optional)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_maven_metadata",
            "group_id": "com.example",
            "artifact_id": "mylib",
            "versions": ["1.0.0", "1.0.1", "1.1.0"],
            "latest": "1.1.0",
            "release": "1.1.0"
        }),
    }
}

/// Action definition for send_maven_error (sync)
fn send_maven_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_maven_error".to_string(),
        description: "Send an error response (typically 404 Not Found)".to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 404)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message (default: 'Not Found')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_maven_error",
            "status": 404,
            "message": "Artifact not found"
        }),
    }
}

// ============================================================================
// Maven Action Constants
// ============================================================================

pub static SEND_MAVEN_ARTIFACT_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| send_maven_artifact_action());
pub static SEND_MAVEN_METADATA_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| send_maven_metadata_action());
pub static SEND_MAVEN_ERROR_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| send_maven_error_action());

// ============================================================================
// Maven Event Type Constants
// ============================================================================

/// Maven artifact request event - triggered when client requests an artifact
pub static MAVEN_ARTIFACT_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "maven_artifact_request",
        "Maven artifact request received from client",
        json!({
            "type": "send_maven_artifact",
            "status": 200,
            "content_type": "application/java-archive",
            "body": "UEsDBBQACAgIAAAAIQAAAAAAAAAAAAAAAAA..."
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "HTTP method (usually GET)".to_string(),
            required: true,
        },
        Parameter {
            name: "uri".to_string(),
            type_hint: "string".to_string(),
            description: "Full request URI".to_string(),
            required: true,
        },
        Parameter {
            name: "group_id".to_string(),
            type_hint: "string".to_string(),
            description: "Maven group ID (e.g., 'com.example')".to_string(),
            required: true,
        },
        Parameter {
            name: "artifact_id".to_string(),
            type_hint: "string".to_string(),
            description: "Maven artifact ID".to_string(),
            required: true,
        },
        Parameter {
            name: "version".to_string(),
            type_hint: "string".to_string(),
            description: "Artifact version (null for metadata requests)".to_string(),
            required: false,
        },
        Parameter {
            name: "classifier".to_string(),
            type_hint: "string".to_string(),
            description: "Artifact classifier (e.g., 'sources', 'javadoc')".to_string(),
            required: false,
        },
        Parameter {
            name: "extension".to_string(),
            type_hint: "string".to_string(),
            description: "File extension (e.g., 'jar', 'pom', 'xml')".to_string(),
            required: true,
        },
        Parameter {
            name: "is_metadata".to_string(),
            type_hint: "boolean".to_string(),
            description: "True if requesting maven-metadata.xml".to_string(),
            required: true,
        },
        Parameter {
            name: "is_checksum".to_string(),
            type_hint: "boolean".to_string(),
            description: "True if requesting a checksum file".to_string(),
            required: true,
        },
        Parameter {
            name: "checksum_type".to_string(),
            type_hint: "string".to_string(),
            description: "Checksum type if is_checksum is true (sha1, md5, sha256, sha512)"
                .to_string(),
            required: false,
        },
        Parameter {
            name: "headers".to_string(),
            type_hint: "object".to_string(),
            description: "HTTP request headers".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        SEND_MAVEN_ARTIFACT_ACTION.clone(),
        SEND_MAVEN_METADATA_ACTION.clone(),
        SEND_MAVEN_ERROR_ACTION.clone(),
    ])
});

/// Get Maven event types
pub fn get_maven_event_types() -> Vec<EventType> {
    vec![MAVEN_ARTIFACT_REQUEST_EVENT.clone()]
}
