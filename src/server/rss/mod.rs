//! RSS (Really Simple Syndication) Feed Server
//!
//! Serves RSS 2.0 XML feeds over HTTP with LLM-controlled content.
//! The LLM generates feed content dynamically for each request.

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::OllamaClient;
use crate::protocol::Event;
use crate::server::rss::actions::{RssProtocol, RSS_FEED_REQUESTED_EVENT};
use crate::state::app_state::AppState;
use crate::state::server::ServerId;
use anyhow::{Context, Result};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info};

/// RSS server - generates feeds dynamically via LLM
pub struct RssServer;

impl RssServer {
    /// Spawn RSS server with LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr)
            .await
            .context("Failed to bind RSS server")?;
        let local_addr = listener
            .local_addr()
            .context("Failed to get RSS server local address")?;

        info!("RSS server listening on {}", local_addr);
        status_tx
            .send(format!("[RSS] Server listening on {}", local_addr))
            .ok();

        let llm_client = Arc::new(llm_client);
        let protocol = Arc::new(RssProtocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        debug!("RSS connection from {}", peer_addr);

                        let llm = Arc::clone(&llm_client);
                        let state = Arc::clone(&app_state);
                        let status = status_tx.clone();
                        let proto = Arc::clone(&protocol);

                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                                let llm = Arc::clone(&llm);
                                let state = Arc::clone(&state);
                                let status = status.clone();
                                let proto = Arc::clone(&proto);

                                async move {
                                    Self::handle_request(req, llm, state, status, server_id, proto)
                                        .await
                                }
                            });

                            if let Err(e) =
                                http1::Builder::new().serve_connection(io, service).await
                            {
                                error!("RSS connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("RSS accept error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle HTTP request for RSS feed - calls LLM to generate content
    async fn handle_request(
        req: Request<hyper::body::Incoming>,
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        status_tx: UnboundedSender<String>,
        server_id: ServerId,
        protocol: Arc<RssProtocol>,
    ) -> Result<Response<http_body_util::Full<Bytes>>, hyper::Error> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let path = uri.path().to_string();
        let headers_map = req.headers().clone();

        debug!("RSS request: {} {}", method, path);
        status_tx
            .send(format!("[RSS] Request: {} {}", method, path))
            .ok();

        // Only support GET requests
        if method != hyper::Method::GET {
            return Ok(Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(http_body_util::Full::new(Bytes::from("Method Not Allowed")))
                .unwrap());
        }

        // Extract headers
        let mut headers = std::collections::HashMap::new();
        for (key, value) in headers_map.iter() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.as_str().to_lowercase(), value_str.to_string());
            }
        }

        // Create event for LLM
        let event = Event::new(
            &RSS_FEED_REQUESTED_EVENT,
            serde_json::json!({
                "path": path,
                "headers": headers,
            }),
        );

        info!("RSS calling LLM for feed: {}", path);
        status_tx
            .send(format!("[RSS] Calling LLM for feed: {}", path))
            .ok();

        // Call LLM to generate RSS feed
        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            None,
            &event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(execution_result) => {
                // Log messages
                for message in &execution_result.messages {
                    info!("{}", message);
                    status_tx.send(format!("[INFO] {}", message)).ok();
                }

                // Process protocol results
                for protocol_result in execution_result.protocol_results {
                    match protocol_result {
                        crate::llm::actions::protocol_trait::ActionResult::Custom {
                            name,
                            data,
                        } => {
                            if name == "generate_rss_feed" {
                                // Extract RSS feed data from LLM response
                                let rss_xml = Self::build_rss_from_llm_data(data);

                                match rss_xml {
                                    Ok(xml) => {
                                        let xml_bytes = xml.into_bytes();
                                        info!(
                                            "RSS feed generated: {} ({} bytes)",
                                            path,
                                            xml_bytes.len()
                                        );
                                        status_tx
                                            .send(format!(
                                                "[RSS] Generated feed: {} ({} bytes)",
                                                path,
                                                xml_bytes.len()
                                            ))
                                            .ok();

                                        return Ok(Response::builder()
                                            .status(StatusCode::OK)
                                            .header(
                                                "Content-Type",
                                                "application/rss+xml; charset=utf-8",
                                            )
                                            .body(http_body_util::Full::new(Bytes::from(xml_bytes)))
                                            .unwrap());
                                    }
                                    Err(e) => {
                                        error!("Failed to build RSS feed: {}", e);
                                        status_tx
                                            .send(format!(
                                                "[ERROR] Failed to build RSS feed: {}",
                                                e
                                            ))
                                            .ok();
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // If no feed was generated, return 404
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(http_body_util::Full::new(Bytes::from("Feed Not Found")))
                    .unwrap())
            }
            Err(e) => {
                error!("LLM error for RSS request: {}", e);
                status_tx.send(format!("[ERROR] LLM error: {}", e)).ok();

                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(http_body_util::Full::new(Bytes::from(
                        "Internal Server Error",
                    )))
                    .unwrap())
            }
        }
    }

    /// Build RSS XML from LLM-generated data
    fn build_rss_from_llm_data(data: serde_json::Value) -> Result<String> {
        use rss::{CategoryBuilder, ChannelBuilder, ItemBuilder};

        // Extract channel metadata
        let title = data["title"]
            .as_str()
            .unwrap_or("Untitled Feed")
            .to_string();
        let link = data["link"]
            .as_str()
            .unwrap_or("http://localhost")
            .to_string();
        let description = data["description"]
            .as_str()
            .unwrap_or("No description")
            .to_string();

        let mut channel_builder = ChannelBuilder::default();
        channel_builder
            .title(title)
            .link(link)
            .description(description);

        // Add optional channel fields
        if let Some(language) = data["language"].as_str() {
            channel_builder.language(Some(language.to_string()));
        }
        if let Some(ttl) = data["ttl"].as_str() {
            channel_builder.ttl(Some(ttl.to_string()));
        }
        if let Some(last_build_date) = data["last_build_date"].as_str() {
            channel_builder.last_build_date(Some(last_build_date.to_string()));
        }

        // Extract items
        let mut items = Vec::new();
        if let Some(items_array) = data["items"].as_array() {
            for item_data in items_array {
                let mut item_builder = ItemBuilder::default();

                if let Some(title) = item_data["title"].as_str() {
                    item_builder.title(Some(title.to_string()));
                }
                if let Some(link) = item_data["link"].as_str() {
                    item_builder.link(Some(link.to_string()));
                }
                if let Some(description) = item_data["description"].as_str() {
                    item_builder.description(Some(description.to_string()));
                }
                if let Some(author) = item_data["author"].as_str() {
                    item_builder.author(Some(author.to_string()));
                }
                if let Some(pub_date) = item_data["pub_date"].as_str() {
                    item_builder.pub_date(Some(pub_date.to_string()));
                }
                if let Some(guid) = item_data["guid"].as_str() {
                    item_builder.guid(Some(rss::GuidBuilder::default().value(guid).build()));
                }

                // Add categories
                if let Some(categories_array) = item_data["categories"].as_array() {
                    let categories: Vec<rss::Category> = categories_array
                        .iter()
                        .filter_map(|cat| {
                            if let Some(cat_str) = cat.as_str() {
                                Some(CategoryBuilder::default().name(cat_str).build())
                            } else if let Some(cat_obj) = cat.as_object() {
                                let name = cat_obj.get("name")?.as_str()?.to_string();
                                let mut builder = CategoryBuilder::default();
                                builder.name(name);
                                if let Some(domain) = cat_obj.get("domain").and_then(|v| v.as_str())
                                {
                                    builder.domain(Some(domain.to_string()));
                                }
                                Some(builder.build())
                            } else {
                                None
                            }
                        })
                        .collect();
                    item_builder.categories(categories);
                }

                items.push(item_builder.build());
            }
        }

        channel_builder.items(items);
        let channel = channel_builder.build();

        Ok(channel.to_string())
    }
}
