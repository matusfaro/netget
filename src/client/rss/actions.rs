//! RSS client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult, ConnectContext},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

/// RSS client connected event
pub static RSS_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "rss_connected",
        "RSS client initialized and ready to fetch feeds",
    )
    .with_parameters(vec![Parameter {
        name: "base_url".to_string(),
        type_hint: "string".to_string(),
        description: "Base URL for RSS feeds".to_string(),
        required: true,
    }])
});

/// RSS client feed fetched event
pub static RSS_CLIENT_FEED_FETCHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("rss_feed_fetched", "RSS feed fetched and parsed").with_parameters(vec![
        Parameter {
            name: "url".to_string(),
            type_hint: "string".to_string(),
            description: "Feed URL".to_string(),
            required: true,
        },
        Parameter {
            name: "feed_title".to_string(),
            type_hint: "string".to_string(),
            description: "Feed title".to_string(),
            required: true,
        },
        Parameter {
            name: "feed_link".to_string(),
            type_hint: "string".to_string(),
            description: "Feed link (website URL)".to_string(),
            required: true,
        },
        Parameter {
            name: "feed_description".to_string(),
            type_hint: "string".to_string(),
            description: "Feed description".to_string(),
            required: true,
        },
        Parameter {
            name: "item_count".to_string(),
            type_hint: "number".to_string(),
            description: "Number of items in feed".to_string(),
            required: true,
        },
        Parameter {
            name: "items".to_string(),
            type_hint: "array".to_string(),
            description: "Array of feed items".to_string(),
            required: true,
        },
    ])
});

/// RSS client protocol action handler
pub struct RssClientProtocol;

impl RssClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for RssClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        Vec::new()
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "fetch_rss_feed".to_string(),
                description: "Fetch and parse an RSS feed from a URL".to_string(),
                parameters: vec![
                    Parameter {
                        name: "url".to_string(),
                        type_hint: "string".to_string(),
                        description: "Full URL to RSS feed (e.g., http://example.com/feed.xml)"
                            .to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "if_modified_since".to_string(),
                        type_hint: "string".to_string(),
                        description: "RFC 2822 date for conditional fetch (e.g., 'Mon, 09 Nov 2025 12:00:00 GMT')"
                            .to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "fetch_rss_feed",
                    "url": "http://example.com/tech-news.xml",
                    "if_modified_since": "Mon, 09 Nov 2025 12:00:00 GMT"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the RSS feed source".to_string(),
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
                name: "fetch_rss_feed".to_string(),
                description: "Fetch another RSS feed in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "url".to_string(),
                        type_hint: "string".to_string(),
                        description: "Full URL to RSS feed".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "if_modified_since".to_string(),
                        type_hint: "string".to_string(),
                        description: "RFC 2822 date for conditional fetch".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "fetch_rss_feed",
                    "url": "http://example.com/related-feed.xml",
                    "if_modified_since": "Mon, 09 Nov 2025 12:00:00 GMT"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more user input before fetching more feeds".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "RSS"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            RSS_CLIENT_CONNECTED_EVENT.clone(),
            RSS_CLIENT_FEED_FETCHED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>RSS"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["rss", "rss client", "feed reader", "syndication"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("reqwest HTTP client + rss crate for RSS 2.0 XML parsing")
            .llm_control("Feed selection, item filtering, content interpretation")
            .e2e_testing("RSS server - planned <10 LLM calls")
            .build()
    }

    fn description(&self) -> &'static str {
        "RSS feed client for reading web syndication feeds"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to localhost:8080 via rss and fetch /tech-news.xml, show me the latest 5 items"
    }

    fn group_name(&self) -> &'static str {
        "Web"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for RssClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::rss::RssClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.app_state,
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
            "fetch_rss_feed" => {
                let url = action
                    .get("url")
                    .and_then(|v| v.as_str())
                    .context("Missing 'url' field")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "fetch_rss_feed".to_string(),
                    data: json!({ "url": url }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown RSS client action: {action_type}")),
        }
    }
}
