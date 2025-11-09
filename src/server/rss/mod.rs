//! RSS (Really Simple Syndication) Feed Server
//!
//! Serves RSS 2.0 XML feeds over HTTP with LLM-controlled content.
//! The LLM manages feed channels, items, and metadata.

pub mod actions;

use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::server::ServerId;
use anyhow::{Context, Result};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rss::{Channel, ChannelBuilder, ItemBuilder};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// RSS feed storage
#[derive(Clone)]
struct RssFeedStore {
    feeds: Arc<RwLock<HashMap<String, Channel>>>,
}

impl RssFeedStore {
    fn new() -> Self {
        Self {
            feeds: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn get_feed(&self, path: &str) -> Option<Channel> {
        self.feeds.read().await.get(path).cloned()
    }

    async fn set_feed(&self, path: String, channel: Channel) {
        self.feeds.write().await.insert(path, channel);
    }

    async fn delete_feed(&self, path: &str) -> bool {
        self.feeds.write().await.remove(path).is_some()
    }

    async fn list_paths(&self) -> Vec<String> {
        self.feeds.read().await.keys().cloned().collect()
    }
}

/// RSS server state
pub struct RssServer {
    feed_store: RssFeedStore,
}

impl RssServer {
    pub fn new() -> Self {
        Self {
            feed_store: RssFeedStore::new(),
        }
    }

    /// Spawn RSS server with LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: UnboundedSender<String>,
        _server_id: ServerId,
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

        let server = Arc::new(RssServer::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        debug!("RSS connection from {}", peer_addr);

                        let server_clone = Arc::clone(&server);
                        let status_tx_clone = status_tx.clone();

                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                                let server = Arc::clone(&server_clone);
                                let status = status_tx_clone.clone();

                                async move {
                                    server.handle_request(req, status).await
                                }
                            });

                            if let Err(e) = http1::Builder::new()
                                .serve_connection(io, service)
                                .await
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

    /// Handle HTTP request for RSS feed
    async fn handle_request(
        &self,
        req: Request<hyper::body::Incoming>,
        status_tx: UnboundedSender<String>,
    ) -> Result<Response<http_body_util::Full<Bytes>>, hyper::Error> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let path = uri.path().to_string();

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

        // Check if feed exists
        if let Some(channel) = self.feed_store.get_feed(&path).await {
            // Generate RSS XML
            match channel.to_string().into_bytes() {
                xml_bytes => {
                    info!("RSS feed served: {} ({} bytes)", path, xml_bytes.len());
                    status_tx
                        .send(format!("[RSS] Served feed: {}", path))
                        .ok();

                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/rss+xml; charset=utf-8")
                        .body(http_body_util::Full::new(Bytes::from(xml_bytes)))
                        .unwrap())
                }
            }
        } else {
            // Feed not found - trigger LLM to potentially create it
            info!("RSS feed not found: {}", path);

            // For now, return 404
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(http_body_util::Full::new(Bytes::from("Feed Not Found")))
                .unwrap())
        }
    }

    /// Create or update RSS feed
    pub async fn create_feed(
        &self,
        path: String,
        title: String,
        link: String,
        description: String,
    ) -> Result<()> {
        let channel = ChannelBuilder::default()
            .title(title)
            .link(link)
            .description(description)
            .build();

        self.feed_store.set_feed(path.clone(), channel).await;
        info!("RSS feed created: {}", path);
        Ok(())
    }

    /// Add item to RSS feed
    pub async fn add_item(
        &self,
        path: &str,
        title: String,
        link: Option<String>,
        description: Option<String>,
        pub_date: Option<String>,
    ) -> Result<()> {
        let mut channel = self
            .feed_store
            .get_feed(path)
            .await
            .context("Feed not found")?;

        let mut item_builder = ItemBuilder::default();
        item_builder.title(Some(title));

        if let Some(l) = link {
            item_builder.link(Some(l));
        }
        if let Some(d) = description {
            item_builder.description(Some(d));
        }
        if let Some(pd) = pub_date {
            item_builder.pub_date(Some(pd));
        }

        let item = item_builder.build();

        let mut items = channel.items().to_vec();
        items.push(item);
        channel.set_items(items);

        self.feed_store.set_feed(path.to_string(), channel).await;
        info!("RSS item added to feed: {}", path);
        Ok(())
    }

    /// Delete RSS feed
    pub async fn delete_feed(&self, path: &str) -> Result<()> {
        if self.feed_store.delete_feed(path).await {
            info!("RSS feed deleted: {}", path);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Feed not found: {}", path))
        }
    }

    /// List all feed paths
    pub async fn list_feeds(&self) -> Vec<String> {
        self.feed_store.list_paths().await
    }
}
