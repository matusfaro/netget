//! Kafka broker server implementation
//!
//! Implements a simplified Kafka broker that integrates with LLM for message routing and topic management.
//! Uses kafka-protocol crate for wire format parsing/serialization.

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::KafkaProtocol;
use crate::state::app_state::AppState;
use actions::{
    FETCH_REQUEST_EVENT, METADATA_REQUEST_EVENT, OFFSET_COMMIT_REQUEST_EVENT,
    PRODUCE_REQUEST_EVENT,
};
use anyhow::Result;
use kafka_protocol::messages::{
    ApiKey, ApiVersionsResponse, FetchRequest, FetchResponse,
    MetadataRequest, MetadataResponse, OffsetCommitRequest, OffsetCommitResponse, ProduceRequest,
    ProduceResponse, RequestHeader, ResponseHeader,
};
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_protocol::records::{Record, RecordBatchDecoder, RecordBatchEncoder, RecordEncodeOptions, Compression};
use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, trace, warn};

/// Kafka broker server state
pub struct KafkaServer {
    /// Cluster ID
    cluster_id: String,
    /// Broker ID
    broker_id: i32,
    /// Auto-create topics on first produce
    auto_create_topics: bool,
    /// Default partition count
    default_partitions: i32,
    /// Log retention hours
    log_retention_hours: i64,
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
            .and_then(|p| p.get_optional_string("cluster_id"))
            .unwrap_or_else(|| "netget-kafka-1".to_string());
        let broker_id = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_i64("broker_id"))
            .unwrap_or(0) as i32;
        let auto_create_topics = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_bool("auto_create_topics"))
            .unwrap_or(true);
        let default_partitions = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_i64("default_partitions"))
            .unwrap_or(1) as i32;
        let log_retention_hours = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_i64("log_retention_hours"))
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

        let server = Arc::new(KafkaServer {
            cluster_id,
            broker_id,
            auto_create_topics,
            default_partitions,
            log_retention_hours,
            topics: Arc::new(RwLock::new(HashMap::new())),
            consumer_offsets: Arc::new(RwLock::new(HashMap::new())),
        });

        let protocol = Arc::new(KafkaProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        debug!("Kafka client connected from {}", peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] Kafka client connected from {}", peer_addr));

                        let connection_id = ConnectionId::new();
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
                            protocol_info: ProtocolConnectionInfo::Kafka {
                                recent_requests: vec![],
                            },
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_clone.send("__UPDATE_UI__".to_string());

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                peer_addr,
                                local_addr,
                                connection_id,
                                server_clone,
                                llm_clone,
                                state_clone,
                                status_clone,
                                server_id,
                                protocol_clone,
                            )
                            .await
                            {
                                error!("Kafka connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Kafka accept error: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Kafka accept error: {}", e));
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle a Kafka client connection
    async fn handle_connection(
        mut stream: TcpStream,
        peer_addr: SocketAddr,
        local_addr: SocketAddr,
        connection_id: ConnectionId,
        server: Arc<KafkaServer>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: Arc<KafkaProtocol>,
    ) -> Result<()> {
        let mut buffer = vec![0u8; 8192]; // Kafka messages can be large

        loop {
            // Read message size (4 bytes, big-endian)
            let n = stream.read(&mut buffer[..4]).await?;
            if n == 0 {
                debug!("Kafka client {} disconnected", peer_addr);
                let _ = status_tx.send(format!("[DEBUG] Kafka client {} disconnected", peer_addr));
                break;
            }

            let message_size = i32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;

            // Read full message
            buffer.resize(message_size, 0);
            stream.read_exact(&mut buffer[..message_size]).await?;

            debug!("Kafka received {} bytes from {}", message_size, peer_addr);
            let _ = status_tx.send(format!("[DEBUG] Kafka received {} bytes from {}", message_size, peer_addr));

            // TRACE: Log hex dump of raw message
            trace!("Kafka raw message (hex): {}", hex::encode(&buffer[..message_size]));
            let _ = status_tx.send(format!("[TRACE] Kafka raw message (hex): {}", hex::encode(&buffer[..message_size])));

            // Parse request header
            let mut cursor = std::io::Cursor::new(&buffer[..message_size]);
            let header = match RequestHeader::decode(&mut cursor, 0) {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to parse Kafka request header: {}", e);
                    let _ = status_tx.send(format!("[ERROR] Failed to parse Kafka request header: {}", e));
                    continue;
                }
            };

            debug!("Kafka request: API={:?}, correlation_id={}", header.request_api_key, header.correlation_id);
            let _ = status_tx.send(format!("[DEBUG] Kafka request: API={:?}, correlation_id={}", header.request_api_key, header.correlation_id));

            // Handle different API keys
            let response_bytes = match header.request_api_key.try_into() {
                Ok(ApiKey::ApiVersions) => {
                    Self::handle_api_versions(&header, &status_tx).await?
                }
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
                    debug!("Unsupported Kafka API: {:?}", other_key);
                    let _ = status_tx.send(format!("[DEBUG] Unsupported Kafka API: {:?}", other_key));
                    // Return error response
                    Self::create_error_response(&header, 35 /* UNSUPPORTED_VERSION */)
                }
                Err(_) => {
                    debug!("Invalid Kafka API key: {}", header.request_api_key);
                    let _ = status_tx.send(format!("[DEBUG] Invalid Kafka API key: {}", header.request_api_key));
                    Self::create_error_response(&header, 35)
                }
            };

            // Send response
            let response_size = (response_bytes.len() as i32).to_be_bytes();
            stream.write_all(&response_size).await?;
            stream.write_all(&response_bytes).await?;

            debug!("Kafka sent {} bytes to {}", response_bytes.len(), peer_addr);
            let _ = status_tx.send(format!("[DEBUG] Kafka sent {} bytes to {}", response_bytes.len(), peer_addr));

            trace!("Kafka response (hex): {}", hex::encode(&response_bytes));
            let _ = status_tx.send(format!("[TRACE] Kafka response (hex): {}", hex::encode(&response_bytes)));
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

        let response_header = ResponseHeader::default()
            .with_correlation_id(header.correlation_id);

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
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: &Arc<KafkaProtocol>,
        _peer_addr: SocketAddr,
        local_addr: SocketAddr,
    ) -> Result<Vec<u8>> {
        use kafka_protocol::messages::metadata_response::{MetadataResponseBroker, MetadataResponseTopic, MetadataResponsePartition};
        use kafka_protocol::messages::{BrokerId, TopicName};
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

        debug!("Metadata request for topics: {:?}", requested_topics);
        let _ = status_tx.send(format!("[DEBUG] Metadata request for topics: {:?}", requested_topics));

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

        info!("Returning metadata for {} topic(s), {} broker(s)", response_topics.len(), 1);
        let _ = status_tx.send(format!("[INFO] Returning metadata for {} topic(s)", response_topics.len()));

        // Build response
        let response = MetadataResponse::default()
            .with_brokers(vec![broker])
            .with_cluster_id(Some(server.cluster_id.clone().into()))
            .with_controller_id(BrokerId(server.broker_id))
            .with_topics(response_topics);

        let response_header = ResponseHeader::default()
            .with_correlation_id(header.correlation_id);

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
        use kafka_protocol::messages::produce_response::{TopicProduceResponse, PartitionProduceResponse};
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

                // Auto-create topic if needed
                let partitions = topics_lock.entry(topic_name.clone()).or_insert_with(|| {
                    info!("Auto-creating topic '{}' with {} partition(s)", topic_name, server.default_partitions);
                    let _ = status_tx.send(format!("[INFO] Auto-creating topic '{}'", topic_name));
                    vec![Vec::new(); server.default_partitions as usize]
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

                    match RecordBatchDecoder::decode_with_custom_compression::<_, fn(&mut Bytes, Compression) -> Result<std::io::Cursor<Bytes>>>(
                        &mut records_cursor,
                        None::<fn(&mut Bytes, Compression) -> Result<std::io::Cursor<Bytes>>>,
                    ) {
                        Ok(decoded_records) => {
                            debug!("Parsed {} record(s) from batch ({} bytes)", decoded_records.len(), records_bytes.len());
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
                            let _ = status_tx.send(format!("[WARN] Failed to parse records: {:?}", e));

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

                info!("Produced {} record(s) to topic '{}' partition {} at offset {}",
                      record_count, topic_name, partition_idx, base_offset);
                let _ = status_tx.send(format!("[INFO] Produced {} record(s) to '{}' partition {}",
                                                record_count, topic_name, partition_idx));

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

        let response_header = ResponseHeader::default()
            .with_correlation_id(header.correlation_id);

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
        use kafka_protocol::messages::fetch_response::{FetchableTopicResponse, PartitionData};
        use kafka_protocol::protocol::StrBytes;
        use bytes::Bytes;

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
                            let base_offset = matching_records[0].offset;

                            // Convert stored records to kafka-protocol Record format
                            let kafka_records: Vec<Record> = matching_records
                                .iter()
                                .map(|r| Record {
                                    transactional: false,
                                    control: false,
                                    partition_leader_epoch: 0,
                                    producer_id: -1,
                                    producer_epoch: -1,
                                    timestamp_type: kafka_protocol::records::TimestampType::Creation,
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

                            match RecordBatchEncoder::encode_with_custom_compression::<_, _, fn(&mut bytes::BytesMut, &mut Vec<u8>, Compression) -> Result<()>>(
                                &mut records_buf,
                                &kafka_records,
                                &encode_options,
                                None::<fn(&mut bytes::BytesMut, &mut Vec<u8>, Compression) -> Result<()>>,
                            ) {
                                Ok(_) => {
                                    debug!("Encoded {} record(s) into {} bytes", kafka_records.len(), records_buf.len());

                                    info!("Fetched {} record(s) from topic '{}' partition {} starting at offset {}",
                                          matching_records.len(), topic_name, partition_idx, fetch_offset);
                                    let _ = status_tx.send(format!("[INFO] Fetched {} record(s) from '{}' partition {}",
                                                                    matching_records.len(), topic_name, partition_idx));

                                    partition_responses.push(
                                        PartitionData::default()
                                            .with_partition_index(partition_idx)
                                            .with_high_watermark(records.len() as i64)
                                            .with_records(Some(Bytes::from(records_buf)))
                                            .with_error_code(0),
                                    );
                                }
                                Err(e) => {
                                    warn!("Failed to encode records: {:?}, returning empty batch", e);
                                    let _ = status_tx.send(format!("[WARN] Failed to encode records: {:?}", e));

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
                            debug!("No records at offset {} for topic '{}' partition {}",
                                   fetch_offset, topic_name, partition_idx);
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

        let response_header = ResponseHeader::default()
            .with_correlation_id(header.correlation_id);

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
        use kafka_protocol::messages::offset_commit_response::{OffsetCommitResponseTopic, OffsetCommitResponsePartition};
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
        let group_offsets = offsets_lock.entry(group_id.clone()).or_insert_with(HashMap::new);

        // Process each topic
        for topic in &request.topics {
            let topic_name = topic.name.to_string();
            let mut partition_responses = Vec::new();

            // Get or create topic
            let topic_offsets = group_offsets.entry(topic_name.clone()).or_insert_with(HashMap::new);

            // Process each partition
            for partition in &topic.partitions {
                let partition_idx = partition.partition_index;
                let committed_offset = partition.committed_offset;

                // Store offset
                topic_offsets.insert(partition_idx, committed_offset);

                info!("Consumer group '{}' committed offset {} for topic '{}' partition {}",
                      group_id, committed_offset, topic_name, partition_idx);
                let _ = status_tx.send(format!("[INFO] Group '{}' committed offset {} for '{}' partition {}",
                                                group_id, committed_offset, topic_name, partition_idx));

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

        let response_header = ResponseHeader::default()
            .with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        response_header.encode(&mut buf, 0)?;
        response.encode(&mut buf, 0)?;

        Ok(buf)
    }

    /// Create error response
    fn create_error_response(header: &RequestHeader, error_code: i16) -> Vec<u8> {
        let response_header = ResponseHeader::default()
            .with_correlation_id(header.correlation_id);

        let mut buf = Vec::new();
        let _ = response_header.encode(&mut buf, 0);
        // Add error code
        buf.extend_from_slice(&error_code.to_be_bytes());

        buf
    }
}
