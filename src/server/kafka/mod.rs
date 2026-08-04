//! Kafka broker — INCOMPLETE, no LLM integration and no working handshake
//!
//! `DevelopmentState::Incomplete` (see `actions.rs`), so it is hidden from the LLM.
//! Three independent reasons, any one of which is disqualifying:
//!
//! 1. **No client can negotiate.** `handle_api_versions` returns
//!    `ApiVersionsResponse::default()`, whose `api_keys` list is empty. ApiVersions is
//!    the first request every Kafka client sends; an empty list means "this broker
//!    supports nothing" and the client gives up. `tests/server/kafka/e2e_test.rs`
//!    records the symptom — rdkafka crashes against this server, so the tests fall
//!    back to asserting that a TCP connect succeeds.
//!
//! 2. **The API version is ignored everywhere.** Every `decode`/`encode` call below
//!    passes a hardcoded version 0. Request header v0 does not carry `client_id`
//!    (`kafka-protocol` gates it on `version >= 1`), so for the header v1/v2 that real
//!    clients send, the cursor is left inside the `client_id` string and every body
//!    field after it — topic names, partition indices, offsets — is garbage. Responses
//!    are likewise encoded at v0 whatever the client asked for.
//!
//! 3. **The LLM is never called.** `handle_metadata`, `handle_produce`, `handle_fetch`
//!    and `handle_offset_commit` all take `_llm_client`, `_app_state`, `_server_id` and
//!    `_protocol` and use none of them. Every response is computed by the hardcoded
//!    Rust below. Consequently all nine actions in `actions.rs` are dead code, none of
//!    the four declared event types is ever constructed, and — because handler
//!    dispatch lives inside `call_llm` — script and static `event_handlers` never fire.
//!
//! It also violates the project's no-storage rule outright: `topics` and
//! `consumer_offsets` are a real in-Rust broker database, written from unauthenticated
//! network input and never evicted. That is deliberately left in place rather than
//! half-removed, because ripping it out without the LLM path would leave a server that
//! answers nothing at all. Removing it is step 2 of the work described in CLAUDE.md.
//!
//! What *was* fixed here is the crash and denial-of-service surface, which was live
//! regardless of the maturity label: see `MIN_REQUEST_BYTES`, `MAX_REQUEST_BYTES`,
//! `MAX_PARTITIONS` and `MAX_TRACE_HEX_BYTES`.
//!
//! Correlation: `correlation_id` is the one thing this module gets right — it is
//! parsed from the request header and echoed into every response header. It is not
//! exposed to any handler, because no event is emitted.
//!
//! Uses the kafka-protocol crate for wire format parsing/serialization.

pub mod actions;

use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use crate::server::KafkaProtocol;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error, console_trace};
use anyhow::Result;
use bytes::Bytes;
use kafka_protocol::messages::{
    ApiKey, ApiVersionsResponse, FetchRequest, FetchResponse, MetadataRequest, MetadataResponse,
    OffsetCommitRequest, OffsetCommitResponse, ProduceRequest, ProduceResponse, RequestHeader,
    ResponseHeader,
};
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_protocol::records::{
    Compression, Record, RecordBatchDecoder, RecordBatchEncoder, RecordEncodeOptions,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Smallest useful Kafka request: api_key (i16) + api_version (i16) + correlation_id (i32).
const MIN_REQUEST_BYTES: i32 = 8;

/// Largest request accepted from a client. Real brokers cap this with
/// `socket.request.max.bytes` (default 100 MiB); without a cap the wire-supplied
/// size is an allocation primitive for any unauthenticated peer.
const MAX_REQUEST_BYTES: usize = 100 * 1024 * 1024;

/// Cap on how much of a request is hex-dumped at TRACE level.
const MAX_TRACE_HEX_BYTES: usize = 4096;

/// Upper bound on partitions the broker will materialise for one topic. The
/// partition index arrives on the wire as an i32 and was used directly to size a
/// Vec, so -1 (`usize::MAX` after the cast) looped until the allocator aborted.
const MAX_PARTITIONS: i32 = 1024;

/// Kafka broker server state
pub struct KafkaServer {
    /// Cluster ID
    cluster_id: String,
    /// Broker ID
    broker_id: i32,
    /// Auto-create topics on first produce
    _auto_create_topics: bool,
    /// Default partition count
    default_partitions: i32,
    /// Log retention hours
    _log_retention_hours: i64,
    /// Topic storage: topic_name -> partitions -> (offset, Vec<records>)
    topics: Arc<RwLock<HashMap<String, Vec<Vec<KafkaRecord>>>>>,
    /// Consumer group offsets: group_id -> topic -> partition -> offset
    consumer_offsets: Arc<RwLock<HashMap<String, HashMap<String, HashMap<i32, i64>>>>>,
}

/// Kafka record (simplified)
#[derive(Debug, Clone)]
struct KafkaRecord {
    offset: i64,
    key: Option<Vec<u8>>,
    value: Vec<u8>,
    timestamp: i64,
}

impl KafkaServer {
    /// Spawn Kafka server with LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract startup parameters with defaults
        let cluster_id = startup_params
            .as_ref()
            .map(|p| p.get_optional_string("cluster_id"))
            .transpose()?
            .flatten()
            .unwrap_or_else(|| "netget-kafka-1".to_string());
        let broker_id = startup_params
            .as_ref()
            .map(|p| p.get_optional_i64("broker_id"))
            .transpose()?
            .flatten()
            .unwrap_or(0) as i32;
        let auto_create_topics = startup_params
            .as_ref()
            .map(|p| p.get_optional_bool("auto_create_topics"))
            .transpose()?
            .flatten()
            .unwrap_or(true);
        // Clamped, not cast: this value comes from LLM or MCP-client JSON, and
        // `vec![Vec::new(); n as usize]` aborts the process on a negative n.
        let default_partitions = startup_params
            .as_ref()
            .map(|p| p.get_optional_i64("default_partitions"))
            .transpose()?
            .flatten()
            .unwrap_or(1)
            .clamp(1, MAX_PARTITIONS as i64) as i32;
        let log_retention_hours = startup_params
            .as_ref()
            .map(|p| p.get_optional_i64("log_retention_hours"))
            .transpose()?
            .flatten()
            .unwrap_or(168);

        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!(
            "Kafka broker listening on {} (cluster={}, broker_id={})",
            local_addr, cluster_id, broker_id
        );
        let _ = status_tx.send(format!(
            "[INFO] Kafka broker listening on {} (cluster={}, broker_id={})",
            local_addr, cluster_id, broker_id
        ));
        warn!(
            "Kafka server on {} is INCOMPLETE: ApiVersions advertises no supported APIs, every \
             request is decoded at hardcoded version 0, and the LLM is never called",
            local_addr
        );
        let _ = status_tx.send(
            "[WARN] Kafka is INCOMPLETE: no real client completes the ApiVersions handshake, and \
             the LLM is never consulted — instructions, script handlers and static handlers have \
             no effect. Responses come from hardcoded Rust."
                .to_string(),
        );

        let server = Arc::new(KafkaServer {
            cluster_id,
            broker_id,
            _auto_create_topics: auto_create_topics,
            default_partitions,
            _log_retention_hours: log_retention_hours,
            topics: Arc::new(RwLock::new(HashMap::new())),
            consumer_offsets: Arc::new(RwLock::new(HashMap::new())),
        });

        let protocol = Arc::new(KafkaProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        console_debug!(status_tx, "Kafka client connected from {}", peer_addr);

                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let server_clone = server.clone();
                        let protocol_clone = protocol.clone();

                        // Track connection in UI
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_clone.send("__UPDATE_UI__".to_string());

                        tokio::spawn(async move {
                            let result = Self::handle_connection(
                                stream,
                                peer_addr,
                                local_addr,
                                connection_id,
                                server_clone,
                                llm_clone,
                                state_clone.clone(),
                                status_clone.clone(),
                                server_id,
                                protocol_clone,
                            )
                            .await;

                            if let Err(e) = result {
                                error!("Kafka connection error: {}", e);
                            }

                            // Connections used to be added and never removed, so the
                            // TUI accumulated Active entries for dead sockets.
                            state_clone
                                .update_connection_status(
                                    server_id,
                                    connection_id,
                                    crate::state::server::ConnectionStatus::Closed,
                                )
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        // Without a break this spun a hot loop on a persistent
                        // error (EMFILE), saturating a core and flooding the
                        // unbounded status channel.
                        error!("Kafka accept error: {}", e);
                        console_error!(status_tx, "Kafka accept loop stopping: {}", e);
                        break;
                    }
                }
            }
        });

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }

    /// Handle a Kafka client connection
    async fn handle_connection(
        mut stream: TcpStream,
        peer_addr: SocketAddr,
        local_addr: SocketAddr,
        _connection_id: ConnectionId,
        server: Arc<KafkaServer>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: Arc<KafkaProtocol>,
    ) -> Result<()> {
        let mut buffer = vec![0u8; 8192]; // Kafka messages can be large

        loop {
            // Read the size prefix with read_exact. A plain read() may return 1-3
            // bytes, and the old code parsed all four regardless, mixing in stale
            // bytes from the previous message.
            let mut size_prefix = [0u8; 4];
            match stream.read_exact(&mut size_prefix).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    console_debug!(status_tx, "Kafka client {} disconnected", peer_addr);
                    break;
                }
                Err(e) => return Err(e.into()),
            }

            // Validate the declared size before it reaches any allocator. This is
            // unauthenticated input: `i32 as usize` sign-extends, so a prefix of
            // 0x80000000 became ~1.8e19 and aborted the process on Vec::resize,
            // while 0x7fffffff zeroed 2 GiB per connection. Sizes below the 8-byte
            // request header are equally unusable.
            let declared = i32::from_be_bytes(size_prefix);
            if declared < MIN_REQUEST_BYTES || declared as i64 > MAX_REQUEST_BYTES as i64 {
                warn!(
                    "Kafka client {} declared an invalid request size of {} bytes; closing",
                    peer_addr, declared
                );
                console_error!(
                    status_tx,
                    "Kafka client {} declared an invalid request size of {} bytes (allowed {}..={}); closing connection",
                    peer_addr,
                    declared,
                    MIN_REQUEST_BYTES,
                    MAX_REQUEST_BYTES
                );
                break;
            }
            let message_size = declared as usize;

            // Grow only: the buffer must never shrink below the size prefix.
            if buffer.len() < message_size {
                buffer.resize(message_size, 0);
            }
            stream.read_exact(&mut buffer[..message_size]).await?;

            console_debug!(
                status_tx,
                "Kafka received {} bytes from {}",
                message_size,
                peer_addr
            );

            // TRACE: hex dump, capped — hex::encode doubles the payload, so a
            // maximum-size request would otherwise build a 200 MiB String.
            if message_size <= MAX_TRACE_HEX_BYTES {
                console_trace!(
                    status_tx,
                    "Kafka raw message (hex): {}",
                    hex::encode(&buffer[..message_size])
                );
            } else {
                console_trace!(
                    status_tx,
                    "Kafka raw message: {} bytes (too large to hex dump; first {} bytes) {}",
                    message_size,
                    MAX_TRACE_HEX_BYTES,
                    hex::encode(&buffer[..MAX_TRACE_HEX_BYTES])
                );
            }

            // Parse request header
            let mut cursor = std::io::Cursor::new(&buffer[..message_size]);
            let header = match RequestHeader::decode(&mut cursor, 0) {
                Ok(h) => h,
                Err(e) => {
                    console_error!(status_tx, "Failed to parse Kafka request header: {}", e);
                    continue;
                }
            };

            console_debug!(
                status_tx,
                "Kafka request: API={:?}, correlation_id={}",
                header.request_api_key,
                header.correlation_id
            );

            // Handle different API keys
            let response_bytes = match header.request_api_key.try_into() {
                Ok(ApiKey::ApiVersions) => Self::handle_api_versions(&header, &status_tx).await?,
                Ok(ApiKey::Metadata) => {
                    Self::handle_metadata(
                        &header,
                        &buffer[..message_size],
                        &server,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        server_id,
                        &protocol,
                        peer_addr,
                        local_addr,
                    )
                    .await?
                }
                Ok(ApiKey::Produce) => {
                    Self::handle_produce(
                        &header,
                        &buffer[..message_size],
                        &server,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        server_id,
                        &protocol,
                        peer_addr,
                        local_addr,
                    )
                    .await?
                }
                Ok(ApiKey::Fetch) => {
                    Self::handle_fetch(
                        &header,
                        &buffer[..message_size],
                        &server,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        server_id,
                        &protocol,
                        peer_addr,
                        local_addr,
                    )
                    .await?
                }
                Ok(ApiKey::OffsetCommit) => {
                    Self::handle_offset_commit(
                        &header,
                        &buffer[..message_size],
                        &server,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        server_id,
                        &protocol,
                        peer_addr,
                        local_addr,
                    )
                    .await?
                }
                Ok(other_key) => {
                    console_debug!(status_tx, "Unsupported Kafka API: {:?}", other_key);
                    // Return error response
                    Self::create_error_response(&header, 35 /* UNSUPPORTED_VERSION */)
                }
                Err(_) => {
                    console_debug!(
                        status_tx,
                        "Invalid Kafka API key: {}",
                        header.request_api_key
                    );
                    Self::create_error_response(&header, 35)
                }
            };

            // Send response
            let response_size = (response_bytes.len() as i32).to_be_bytes();
            stream.write_all(&response_size).await?;
            stream.write_all(&response_bytes).await?;

            console_debug!(
                status_tx,
                "Kafka sent {} bytes to {}",
                response_bytes.len(),
                peer_addr
            );

            console_trace!(
                status_tx,
                "Kafka response (hex): {}",
                hex::encode(&response_bytes)
            );
        }

        Ok(())
    }

    /// Handle ApiVersions request
    async fn handle_api_versions(
        header: &RequestHeader,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<Vec<u8>> {
        debug!("Handling ApiVersions request");
        let _ = status_tx.send("[DEBUG] Handling ApiVersions request".to_string());

        // Build ApiVersions response with supported APIs
        let response = ApiVersionsResponse::default();

        let response_header = ResponseHeader::default().with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        response_header.encode(&mut buf, 0)?;
        response.encode(&mut buf, 0)?;

        Ok(buf)
    }

    /// Handle Metadata request (with LLM)
    async fn handle_metadata(
        header: &RequestHeader,
        message: &[u8],
        server: &Arc<KafkaServer>,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _protocol: &Arc<KafkaProtocol>,
        _peer_addr: SocketAddr,
        local_addr: SocketAddr,
    ) -> Result<Vec<u8>> {
        use kafka_protocol::messages::metadata_response::{
            MetadataResponseBroker, MetadataResponsePartition, MetadataResponseTopic,
        };
        use kafka_protocol::messages::BrokerId;
        use kafka_protocol::protocol::StrBytes;

        debug!("Handling Metadata request");
        let _ = status_tx.send("[DEBUG] Handling Metadata request".to_string());

        // Parse metadata request
        let mut cursor = std::io::Cursor::new(message);
        let _ = RequestHeader::decode(&mut cursor, 0)?; // Skip header
        let request = MetadataRequest::decode(&mut cursor, 0)?;

        // Extract requested topics
        let requested_topics: Vec<String> = request
            .topics
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t.name.as_ref().map(|n| n.to_string()))
            .collect();

        console_debug!(
            status_tx,
            "Metadata request for topics: {:?}",
            requested_topics
        );

        // Build broker info
        let broker = MetadataResponseBroker::default()
            .with_node_id(BrokerId(server.broker_id))
            .with_host("localhost".into())
            .with_port(local_addr.port() as i32);

        // Get topics from storage
        let topics_lock = server.topics.read().await;
        let mut response_topics = Vec::new();

        if requested_topics.is_empty() {
            // Return all topics
            for (topic_name, partitions) in topics_lock.iter() {
                let mut partition_metadata = Vec::new();
                for (partition_idx, _records) in partitions.iter().enumerate() {
                    partition_metadata.push(
                        MetadataResponsePartition::default()
                            .with_partition_index(partition_idx as i32)
                            .with_leader_id(BrokerId(server.broker_id))
                            .with_replica_nodes(vec![BrokerId(server.broker_id)])
                            .with_isr_nodes(vec![BrokerId(server.broker_id)])
                            .with_error_code(0),
                    );
                }

                response_topics.push(
                    MetadataResponseTopic::default()
                        .with_name(Some(StrBytes::from_string(topic_name.clone()).into()))
                        .with_partitions(partition_metadata)
                        .with_error_code(0),
                );
            }
        } else {
            // Return only requested topics
            for topic_name in &requested_topics {
                if let Some(partitions) = topics_lock.get(topic_name) {
                    let mut partition_metadata = Vec::new();
                    for (partition_idx, _records) in partitions.iter().enumerate() {
                        partition_metadata.push(
                            MetadataResponsePartition::default()
                                .with_partition_index(partition_idx as i32)
                                .with_leader_id(BrokerId(server.broker_id))
                                .with_replica_nodes(vec![BrokerId(server.broker_id)])
                                .with_isr_nodes(vec![BrokerId(server.broker_id)])
                                .with_error_code(0),
                        );
                    }

                    response_topics.push(
                        MetadataResponseTopic::default()
                            .with_name(Some(StrBytes::from_string(topic_name.clone()).into()))
                            .with_partitions(partition_metadata)
                            .with_error_code(0),
                    );
                } else {
                    // Topic doesn't exist
                    response_topics.push(
                        MetadataResponseTopic::default()
                            .with_name(Some(StrBytes::from_string(topic_name.clone()).into()))
                            .with_error_code(3), // Unknown topic
                    );
                }
            }
        }

        info!(
            "Returning metadata for {} topic(s), {} broker(s)",
            response_topics.len(),
            1
        );
        let _ = status_tx.send(format!(
            "[INFO] Returning metadata for {} topic(s)",
            response_topics.len()
        ));

        // Build response
        let response = MetadataResponse::default()
            .with_brokers(vec![broker])
            .with_cluster_id(Some(server.cluster_id.clone().into()))
            .with_controller_id(BrokerId(server.broker_id))
            .with_topics(response_topics);

        let response_header = ResponseHeader::default().with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        response_header.encode(&mut buf, 0)?;
        response.encode(&mut buf, 0)?;

        Ok(buf)
    }

    /// Handle Produce request (with LLM)
    async fn handle_produce(
        header: &RequestHeader,
        message: &[u8],
        server: &Arc<KafkaServer>,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _protocol: &Arc<KafkaProtocol>,
        _peer_addr: SocketAddr,
        _local_addr: SocketAddr,
    ) -> Result<Vec<u8>> {
        use kafka_protocol::messages::produce_response::{
            PartitionProduceResponse, TopicProduceResponse,
        };
        use kafka_protocol::protocol::StrBytes;

        debug!("Handling Produce request");
        let _ = status_tx.send("[DEBUG] Handling Produce request".to_string());

        // Parse produce request
        let mut cursor = std::io::Cursor::new(message);
        let _ = RequestHeader::decode(&mut cursor, 0)?; // Skip header
        let request = ProduceRequest::decode(&mut cursor, 0)?;

        let mut topic_responses = Vec::new();
        let mut topics_lock = server.topics.write().await;

        // Process each topic
        for topic_data in &request.topic_data {
            let topic_name = topic_data.name.to_string();
            let mut partition_responses = Vec::new();

            // Process each partition
            for partition_data in &topic_data.partition_data {
                let partition_idx = partition_data.index;

                // The index is unvalidated wire input. -1 cast to usize is
                // usize::MAX, which made the loop below push Vec::new() until the
                // allocator aborted the process; 2_000_000_000 is a legal i32 that
                // asks for ~48 GB. Refuse anything outside the supported range with
                // UNKNOWN_TOPIC_OR_PARTITION (3).
                if partition_idx < 0 || partition_idx > MAX_PARTITIONS {
                    warn!(
                        "Kafka produce for '{}' names out-of-range partition {}",
                        topic_name, partition_idx
                    );
                    console_error!(
                        status_tx,
                        "Kafka produce for '{}' names out-of-range partition {} (0..={}); rejected",
                        topic_name,
                        partition_idx,
                        MAX_PARTITIONS
                    );
                    partition_responses.push(
                        PartitionProduceResponse::default()
                            .with_index(partition_idx)
                            .with_error_code(3)
                            .with_base_offset(-1),
                    );
                    continue;
                }

                // Auto-create topic if needed
                let default_partitions = server.default_partitions;
                let partitions = topics_lock.entry(topic_name.clone()).or_insert_with(|| {
                    info!(
                        "Auto-creating topic '{}' with {} partition(s)",
                        topic_name, default_partitions
                    );
                    let _ = status_tx.send(format!("[INFO] Auto-creating topic '{}'", topic_name));
                    vec![Vec::new(); default_partitions as usize]
                });

                // Ensure partition exists
                while partitions.len() <= partition_idx as usize {
                    partitions.push(Vec::new());
                }

                let partition = &mut partitions[partition_idx as usize];

                // Parse records from batch
                let mut record_count = 0;
                if let Some(records_bytes) = &partition_data.records {
                    // Parse records using RecordBatchDecoder
                    // Convert to owned Bytes for parsing
                    let owned_bytes = Bytes::copy_from_slice(records_bytes.as_ref());
                    let mut records_cursor = std::io::Cursor::new(owned_bytes);

                    match RecordBatchDecoder::decode_with_custom_compression::<
                        _,
                        fn(&mut Bytes, Compression) -> Result<std::io::Cursor<Bytes>>,
                    >(
                        &mut records_cursor,
                        None::<fn(&mut Bytes, Compression) -> Result<std::io::Cursor<Bytes>>>,
                    ) {
                        Ok(decoded_records) => {
                            debug!(
                                "Parsed {} record(s) from batch ({} bytes)",
                                decoded_records.len(),
                                records_bytes.len()
                            );
                            record_count = decoded_records.len();

                            // Store records in partition
                            let base_offset = partition.len() as i64;
                            for (idx, record) in decoded_records.into_iter().enumerate() {
                                partition.push(KafkaRecord {
                                    offset: base_offset + idx as i64,
                                    key: record.key.map(|k| k.to_vec()),
                                    value: record.value.map(|v| v.to_vec()).unwrap_or_default(),
                                    timestamp: record.timestamp,
                                });
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse record batch: {:?}, storing placeholder", e);
                            let _ =
                                status_tx.send(format!("[WARN] Failed to parse records: {:?}", e));

                            // Store a placeholder record on parse failure
                            let offset = partition.len() as i64;
                            partition.push(KafkaRecord {
                                offset,
                                key: None,
                                value: vec![],
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            });
                            record_count = 1;
                        }
                    }
                }

                // Get base offset (first assigned offset)
                let base_offset = if partition.is_empty() {
                    0
                } else {
                    partition.len() as i64 - record_count as i64
                };

                info!(
                    "Produced {} record(s) to topic '{}' partition {} at offset {}",
                    record_count, topic_name, partition_idx, base_offset
                );
                let _ = status_tx.send(format!(
                    "[INFO] Produced {} record(s) to '{}' partition {}",
                    record_count, topic_name, partition_idx
                ));

                partition_responses.push(
                    PartitionProduceResponse::default()
                        .with_index(partition_idx)
                        .with_base_offset(base_offset)
                        .with_error_code(0),
                );
            }

            topic_responses.push(
                TopicProduceResponse::default()
                    .with_name(StrBytes::from_string(topic_name).into())
                    .with_partition_responses(partition_responses),
            );
        }

        drop(topics_lock);

        // Build response
        let response = ProduceResponse::default()
            .with_responses(topic_responses)
            .with_throttle_time_ms(0);

        let response_header = ResponseHeader::default().with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        response_header.encode(&mut buf, 0)?;
        response.encode(&mut buf, 0)?;

        Ok(buf)
    }

    /// Handle Fetch request (with LLM)
    async fn handle_fetch(
        header: &RequestHeader,
        message: &[u8],
        server: &Arc<KafkaServer>,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _protocol: &Arc<KafkaProtocol>,
        _peer_addr: SocketAddr,
        _local_addr: SocketAddr,
    ) -> Result<Vec<u8>> {
        use bytes::Bytes;
        use kafka_protocol::messages::fetch_response::{FetchableTopicResponse, PartitionData};
        use kafka_protocol::protocol::StrBytes;

        debug!("Handling Fetch request");
        let _ = status_tx.send("[DEBUG] Handling Fetch request".to_string());

        // Parse fetch request
        let mut cursor = std::io::Cursor::new(message);
        let _ = RequestHeader::decode(&mut cursor, 0)?; // Skip header
        let request = FetchRequest::decode(&mut cursor, 0)?;

        let mut topic_responses = Vec::new();
        let topics_lock = server.topics.read().await;

        // Process each topic
        for topic in &request.topics {
            let topic_name = topic.topic.to_string();
            let mut partition_responses = Vec::new();

            // Process each partition
            for partition in &topic.partitions {
                let partition_idx = partition.partition;
                let fetch_offset = partition.fetch_offset;

                // Get topic and partition
                if let Some(partitions) = topics_lock.get(&topic_name) {
                    if let Some(records) = partitions.get(partition_idx as usize) {
                        // Find records starting from fetch_offset
                        let matching_records: Vec<_> = records
                            .iter()
                            .filter(|r| r.offset >= fetch_offset)
                            .collect();

                        if !matching_records.is_empty() {
                            let _base_offset = matching_records[0].offset;

                            // Convert stored records to kafka-protocol Record format
                            let kafka_records: Vec<Record> = matching_records
                                .iter()
                                .map(|r| Record {
                                    transactional: false,
                                    control: false,
                                    partition_leader_epoch: 0,
                                    producer_id: -1,
                                    producer_epoch: -1,
                                    timestamp_type:
                                        kafka_protocol::records::TimestampType::Creation,
                                    offset: r.offset,
                                    sequence: 0,
                                    timestamp: r.timestamp,
                                    key: r.key.as_ref().map(|k| Bytes::copy_from_slice(k)),
                                    value: Some(Bytes::copy_from_slice(&r.value)),
                                    headers: Default::default(),
                                })
                                .collect();

                            // Encode records into batch
                            let mut records_buf = Vec::new();
                            let encode_options = RecordEncodeOptions {
                                version: 2, // Use record batch format (version 2)
                                compression: Compression::None,
                            };

                            match RecordBatchEncoder::encode_with_custom_compression::<
                                _,
                                _,
                                fn(&mut bytes::BytesMut, &mut Vec<u8>, Compression) -> Result<()>,
                            >(
                                &mut records_buf,
                                &kafka_records,
                                &encode_options,
                                None::<
                                    fn(
                                        &mut bytes::BytesMut,
                                        &mut Vec<u8>,
                                        Compression,
                                    ) -> Result<()>,
                                >,
                            ) {
                                Ok(_) => {
                                    debug!(
                                        "Encoded {} record(s) into {} bytes",
                                        kafka_records.len(),
                                        records_buf.len()
                                    );

                                    info!("Fetched {} record(s) from topic '{}' partition {} starting at offset {}",
                                          matching_records.len(), topic_name, partition_idx, fetch_offset);
                                    let _ = status_tx.send(format!(
                                        "[INFO] Fetched {} record(s) from '{}' partition {}",
                                        matching_records.len(),
                                        topic_name,
                                        partition_idx
                                    ));

                                    partition_responses.push(
                                        PartitionData::default()
                                            .with_partition_index(partition_idx)
                                            .with_high_watermark(records.len() as i64)
                                            .with_records(Some(Bytes::from(records_buf)))
                                            .with_error_code(0),
                                    );
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to encode records: {:?}, returning empty batch",
                                        e
                                    );
                                    let _ = status_tx
                                        .send(format!("[WARN] Failed to encode records: {:?}", e));

                                    // Return empty records on encoding failure
                                    partition_responses.push(
                                        PartitionData::default()
                                            .with_partition_index(partition_idx)
                                            .with_high_watermark(records.len() as i64)
                                            .with_error_code(0),
                                    );
                                }
                            }
                        } else {
                            // No records at this offset
                            debug!(
                                "No records at offset {} for topic '{}' partition {}",
                                fetch_offset, topic_name, partition_idx
                            );
                            partition_responses.push(
                                PartitionData::default()
                                    .with_partition_index(partition_idx)
                                    .with_high_watermark(records.len() as i64)
                                    .with_error_code(0),
                            );
                        }
                    } else {
                        // Partition doesn't exist
                        partition_responses.push(
                            PartitionData::default()
                                .with_partition_index(partition_idx)
                                .with_error_code(6), // Invalid partition
                        );
                    }
                } else {
                    // Topic doesn't exist
                    partition_responses.push(
                        PartitionData::default()
                            .with_partition_index(partition_idx)
                            .with_error_code(3), // Unknown topic
                    );
                }
            }

            topic_responses.push(
                FetchableTopicResponse::default()
                    .with_topic(StrBytes::from_string(topic_name).into())
                    .with_partitions(partition_responses),
            );
        }

        drop(topics_lock);

        // Build response
        let response = FetchResponse::default()
            .with_responses(topic_responses)
            .with_throttle_time_ms(0);

        let response_header = ResponseHeader::default().with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        response_header.encode(&mut buf, 0)?;
        response.encode(&mut buf, 0)?;

        Ok(buf)
    }

    /// Handle OffsetCommit request (with LLM)
    async fn handle_offset_commit(
        header: &RequestHeader,
        message: &[u8],
        server: &Arc<KafkaServer>,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _protocol: &Arc<KafkaProtocol>,
        _peer_addr: SocketAddr,
        _local_addr: SocketAddr,
    ) -> Result<Vec<u8>> {
        use kafka_protocol::messages::offset_commit_response::{
            OffsetCommitResponsePartition, OffsetCommitResponseTopic,
        };
        use kafka_protocol::protocol::StrBytes;

        debug!("Handling OffsetCommit request");
        let _ = status_tx.send("[DEBUG] Handling OffsetCommit request".to_string());

        // Parse offset commit request
        let mut cursor = std::io::Cursor::new(message);
        let _ = RequestHeader::decode(&mut cursor, 0)?; // Skip header
        let request = OffsetCommitRequest::decode(&mut cursor, 0)?;

        let group_id = request.group_id.to_string();
        let mut topic_responses = Vec::new();
        let mut offsets_lock = server.consumer_offsets.write().await;

        // Get or create group
        let group_offsets = offsets_lock
            .entry(group_id.clone())
            .or_insert_with(HashMap::new);

        // Process each topic
        for topic in &request.topics {
            let topic_name = topic.name.to_string();
            let mut partition_responses = Vec::new();

            // Get or create topic
            let topic_offsets = group_offsets
                .entry(topic_name.clone())
                .or_insert_with(HashMap::new);

            // Process each partition
            for partition in &topic.partitions {
                let partition_idx = partition.partition_index;
                let committed_offset = partition.committed_offset;

                // Store offset
                topic_offsets.insert(partition_idx, committed_offset);

                info!(
                    "Consumer group '{}' committed offset {} for topic '{}' partition {}",
                    group_id, committed_offset, topic_name, partition_idx
                );
                let _ = status_tx.send(format!(
                    "[INFO] Group '{}' committed offset {} for '{}' partition {}",
                    group_id, committed_offset, topic_name, partition_idx
                ));

                partition_responses.push(
                    OffsetCommitResponsePartition::default()
                        .with_partition_index(partition_idx)
                        .with_error_code(0),
                );
            }

            topic_responses.push(
                OffsetCommitResponseTopic::default()
                    .with_name(StrBytes::from_string(topic_name).into())
                    .with_partitions(partition_responses),
            );
        }

        drop(offsets_lock);

        // Build response
        let response = OffsetCommitResponse::default()
            .with_topics(topic_responses)
            .with_throttle_time_ms(0);

        let response_header = ResponseHeader::default().with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        response_header.encode(&mut buf, 0)?;
        response.encode(&mut buf, 0)?;

        Ok(buf)
    }

    /// Create error response
    fn create_error_response(header: &RequestHeader, error_code: i16) -> Vec<u8> {
        let response_header = ResponseHeader::default().with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        let _ = response_header.encode(&mut buf, 0);
        // Add error code
        buf.extend_from_slice(&error_code.to_be_bytes());

        buf
    }
}
