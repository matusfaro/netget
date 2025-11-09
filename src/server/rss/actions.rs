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

/// RSS protocol action handler
pub struct RssProtocol;

impl RssProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Event types
pub static RSS_FEED_CREATED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "rss_feed_created",
        "RSS feed created successfully",
    )
    .with_parameters(vec![
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Feed path (e.g., /news.xml)".to_string(),
            required: true,
        },
    ])
});

pub static RSS_ITEM_ADDED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "rss_item_added",
        "RSS item added to feed",
    )
    .with_parameters(vec![
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Feed path".to_string(),
            required: true,
        },
        Parameter {
            name: "title".to_string(),
            type_hint: "string".to_string(),
            description: "Item title".to_string(),
            required: true,
        },
    ])
});

// Implement Protocol trait (common functionality)
impl Protocol for RssProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        Vec::new()
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "create_rss_feed".to_string(),
                description: "Create a new RSS feed at a specific path".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Feed path (e.g., /news.xml, /blog.xml)".to_string(),
                        required: true,
                    },
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
                ],
                example: json!({
                    "type": "create_rss_feed",
                    "path": "/tech-news.xml",
                    "title": "Tech News Feed",
                    "link": "https://example.com",
                    "description": "Latest technology news and updates"
                }),
            },
            ActionDefinition {
                name: "add_rss_item".to_string(),
                description: "Add a new item to an existing RSS feed".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Feed path".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "title".to_string(),
                        type_hint: "string".to_string(),
                        description: "Item title".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "link".to_string(),
                        type_hint: "string".to_string(),
                        description: "Item link (article URL)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "description".to_string(),
                        type_hint: "string".to_string(),
                        description: "Item description or summary".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "pub_date".to_string(),
                        type_hint: "string".to_string(),
                        description: "Publication date (RFC 2822 format)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "add_rss_item",
                    "path": "/tech-news.xml",
                    "title": "New AI Model Released",
                    "link": "https://example.com/ai-news",
                    "description": "Company X released a new AI model today",
                    "pub_date": "Mon, 01 Jan 2024 12:00:00 GMT"
                }),
            },
            ActionDefinition {
                name: "delete_rss_feed".to_string(),
                description: "Delete an RSS feed".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Feed path to delete".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "delete_rss_feed",
                    "path": "/old-feed.xml"
                }),
            },
            ActionDefinition {
                name: "list_rss_feeds".to_string(),
                description: "List all available RSS feeds".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_rss_feeds"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        // RSS feeds are served via HTTP GET, no sync actions needed
        Vec::new()
    }

    fn protocol_name(&self) -> &'static str {
        "RSS"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            RSS_FEED_CREATED_EVENT.clone(),
            RSS_ITEM_ADDED_EVENT.clone(),
        ]
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
            .llm_control("Feed metadata, items, publication dates")
            .e2e_testing("reqwest HTTP client - planned <10 LLM calls")
            .build()
    }

    fn description(&self) -> &'static str {
        "RSS feed server for web syndication"
    }

    fn example_prompt(&self) -> &'static str {
        "Create an RSS feed server on port 8080 serving tech news at /tech.xml and sports news at /sports.xml"
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
            "create_rss_feed" => self.execute_create_feed(action),
            "add_rss_item" => self.execute_add_item(action),
            "delete_rss_feed" => self.execute_delete_feed(action),
            "list_rss_feeds" => self.execute_list_feeds(action),
            _ => Err(anyhow::anyhow!("Unknown RSS action: {action_type}")),
        }
    }
}

impl RssProtocol {
    /// Execute create_rss_feed action
    fn execute_create_feed(&self, action: serde_json::Value) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?
            .to_string();
        let title = action
            .get("title")
            .and_then(|v| v.as_str())
            .context("Missing 'title' field")?
            .to_string();
        let link = action
            .get("link")
            .and_then(|v| v.as_str())
            .context("Missing 'link' field")?
            .to_string();
        let description = action
            .get("description")
            .and_then(|v| v.as_str())
            .context("Missing 'description' field")?
            .to_string();

        // Return custom result (RSS feed management will be added in future)
        Ok(ActionResult::Custom {
            name: "rss_feed_created".to_string(),
            data: json!({
                "path": path,
                "title": title,
                "link": link,
                "description": description,
            }),
        })
    }

    /// Execute add_rss_item action
    fn execute_add_item(&self, action: serde_json::Value) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?
            .to_string();
        let title = action
            .get("title")
            .and_then(|v| v.as_str())
            .context("Missing 'title' field")?
            .to_string();
        let link = action.get("link").and_then(|v| v.as_str()).map(|s| s.to_string());
        let description = action.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
        let pub_date = action.get("pub_date").and_then(|v| v.as_str()).map(|s| s.to_string());

        Ok(ActionResult::Custom {
            name: "rss_item_added".to_string(),
            data: json!({
                "path": path,
                "title": title,
                "link": link,
                "description": description,
                "pub_date": pub_date,
            }),
        })
    }

    /// Execute delete_rss_feed action
    fn execute_delete_feed(&self, action: serde_json::Value) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?
            .to_string();

        Ok(ActionResult::Custom {
            name: "rss_feed_deleted".to_string(),
            data: json!({
                "path": path,
            }),
        })
    }

    /// Execute list_rss_feeds action
    fn execute_list_feeds(&self, _action: serde_json::Value) -> Result<ActionResult> {
        Ok(ActionResult::Custom {
            name: "rss_feeds_listed".to_string(),
            data: json!({}),
        })
    }
}
