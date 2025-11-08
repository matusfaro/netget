//! Maven client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Maven client connected event
pub static MAVEN_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "maven_connected",
        "Maven client connected to repository"
    )
    .with_parameters(vec![
        Parameter {
            name: "repository_url".to_string(),
            type_hint: "string".to_string(),
            description: "Maven repository base URL".to_string(),
            required: true,
        },
    ])
});

/// Maven artifact downloaded event
pub static MAVEN_CLIENT_ARTIFACT_DOWNLOADED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "maven_artifact_downloaded",
        "Maven artifact successfully downloaded"
    )
    .with_parameters(vec![
        Parameter {
            name: "group_id".to_string(),
            type_hint: "string".to_string(),
            description: "Maven artifact group ID".to_string(),
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
            description: "Maven artifact version".to_string(),
            required: true,
        },
        Parameter {
            name: "packaging".to_string(),
            type_hint: "string".to_string(),
            description: "Artifact packaging type (jar, war, pom, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "size_bytes".to_string(),
            type_hint: "number".to_string(),
            description: "Downloaded artifact size in bytes".to_string(),
            required: true,
        },
    ])
});

/// Maven POM received event
pub static MAVEN_CLIENT_POM_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "maven_pom_received",
        "Maven POM file downloaded and received"
    )
    .with_parameters(vec![
        Parameter {
            name: "group_id".to_string(),
            type_hint: "string".to_string(),
            description: "Maven artifact group ID".to_string(),
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
            description: "Maven artifact version".to_string(),
            required: true,
        },
        Parameter {
            name: "pom_content".to_string(),
            type_hint: "string".to_string(),
            description: "POM file XML content".to_string(),
            required: true,
        },
    ])
});

/// Maven metadata received event
pub static MAVEN_CLIENT_METADATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "maven_metadata_received",
        "Maven metadata XML received with version information"
    )
    .with_parameters(vec![
        Parameter {
            name: "group_id".to_string(),
            type_hint: "string".to_string(),
            description: "Maven artifact group ID".to_string(),
            required: true,
        },
        Parameter {
            name: "artifact_id".to_string(),
            type_hint: "string".to_string(),
            description: "Maven artifact ID".to_string(),
            required: true,
        },
        Parameter {
            name: "metadata_content".to_string(),
            type_hint: "string".to_string(),
            description: "Maven metadata XML content".to_string(),
            required: true,
        },
    ])
});

/// Maven client protocol action handler
pub struct MavenClientProtocol;

impl MavenClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for MavenClientProtocol {
        fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
            vec![
                ParameterDefinition {
                    name: "repository_url".to_string(),
                    description: "Maven repository base URL (defaults to Maven Central)".to_string(),
                    type_hint: "string".to_string(),
                    required: false,
                    example: json!("https://repo.maven.apache.org/maven2"),
                },
            ]
        }
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            vec![
                ActionDefinition {
                    name: "download_artifact".to_string(),
                    description: "Download a Maven artifact by coordinates (groupId:artifactId:version)".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "group_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Maven group ID (e.g., 'org.apache.commons')".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "artifact_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Maven artifact ID (e.g., 'commons-lang3')".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "version".to_string(),
                            type_hint: "string".to_string(),
                            description: "Artifact version (e.g., '3.12.0')".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "packaging".to_string(),
                            type_hint: "string".to_string(),
                            description: "Artifact packaging type (jar, war, pom, etc.), defaults to 'jar'".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "download_artifact",
                        "group_id": "org.apache.commons",
                        "artifact_id": "commons-lang3",
                        "version": "3.12.0",
                        "packaging": "jar"
                    }),
                },
                ActionDefinition {
                    name: "download_pom".to_string(),
                    description: "Download and parse a Maven POM file".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "group_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Maven group ID".to_string(),
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
                            description: "Artifact version".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "download_pom",
                        "group_id": "org.springframework.boot",
                        "artifact_id": "spring-boot-starter",
                        "version": "2.7.0"
                    }),
                },
                ActionDefinition {
                    name: "search_versions".to_string(),
                    description: "Search for available versions of a Maven artifact".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "group_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Maven group ID".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "artifact_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Maven artifact ID".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "search_versions",
                        "group_id": "junit",
                        "artifact_id": "junit"
                    }),
                },
                ActionDefinition {
                    name: "disconnect".to_string(),
                    description: "Disconnect from the Maven repository".to_string(),
                    parameters: vec![],
                    example: json!({
                        "type": "disconnect"
                    }),
                },
            ]
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                ActionDefinition {
                    name: "download_artifact".to_string(),
                    description: "Download another Maven artifact in response to received data".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "group_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Maven group ID".to_string(),
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
                            description: "Artifact version".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "packaging".to_string(),
                            type_hint: "string".to_string(),
                            description: "Artifact packaging type".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "download_artifact",
                        "group_id": "com.google.guava",
                        "artifact_id": "guava",
                        "version": "31.1-jre"
                    }),
                },
                ActionDefinition {
                    name: "download_pom".to_string(),
                    description: "Download POM file in response to received data".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "group_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Maven group ID".to_string(),
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
                            description: "Artifact version".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "download_pom",
                        "group_id": "org.apache.commons",
                        "artifact_id": "commons-collections4",
                        "version": "4.4"
                    }),
                },
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "Maven"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            vec![
                EventType {
                    id: "maven_connected".to_string(),
                    description: "Triggered when Maven client connects to repository".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
                EventType {
                    id: "maven_artifact_downloaded".to_string(),
                    description: "Triggered when Maven artifact is successfully downloaded".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
                EventType {
                    id: "maven_pom_received".to_string(),
                    description: "Triggered when POM file is downloaded and received".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
                EventType {
                    id: "maven_metadata_received".to_string(),
                    description: "Triggered when Maven metadata is received".to_string(),
                    actions: vec![],
                    parameters: vec![],
                },
            ]
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP>HTTP>Maven"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["maven", "maven client", "connect to maven", "maven repository"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .implementation("reqwest HTTP client with Maven repository protocol")
                .llm_control("Full control over artifact resolution, POM parsing, version search")
                .e2e_testing("Maven Central or local Maven repository")
                .build()
        }
        fn description(&self) -> &'static str {
            "Maven client for downloading artifacts and resolving dependencies from Maven repositories"
        }
        fn example_prompt(&self) -> &'static str {
            "Connect to Maven Central and download commons-lang3:3.12.0"
        }
        fn group_name(&self) -> &'static str {
            "Package Managers"
        }
}

// Implement Client trait (client-specific functionality)
impl Client for MavenClientProtocol {
        fn connect(
            &self,
            ctx: crate::protocol::ConnectContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::client::maven::MavenClient;
                MavenClient::connect_with_llm_actions(
                    ctx.remote_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    ctx.client_id,
                )
                .await
            })
        }
        fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
            let action_type = action
                .get("type")
                .and_then(|v| v.as_str())
                .context("Missing 'type' field in action")?;
    
            match action_type {
                "download_artifact" => {
                    let group_id = action
                        .get("group_id")
                        .and_then(|v| v.as_str())
                        .context("Missing 'group_id' field")?
                        .to_string();
    
                    let artifact_id = action
                        .get("artifact_id")
                        .and_then(|v| v.as_str())
                        .context("Missing 'artifact_id' field")?
                        .to_string();
    
                    let version = action
                        .get("version")
                        .and_then(|v| v.as_str())
                        .context("Missing 'version' field")?
                        .to_string();
    
                    let packaging = action
                        .get("packaging")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
    
                    Ok(ClientActionResult::Custom {
                        name: "maven_download_artifact".to_string(),
                        data: json!({
                            "group_id": group_id,
                            "artifact_id": artifact_id,
                            "version": version,
                            "packaging": packaging,
                        }),
                    })
                }
                "download_pom" => {
                    let group_id = action
                        .get("group_id")
                        .and_then(|v| v.as_str())
                        .context("Missing 'group_id' field")?
                        .to_string();
    
                    let artifact_id = action
                        .get("artifact_id")
                        .and_then(|v| v.as_str())
                        .context("Missing 'artifact_id' field")?
                        .to_string();
    
                    let version = action
                        .get("version")
                        .and_then(|v| v.as_str())
                        .context("Missing 'version' field")?
                        .to_string();
    
                    Ok(ClientActionResult::Custom {
                        name: "maven_download_pom".to_string(),
                        data: json!({
                            "group_id": group_id,
                            "artifact_id": artifact_id,
                            "version": version,
                        }),
                    })
                }
                "search_versions" => {
                    let group_id = action
                        .get("group_id")
                        .and_then(|v| v.as_str())
                        .context("Missing 'group_id' field")?
                        .to_string();
    
                    let artifact_id = action
                        .get("artifact_id")
                        .and_then(|v| v.as_str())
                        .context("Missing 'artifact_id' field")?
                        .to_string();
    
                    Ok(ClientActionResult::Custom {
                        name: "maven_search_versions".to_string(),
                        data: json!({
                            "group_id": group_id,
                            "artifact_id": artifact_id,
                        }),
                    })
                }
                "disconnect" => Ok(ClientActionResult::Disconnect),
                _ => Err(anyhow::anyhow!("Unknown Maven client action: {}", action_type)),
            }
        }
}

