//! etcd v3 server implementation with gRPC KV service
//!
//! Implements etcd v3 KV service where the LLM controls all key-value operations
//! through actions. Uses pre-compiled protobuf definitions from build.rs.

pub mod actions;

// Re-export protocol for external use
pub use actions::EtcdProtocol;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};
use anyhow::{Result, bail};

#[cfg(feature = "etcd")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "etcd")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "etcd")]
use crate::state::app_state::AppState;
#[cfg(feature = "etcd")]
use crate::protocol::Event;
#[cfg(feature = "etcd")]
use crate::server::etcd::actions::ETCD_RANGE_REQUEST_EVENT;
#[cfg(feature = "etcd")]
use hyper::{Request, Response, body::Incoming, StatusCode};
#[cfg(feature = "etcd")]
use http_body_util::{BodyExt, Full};
#[cfg(feature = "etcd")]
use bytes::Bytes;
#[cfg(feature = "etcd")]
use prost::Message;

// Include generated protobuf code
#[cfg(feature = "etcd")]
mod etcdserverpb {
    include!(concat!(env!("OUT_DIR"), "/etcdserverpb.rs"));
}
#[cfg(feature = "etcd")]
mod mvccpb {
    include!(concat!(env!("OUT_DIR"), "/mvccpb.rs"));
}

#[cfg(feature = "etcd")]
use etcdserverpb::{RangeRequest, RangeResponse, PutRequest, PutResponse, DeleteRangeRequest, DeleteRangeResponse, TxnRequest, TxnResponse, CompactionRequest, CompactionResponse, ResponseHeader};
#[cfg(feature = "etcd")]
use mvccpb::KeyValue;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// In-memory key-value store with MVCC-like revision tracking
#[cfg(feature = "etcd")]
struct EtcdStore {
    /// Key-value pairs
    #[allow(dead_code)] // Will be used when LLM actions wire up to store mutations
    kvs: HashMap<Vec<u8>, KeyValue>,
    /// Current revision counter
    revision: i64,
    /// Cluster ID
    cluster_id: u64,
    /// Member ID
    member_id: u64,
}

#[cfg(feature = "etcd")]
impl EtcdStore {
    fn new(cluster_id: u64, member_id: u64) -> Self {
        Self {
            kvs: HashMap::new(),
            revision: 0,
            cluster_id,
            member_id,
        }
    }

    fn get_response_header(&self) -> ResponseHeader {
        ResponseHeader {
            cluster_id: self.cluster_id,
            member_id: self.member_id,
            revision: self.revision,
            raft_term: 1, // Simplified: always term 1
        }
    }

    fn increment_revision(&mut self) {
        self.revision += 1;
    }
}

/// etcd v3 server
pub struct EtcdServer;

#[cfg(feature = "etcd")]
impl EtcdServer {
    /// Spawn etcd server with LLM-controlled KV operations
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract cluster configuration
        let cluster_name = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("cluster_name"))
            .unwrap_or_else(|| "netget-cluster".to_string());

        let cluster_id = 0x6574636400000001u64; // "etcd" + 1
        let member_id = 0x6d656d6265720001u64; // "member" + 1 (shortened to fit u64)

        console_info!(status_tx, "etcd server starting on {} (cluster: {})", listen_addr, cluster_name);

        // Create in-memory store
        let store = Arc::new(Mutex::new(EtcdStore::new(cluster_id, member_id)));
        let protocol = Arc::new(EtcdProtocol::new());

        // Bind to address
        let listener = tokio::net::TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        console_info!(status_tx, "etcd server listening on {}", local_addr);

        // Spawn server task
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        console_debug!(status_tx, "etcd connection from {}", peer_addr);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let store_clone = store.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                peer_addr,
                                local_addr,
                                llm_clone,
                                state_clone,
                                status_clone,
                                server_id,
                                store_clone,
                                protocol_clone,
                            ).await {
                                error!("etcd connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "etcd accept error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    async fn handle_connection(
        stream: tokio::net::TcpStream,
        peer_addr: SocketAddr,
        local_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        store: Arc<Mutex<EtcdStore>>,
        protocol: Arc<EtcdProtocol>,
    ) -> Result<()> {
        use hyper::service::service_fn;
        use hyper_util::rt::TokioIo;

        let io = TokioIo::new(stream);

        let service = service_fn(move |req: Request<Incoming>| {
            let llm = llm_client.clone();
            let state = app_state.clone();
            let status = status_tx.clone();
            let store_ref = store.clone();
            let proto = protocol.clone();

            async move {
                Self::handle_grpc_request(
                    req,
                    peer_addr,
                    local_addr,
                    llm,
                    state,
                    status,
                    server_id,
                    store_ref,
                    proto,
                ).await
            }
        });

        hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
            .serve_connection(io, service)
            .await?;

        Ok(())
    }

    async fn handle_grpc_request(
        req: Request<Incoming>,
        _peer_addr: SocketAddr,
        _local_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        store: Arc<Mutex<EtcdStore>>,
        protocol: Arc<EtcdProtocol>,
    ) -> Result<Response<Full<Bytes>>> {
        // Store owned copies before consuming req
        let path = req.uri().path().to_string();
        let method = req.method().as_str().to_string();

        console_debug!(status_tx, "etcd gRPC {} {}", method, path);

        // Read request body
        let whole_body = req.collect().await?.to_bytes();

        // Parse gRPC frame: 5 bytes (compressed flag + length) + protobuf message
        if whole_body.len() < 5 {
            bail!("Invalid gRPC frame: too short");
        }

        let _compressed = whole_body[0];
        let msg_bytes = &whole_body[5..];

        // Route to appropriate handler based on path
        let response_bytes = match path.as_str() {
            "/etcdserverpb.KV/Range" => {
                Self::handle_range(msg_bytes, llm_client, app_state, status_tx, server_id, store, protocol).await?
            }
            "/etcdserverpb.KV/Put" => {
                Self::handle_put(msg_bytes, llm_client, app_state, status_tx, server_id, store, protocol).await?
            }
            "/etcdserverpb.KV/DeleteRange" => {
                Self::handle_delete_range(msg_bytes, llm_client, app_state, status_tx, server_id, store, protocol).await?
            }
            "/etcdserverpb.KV/Txn" => {
                Self::handle_txn(msg_bytes, llm_client, app_state, status_tx, server_id, store, protocol).await?
            }
            "/etcdserverpb.KV/Compact" => {
                Self::handle_compact(msg_bytes, llm_client, app_state, status_tx, server_id, store, protocol).await?
            }
            _ => {
                bail!("Unknown gRPC method: {}", path);
            }
        };

        // Build gRPC response with framing
        let mut response_with_frame = Vec::with_capacity(5 + response_bytes.len());
        response_with_frame.push(0); // Not compressed
        response_with_frame.extend_from_slice(&(response_bytes.len() as u32).to_be_bytes());
        response_with_frame.extend_from_slice(&response_bytes);

        // Build HTTP/2 response with gRPC headers
        let mut res = Response::new(Full::new(Bytes::from(response_with_frame)));
        *res.status_mut() = StatusCode::OK;
        res.headers_mut().insert("content-type", "application/grpc+proto".parse().unwrap());
        res.headers_mut().insert("grpc-status", "0".parse().unwrap()); // OK
        res.headers_mut().insert("grpc-message", "".parse().unwrap());

        Ok(res)
    }

    async fn handle_range(
        msg_bytes: &[u8],
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        store: Arc<Mutex<EtcdStore>>,
        protocol: Arc<EtcdProtocol>,
    ) -> Result<Vec<u8>> {
        let request = RangeRequest::decode(msg_bytes)?;

        let key_str = String::from_utf8_lossy(&request.key);
        console_debug!(status_tx, "etcd Range request: key={}", key_str);

        trace!("etcd Range request: {:?}", request);

        // Create event for LLM
        let event = Event::new(&ETCD_RANGE_REQUEST_EVENT, serde_json::json!({
            "key": key_str,
            "range_end": if request.range_end.is_empty() { None } else { Some(String::from_utf8_lossy(&request.range_end).to_string()) },
            "limit": request.limit,
        }));

        // Call LLM for decision
        let _execution_result = call_llm(
            &llm_client,
            &app_state,
            server_id,
            None,
            &event,
            protocol.as_ref(),
        ).await?;

        // For now, return empty response (LLM will control via actions in full implementation)
        let store_lock = store.lock().await;
        let response = RangeResponse {
            header: Some(store_lock.get_response_header()),
            kvs: vec![],
            more: false,
            count: 0,
        };

        let mut buf = Vec::new();
        response.encode(&mut buf)?;
        Ok(buf)
    }

    async fn handle_put(
        msg_bytes: &[u8],
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        store: Arc<Mutex<EtcdStore>>,
        _protocol: Arc<EtcdProtocol>,
    ) -> Result<Vec<u8>> {
        let request = PutRequest::decode(msg_bytes)?;

        let key_str = String::from_utf8_lossy(&request.key);
        let value_str = String::from_utf8_lossy(&request.value);
        console_debug!(status_tx, "etcd Put request: key={}, value={}", key_str, value_str);

        let mut store_lock = store.lock().await;
        store_lock.increment_revision();

        let response = PutResponse {
            header: Some(store_lock.get_response_header()),
            prev_kv: None,
        };

        let mut buf = Vec::new();
        response.encode(&mut buf)?;
        Ok(buf)
    }

    async fn handle_delete_range(
        msg_bytes: &[u8],
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        store: Arc<Mutex<EtcdStore>>,
        _protocol: Arc<EtcdProtocol>,
    ) -> Result<Vec<u8>> {
        let request = DeleteRangeRequest::decode(msg_bytes)?;

        let key_str = String::from_utf8_lossy(&request.key);
        console_debug!(status_tx, "etcd DeleteRange request: key={}", key_str);

        let mut store_lock = store.lock().await;
        store_lock.increment_revision();

        let response = DeleteRangeResponse {
            header: Some(store_lock.get_response_header()),
            deleted: 0,
            prev_kvs: vec![],
        };

        let mut buf = Vec::new();
        response.encode(&mut buf)?;
        Ok(buf)
    }

    async fn handle_txn(
        msg_bytes: &[u8],
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        store: Arc<Mutex<EtcdStore>>,
        _protocol: Arc<EtcdProtocol>,
    ) -> Result<Vec<u8>> {
        let _request = TxnRequest::decode(msg_bytes)?;

        console_debug!(status_tx, "etcd Txn request");

        let mut store_lock = store.lock().await;
        store_lock.increment_revision();

        let response = TxnResponse {
            header: Some(store_lock.get_response_header()),
            succeeded: false,
            responses: vec![],
        };

        let mut buf = Vec::new();
        response.encode(&mut buf)?;
        Ok(buf)
    }

    async fn handle_compact(
        msg_bytes: &[u8],
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        store: Arc<Mutex<EtcdStore>>,
        _protocol: Arc<EtcdProtocol>,
    ) -> Result<Vec<u8>> {
        let request = CompactionRequest::decode(msg_bytes)?;

        console_debug!(status_tx, "etcd Compact request: revision={}", request.revision);

        let store_lock = store.lock().await;

        let response = CompactionResponse {
            header: Some(store_lock.get_response_header()),
        };

        let mut buf = Vec::new();
        response.encode(&mut buf)?;
        Ok(buf)
    }
}
