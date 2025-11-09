//! RSS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::{EventType, SpawnContext};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::pin::Pin;
use std::sync::LazyLock;

/// RSS feed requested event - fired when a client requests a feed
pub static RSS_FEED_REQUESTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "rss_feed_requested",
        "RSS feed requested by client",
    )
    .with_parameters(vec![
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Feed path (e.g., /news.xml)".to_string(),
            required: true,
        },
        Parameter {
            name: "headers".to_string(),
            type_hint: "object".to_string(),
            description: "HTTP request headers".to_string(),
            required: true,
        },
    ])
});

/// RSS protocol action handler
pub struct RssProtocol;

impl RssProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for RssProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        Vec::new()
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // RSS has no async actions - feeds are generated on request
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![generate_rss_feed_action()]
    }

    fn protocol_name(&self) -> &'static str {
        "RSS"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![RSS_FEED_REQUESTED_EVENT.clone()]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>RSS"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["rss", "rss server", "feed", "syndication", "via rss"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("rss crate for RSS 2.0 XML generation, served over HTTP")
            .llm_control("Feed content generation (title, items, categories)")
            .e2e_testing("reqwest HTTP client - planned <10 LLM calls")
            .build()
    }

    fn description(&self) -> &'static str {
        "RSS feed server for web syndication"
    }

    fn example_prompt(&self) -> &'static str {
        "Create an RSS feed server on port 8080 serving tech news at /tech.xml with categories"
    }

    fn group_name(&self) -> &'static str {
        "Web"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for RssProtocol {
    fn spawn(
        &self,
        ctx: SpawnContext,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>> {
        Box::pin(async move {
            use crate::server::rss::RssServer;

            RssServer::spawn_with_llm_actions(
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
            "generate_rss_feed" => self.execute_generate_feed(action),
            _ => Err(anyhow::anyhow!("Unknown RSS action: {action_type}")),
        }
    }
}

impl RssProtocol {
    /// Execute generate_rss_feed sync action
    fn execute_generate_feed(&self, action: serde_json::Value) -> Result<ActionResult> {
        // Extract feed data from action
        let data = action.clone();

        // Return custom result with feed data
        Ok(ActionResult::Custom {
            name: "generate_rss_feed".to_string(),
            data,
        })
    }
}

/// Action definition for generate_rss_feed (sync)
fn generate_rss_feed_action() -> ActionDefinition {
    ActionDefinition {
        name: "generate_rss_feed".to_string(),
        description: "Generate RSS feed XML for the current request".to_string(),
        parameters: vec![
            Parameter {
                name: "title".to_string(),
                type_hint: "string".to_string(),
                description: "Feed title".to_string(),
                required: true,
            },
            Parameter {
                name: "link".to_string(),
                type_hint: "string".to_string(),
                description: "Feed link (website URL)".to_string(),
                required: true,
            },
            Parameter {
                name: "description".to_string(),
                type_hint: "string".to_string(),
                description: "Feed description".to_string(),
                required: true,
            },
            Parameter {
                name: "language".to_string(),
                type_hint: "string".to_string(),
                description: "Feed language (e.g., 'en-us')".to_string(),
                required: false,
            },
            Parameter {
                name: "ttl".to_string(),
                type_hint: "string".to_string(),
                description: "Time to live in minutes".to_string(),
                required: false,
            },
            Parameter {
                name: "last_build_date".to_string(),
                type_hint: "string".to_string(),
                description: "Last build date (RFC 2822)".to_string(),
                required: false,
            },
            Parameter {
                name: "items".to_string(),
                type_hint: "array".to_string(),
                description: "Array of feed items".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "generate_rss_feed",
            "title": "Tech News Feed",
            "link": "https://example.com",
            "description": "Latest technology news",
            "language": "en-us",
            "ttl": "60",
            "last_build_date": "Mon, 09 Nov 2025 12:00:00 GMT",
            "items": [
                {
                    "title": "New AI Model Released",
                    "link": "https://example.com/ai-news",
                    "description": "Company X released a new AI model",
                    "author": "john@example.com (John Doe)",
                    "pub_date": "Mon, 09 Nov 2025 10:00:00 GMT",
                    "guid": "https://example.com/ai-news",
                    "categories": [
                        "AI",
                        "Technology",
                        {"name": "Machine Learning", "domain": "tech.example.com"}
                    ]
                },
                {
                    "title": "Cloud Computing Trends",
                    "link": "https://example.com/cloud-trends",
                    "description": "Latest trends in cloud computing",
                    "pub_date": "Mon, 09 Nov 2025 09:00:00 GMT",
                    "categories": ["Cloud", "Infrastructure"]
                }
            ]
        }),
    }
}
