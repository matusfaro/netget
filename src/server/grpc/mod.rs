//! gRPC server implementation with dynamic schema support
//!
//! Implements a gRPC server where the LLM provides protobuf schema definitions
//! and controls RPC request/response handling through JSON.

pub mod actions;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};
use anyhow::{Result, Context, bail};

#[cfg(feature = "grpc")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "grpc")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "grpc")]
use crate::llm::ActionResult;
#[cfg(feature = "grpc")]
use crate::state::app_state::AppState;
#[cfg(feature = "grpc")]
use crate::server::GrpcProtocol;
#[cfg(feature = "grpc")]
use crate::protocol::Event;
#[cfg(feature = "grpc")]
use crate::server::grpc::actions::GRPC_UNARY_REQUEST_EVENT;
#[cfg(feature = "grpc")]
use prost_reflect::{DescriptorPool, DynamicMessage, ReflectMessage};
#[cfg(feature = "grpc")]
use prost_types::FileDescriptorSet;
#[cfg(feature = "grpc")]
use prost::Message;
#[cfg(feature = "grpc")]
use serde_json::json;
#[cfg(feature = "grpc")]
use hyper::{Request, Response, body::Incoming, StatusCode};
#[cfg(feature = "grpc")]
use http_body_util::{BodyExt, Full};
#[cfg(feature = "grpc")]
use bytes::Bytes;

/// gRPC server with dynamic schema support
pub struct GrpcServer;

#[cfg(feature = "grpc")]
impl GrpcServer {
    /// Spawn gRPC server with LLM-provided schema and actions
    ///
    /// The LLM provides a protobuf schema definition (as a string) via startup_params.
    /// The server parses requests into JSON, sends to LLM, and encodes JSON responses back to protobuf.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract proto schema from startup params
        let proto_schema = startup_params
            .as_ref()
            .map(|p| p.get_string("proto_schema"))
            .context("Missing 'proto_schema' in startup_params. LLM must provide protobuf definition.")?;

        // Enable reflection by default (can be disabled in startup_params)
        let enable_reflection = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_bool("enable_reflection"))
            .unwrap_or(true);

        debug!("Compiling protobuf schema for gRPC server");
        trace!("Proto schema:\n{}", proto_schema);
        let _ = status_tx.send(format!("[DEBUG] Compiling protobuf schema"));

        // Compile proto schema to FileDescriptorSet
        let file_descriptor_set = Self::compile_proto_schema(&proto_schema)
            .context("Failed to compile protobuf schema")?;

        // Build descriptor pool for dynamic message handling
        let mut fd_bytes = Vec::new();
        file_descriptor_set.encode(&mut fd_bytes)?;
        let descriptor_pool = DescriptorPool::decode(fd_bytes.as_slice())
            .context("Failed to create descriptor pool from FileDescriptorSet")?;

        let services = descriptor_pool.services().collect::<Vec<_>>();

        if services.is_empty() {
            bail!("No services found in protobuf schema. Schema must define at least one service.");
        }

        info!("gRPC server starting with {} service(s)", services.len());
        for service in &services {
            info!("  Service: {} ({} methods)", service.full_name(), service.methods().count());
            let _ = status_tx.send(format!("[INFO] gRPC service: {} ({} methods)",
                service.full_name(), service.methods().count()));
        }

        // Create gRPC server with dynamic handler
        let protocol = Arc::new(GrpcProtocol::new());
        let descriptor_pool_arc = Arc::new(descriptor_pool.clone());

        // Build reflection service if enabled
        let _reflection_service = if enable_reflection {
            info!("gRPC reflection enabled");
            let _ = status_tx.send("[INFO] gRPC reflection enabled".to_string());

            let mut fd_bytes_refl = Vec::new();
            file_descriptor_set.encode(&mut fd_bytes_refl)?;

            Some(tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(fd_bytes_refl.as_slice())
                .build_v1()
                .context("Failed to build gRPC reflection service")?)
        } else {
            info!("gRPC reflection disabled");
            None
        };

        // Create dynamic gRPC service
        let dynamic_service = DynamicGrpcService {
            llm_client: llm_client.clone(),
            app_state: app_state.clone(),
            status_tx: status_tx.clone(),
            server_id,
            descriptor_pool: descriptor_pool_arc,
            protocol,
        };

        // Start HTTP/2 server for gRPC
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let actual_addr = listener.local_addr()?;

        info!("gRPC server listening on {}", actual_addr);
        let _ = status_tx.send(format!("[INFO] gRPC server listening on {}", actual_addr));

        // Spawn server loop
        let service = Arc::new(dynamic_service);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state_clone.get_next_unified_id().await
                        );
                        debug!("gRPC connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[DEBUG] gRPC connection from {}", remote_addr));

                        // Add connection to server state
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: actual_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let service_clone = service.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();

                        // Spawn connection handler
                        tokio::spawn(async move {
                            let io = hyper_util::rt::TokioIo::new(stream);

                            // Create service function for this connection
                            let grpc_service = hyper::service::service_fn(move |req| {
                                let service = service_clone.clone();
                                let conn_id = connection_id;
                                async move {
                                    service.handle_grpc_request(req, conn_id).await
                                }
                            });

                            // Serve HTTP/2 connection
                            if let Err(e) = hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                                .serve_connection(io, grpc_service)
                                .await
                            {
                                debug!("gRPC connection error: {}", e);
                                let _ = status_tx_clone.send(format!("[DEBUG] gRPC connection error: {}", e));
                            }

                            // Clean up connection
                            app_state_clone.remove_connection_from_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept gRPC connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept gRPC connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(actual_addr)
    }

    /// Parse protobuf schema into FileDescriptorSet
    ///
    /// Supports multiple input formats:
    /// 1. Base64-encoded FileDescriptorSet (recommended - no protoc needed)
    /// 2. .proto file path (requires protoc in PATH)
    /// 3. .proto text content (requires protoc in PATH)
    fn compile_proto_schema(proto_schema: &str) -> Result<FileDescriptorSet> {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        // Try base64 decode first (FileDescriptorSet encoded as base64)
        if let Ok(decoded) = STANDARD.decode(proto_schema.trim()) {
            match FileDescriptorSet::decode(decoded.as_slice()) {
                Ok(fds) => {
                    debug!("Loaded FileDescriptorSet from base64 ({} bytes)", decoded.len());
                    return Ok(fds);
                }
                Err(e) => {
                    // Base64 decoded successfully but FileDescriptorSet decode failed
                    // This is likely the correct format but corrupted data
                    bail!("Successfully decoded base64 but failed to parse FileDescriptorSet: {}. \
                           The base64 string may be corrupted or not a valid FileDescriptorSet.", e);
                }
            }
        }

        // Check if it's a file path
        if proto_schema.ends_with(".proto") || proto_schema.ends_with(".pb") {
            return Self::load_proto_from_file(proto_schema);
        }

        // Assume it's .proto text and compile with protoc
        Self::compile_proto_text(proto_schema)
    }

    /// Load FileDescriptorSet from a .proto or .pb file
    fn load_proto_from_file(path: &str) -> Result<FileDescriptorSet> {
        use std::path::Path;
        let path = Path::new(path);

        if !path.exists() {
            bail!("Proto file not found: {}", path.display());
        }

        // If it's a .pb file (pre-compiled descriptor), load directly
        if path.extension().and_then(|e| e.to_str()) == Some("pb") {
            let bytes = std::fs::read(path)?;
            let fds = FileDescriptorSet::decode(bytes.as_slice())
                .context("Failed to decode .pb file as FileDescriptorSet")?;
            debug!("Loaded FileDescriptorSet from {} ({} files)", path.display(), fds.file.len());
            return Ok(fds);
        }

        // Otherwise, compile the .proto file with protoc
        Self::compile_proto_file(path)
    }

    /// Compile .proto file using protoc
    fn compile_proto_file(path: &std::path::Path) -> Result<FileDescriptorSet> {
        use std::process::Command;

        let output_path = std::env::temp_dir().join("netget_grpc_descriptor.pb");

        // Get the directory containing the proto file for proto_path
        let proto_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let filename = path.file_name().context("Invalid proto file path")?;

        // Run protoc to generate FileDescriptorSet
        let output = Command::new("protoc")
            .arg("--include_imports")
            .arg("--include_source_info")
            .arg(format!("--descriptor_set_out={}", output_path.display()))
            .arg(format!("--proto_path={}", proto_dir.display()))
            .arg(filename)
            .output()
            .context("Failed to execute protoc. Is protoc installed and in PATH?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("protoc failed: {}", stderr);
        }

        // Load the generated descriptor set
        let bytes = std::fs::read(&output_path)?;
        let fds = FileDescriptorSet::decode(bytes.as_slice())
            .context("Failed to decode protoc output")?;

        debug!("Compiled {} with protoc ({} files)", path.display(), fds.file.len());
        Ok(fds)
    }

    /// Compile .proto text using protoc
    fn compile_proto_text(proto_text: &str) -> Result<FileDescriptorSet> {
        use std::io::Write;

        // Write to temporary file
        let temp_dir = std::env::temp_dir();
        let proto_file = temp_dir.join("netget_grpc_temp.proto");

        {
            let mut file = std::fs::File::create(&proto_file)?;
            file.write_all(proto_text.as_bytes())?;
        }

        // Compile with protoc
        let result = Self::compile_proto_file(&proto_file);

        // Clean up temp file
        let _ = std::fs::remove_file(&proto_file);

        result
    }
}

/// Dynamic gRPC service that handles requests using LLM
#[cfg(feature = "grpc")]
struct DynamicGrpcService {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    descriptor_pool: Arc<DescriptorPool>,
    protocol: Arc<GrpcProtocol>,
}

#[cfg(feature = "grpc")]
impl DynamicGrpcService {
    /// Handle a gRPC HTTP/2 request
    async fn handle_grpc_request(
        &self,
        req: Request<Incoming>,
        connection_id: crate::server::connection::ConnectionId,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        // Extract service and method from path (format: /package.Service/Method)
        let path = req.uri().path();
        let (service_name, method_name) = match Self::parse_grpc_path(path) {
            Ok((svc, method)) => (svc, method),
            Err(e) => {
                debug!("Invalid gRPC path: {} - {}", path, e);
                return Ok(Self::grpc_error_response(StatusCode::NOT_FOUND, "Invalid path"));
            }
        };

        debug!("gRPC request: {}/{}", service_name, method_name);
        let _ = self.status_tx.send(format!("[DEBUG] gRPC request: {}/{}", service_name, method_name));

        // Validate content-type
        if let Some(content_type) = req.headers().get("content-type") {
            if !content_type.to_str().unwrap_or("").starts_with("application/grpc") {
                return Ok(Self::grpc_error_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Expected application/grpc"));
            }
        }

        // Read request body
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                debug!("Failed to read gRPC request body: {}", e);
                return Ok(Self::grpc_error_response(StatusCode::BAD_REQUEST, "Failed to read body"));
            }
        };

        // Decode gRPC frame (5-byte header: compression flag + 4-byte length + payload)
        let request_payload = match Self::decode_grpc_frame(&body_bytes) {
            Ok(payload) => payload,
            Err(e) => {
                debug!("Failed to decode gRPC frame: {}", e);
                return Ok(Self::grpc_error_response(StatusCode::BAD_REQUEST, "Invalid gRPC frame"));
            }
        };

        trace!("gRPC request payload: {} bytes", request_payload.len());

        // Handle the unary request
        let response_payload = match self.handle_unary(&service_name, &method_name, request_payload, connection_id).await {
            Ok(payload) => payload,
            Err(e) => {
                debug!("gRPC handler error: {}", e);
                let _ = self.status_tx.send(format!("[ERROR] gRPC handler error: {}", e));
                return Ok(Self::grpc_error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()));
            }
        };

        // Encode response with gRPC framing
        let response_frame = Self::encode_grpc_frame(&response_payload);

        // Build HTTP/2 response with gRPC trailers
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/grpc")
            .header("grpc-status", "0")  // OK
            .body(Full::new(Bytes::from(response_frame)))
            .unwrap();

        debug!("gRPC response: {} bytes", response_payload.len());
        let _ = self.status_tx.send(format!("[DEBUG] gRPC response: {} bytes", response_payload.len()));

        Ok(response)
    }

    /// Parse gRPC path into service and method names
    fn parse_grpc_path(path: &str) -> Result<(String, String)> {
        // Format: /package.Service/Method
        if !path.starts_with('/') {
            bail!("Path must start with /");
        }

        let parts: Vec<&str> = path[1..].split('/').collect();
        if parts.len() != 2 {
            bail!("Path must be /Service/Method");
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }

    /// Decode gRPC frame from bytes
    /// Frame format: 1 byte compression flag + 4 bytes length (big-endian) + payload
    fn decode_grpc_frame(frame: &[u8]) -> Result<Vec<u8>> {
        if frame.len() < 5 {
            bail!("Frame too short (need at least 5 bytes)");
        }

        let compressed = frame[0];
        if compressed != 0 {
            bail!("Compression not supported");
        }

        let length = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]) as usize;

        if frame.len() < 5 + length {
            bail!("Frame length mismatch (expected {} bytes, got {})", 5 + length, frame.len());
        }

        Ok(frame[5..5 + length].to_vec())
    }

    /// Encode payload into gRPC frame
    fn encode_grpc_frame(payload: &[u8]) -> Vec<u8> {
        let length = payload.len() as u32;
        let mut frame = Vec::with_capacity(5 + payload.len());

        // Compression flag (0 = not compressed)
        frame.push(0);

        // Length (4 bytes, big-endian)
        frame.extend_from_slice(&length.to_be_bytes());

        // Payload
        frame.extend_from_slice(payload);

        frame
    }

    /// Create gRPC error response
    fn grpc_error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
        Response::builder()
            .status(status)
            .header("content-type", "application/grpc")
            .header("grpc-status", "13")  // INTERNAL
            .header("grpc-message", message)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }

    /// Handle a gRPC unary request
    async fn handle_unary(
        &self,
        service_name: &str,
        method_name: &str,
        request_bytes: Vec<u8>,
        connection_id: crate::server::connection::ConnectionId,
    ) -> Result<Vec<u8>> {
        // Find service and method descriptors
        let service_desc = self.descriptor_pool
            .services()
            .find(|s| s.full_name() == service_name)
            .context("Service not found in schema")?;

        let method_desc = service_desc
            .methods()
            .find(|m| m.name() == method_name)
            .context("Method not found in service")?;

        let input_desc = method_desc.input();
        let output_desc = method_desc.output();

        debug!("gRPC unary call: {}/{}", service_name, method_name);
        let _ = self.status_tx.send(format!("[DEBUG] gRPC call: {}/{}", service_name, method_name));

        // Decode request using dynamic message
        let request_msg = DynamicMessage::decode(input_desc.clone(), request_bytes.as_slice())
            .context("Failed to decode gRPC request")?;

        // Convert DynamicMessage to JSON using prost-reflect's JSON serialization
        let request_json = Self::dynamic_message_to_json(&request_msg)?;

        trace!("gRPC request JSON: {}", serde_json::to_string_pretty(&request_json)?);
        let _ = self.status_tx.send(format!("[TRACE] Request: {}", serde_json::to_string(&request_json)?));

        // Build response schema description for LLM
        let response_schema = Self::build_message_schema(&output_desc);

        // Create event for LLM
        let event = Event::new(&GRPC_UNARY_REQUEST_EVENT, json!({
            "service": service_name,
            "method": method_name,
            "request": request_json,
            "expected_response_schema": response_schema,
        }));

        // Call LLM
        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(connection_id),
            &event,
            self.protocol.as_ref(),
        )
        .await
        .context("LLM call failed")?;

        // Process action results
        for protocol_result in execution_result.protocol_results {
            match protocol_result {
                ActionResult::Custom { name, data } if name == "grpc_unary_response" => {
                    // Extract response message from LLM
                    let response_json = data.get("message")
                        .context("Missing 'message' in grpc_unary_response")?;

                    // Convert JSON to DynamicMessage
                    let response_msg = Self::json_to_dynamic_message(response_json, &output_desc)?;

                    // Encode to protobuf bytes
                    let mut response_bytes = Vec::new();
                    response_msg.encode(&mut response_bytes)?;

                    debug!("gRPC response: {} bytes", response_bytes.len());
                    let _ = self.status_tx.send(format!("[DEBUG] Response: {} bytes", response_bytes.len()));

                    return Ok(response_bytes);
                }
                ActionResult::Custom { name, data } if name == "grpc_error" => {
                    let code = data.get("code").and_then(|c| c.as_str()).unwrap_or("INTERNAL");
                    let message = data.get("message").and_then(|m| m.as_str()).unwrap_or("Internal error");

                    debug!("gRPC error: {} - {}", code, message);
                    bail!("gRPC error: {} - {}", code, message);
                }
                _ => {
                    // Ignore other action results
                }
            }
        }

        // If no response was returned, return empty message
        debug!("No response from LLM, returning empty message");
        let response_msg = DynamicMessage::new(output_desc.clone());
        let mut response_bytes = Vec::new();
        response_msg.encode(&mut response_bytes)?;
        Ok(response_bytes)
    }

    /// Convert DynamicMessage to JSON
    fn dynamic_message_to_json(msg: &DynamicMessage) -> Result<serde_json::Value> {
        // For now, create a basic JSON representation by iterating fields
        // TODO: Use proper protobuf JSON serialization when available
        let desc = msg.descriptor();
        let mut map = serde_json::Map::new();

        for field in desc.fields() {
            // get_field returns Cow<Value>, check if field has a value first
            if msg.has_field(&field) {
                let value = msg.get_field(&field);
                // Convert protobuf Value to JSON
                let json_value = Self::proto_value_to_json(&value)?;
                map.insert(field.name().to_string(), json_value);
            }
        }

        Ok(serde_json::Value::Object(map))
    }

    /// Convert protobuf Value to JSON
    fn proto_value_to_json(value: &prost_reflect::Value) -> Result<serde_json::Value> {
        use prost_reflect::Value;

        Ok(match value {
            Value::Bool(b) => json!(*b),
            Value::I32(i) => json!(*i),
            Value::I64(i) => json!(*i),
            Value::U32(u) => json!(*u),
            Value::U64(u) => json!(*u),
            Value::F32(f) => json!(*f),
            Value::F64(f) => json!(*f),
            Value::String(s) => json!(s),
            Value::Bytes(b) => {
                use base64::engine::general_purpose::STANDARD;
                use base64::Engine;
                json!(STANDARD.encode(b))
            },
            Value::EnumNumber(e) => json!(*e),
            Value::Message(m) => Self::dynamic_message_to_json(m)?,
            Value::List(l) => {
                let items: Result<Vec<_>> = l.iter()
                    .map(|v| Self::proto_value_to_json(v))
                    .collect();
                json!(items?)
            }
            Value::Map(m) => {
                let mut map = serde_json::Map::new();
                for (k, v) in m.iter() {
                    let key = Self::map_key_to_string(k)?;
                    let value = Self::proto_value_to_json(v)?;
                    map.insert(key, value);
                }
                json!(map)
            }
        })
    }

    /// Convert map key to string
    fn map_key_to_string(key: &prost_reflect::MapKey) -> Result<String> {
        use prost_reflect::MapKey;

        Ok(match key {
            MapKey::Bool(b) => b.to_string(),
            MapKey::I32(i) => i.to_string(),
            MapKey::I64(i) => i.to_string(),
            MapKey::U32(u) => u.to_string(),
            MapKey::U64(u) => u.to_string(),
            MapKey::String(s) => s.clone(),
        })
    }

    /// Convert JSON to DynamicMessage
    fn json_to_dynamic_message(
        json: &serde_json::Value,
        message_desc: &prost_reflect::MessageDescriptor,
    ) -> Result<DynamicMessage> {
        // Create a new dynamic message
        let mut msg = DynamicMessage::new(message_desc.clone());

        // Populate fields from JSON
        if let Some(obj) = json.as_object() {
            for (field_name, value) in obj {
                if let Some(field) = message_desc.get_field_by_name(field_name) {
                    let proto_value = Self::json_to_proto_value(value, &field)?;
                    msg.set_field(&field, proto_value);
                }
            }
        }

        Ok(msg)
    }

    /// Convert JSON value to protobuf Value
    fn json_to_proto_value(
        json: &serde_json::Value,
        field: &prost_reflect::FieldDescriptor,
    ) -> Result<prost_reflect::Value> {
        use prost_reflect::{Kind, Value};

        Ok(match field.kind() {
            Kind::Bool => Value::Bool(json.as_bool().context("Expected boolean")?),
            Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => {
                Value::I32(json.as_i64().context("Expected integer")? as i32)
            }
            Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => {
                Value::I64(json.as_i64().context("Expected integer")?)
            }
            Kind::Uint32 | Kind::Fixed32 => {
                Value::U32(json.as_u64().context("Expected unsigned integer")? as u32)
            }
            Kind::Uint64 | Kind::Fixed64 => {
                Value::U64(json.as_u64().context("Expected unsigned integer")?)
            }
            Kind::Float => Value::F32(json.as_f64().context("Expected number")? as f32),
            Kind::Double => Value::F64(json.as_f64().context("Expected number")?),
            Kind::String => Value::String(json.as_str().context("Expected string")?.to_string()),
            Kind::Bytes => {
                use base64::engine::general_purpose::STANDARD;
                use base64::Engine;
                let s = json.as_str().context("Expected base64 string")?;
                let bytes = STANDARD.decode(s).context("Invalid base64")?;
                Value::Bytes(bytes.into())
            }
            Kind::Message(msg_desc) => {
                let msg = Self::json_to_dynamic_message(json, &msg_desc)?;
                Value::Message(msg)
            }
            Kind::Enum(enum_desc) => {
                if let Some(n) = json.as_i64() {
                    Value::EnumNumber(n as i32)
                } else if let Some(s) = json.as_str() {
                    // Try to find enum value by name
                    if let Some(val) = enum_desc.get_value_by_name(s) {
                        Value::EnumNumber(val.number())
                    } else {
                        bail!("Unknown enum value: {}", s);
                    }
                } else {
                    bail!("Expected enum number or string");
                }
            }
        })
    }

    /// Build a JSON schema description of a message type
    fn build_message_schema(message_desc: &prost_reflect::MessageDescriptor) -> serde_json::Value {
        let mut fields = serde_json::Map::new();

        for field in message_desc.fields() {
            let field_type = match field.kind() {
                prost_reflect::Kind::Double => "number (double)",
                prost_reflect::Kind::Float => "number (float)",
                prost_reflect::Kind::Int32 | prost_reflect::Kind::Sint32 | prost_reflect::Kind::Sfixed32 => "int32",
                prost_reflect::Kind::Int64 | prost_reflect::Kind::Sint64 | prost_reflect::Kind::Sfixed64 => "int64",
                prost_reflect::Kind::Uint32 | prost_reflect::Kind::Fixed32 => "uint32",
                prost_reflect::Kind::Uint64 | prost_reflect::Kind::Fixed64 => "uint64",
                prost_reflect::Kind::Bool => "boolean",
                prost_reflect::Kind::String => "string",
                prost_reflect::Kind::Bytes => "bytes (base64)",
                prost_reflect::Kind::Message(_) => "object",
                prost_reflect::Kind::Enum(_) => "enum (string)",
            };

            let cardinality = match field.cardinality() {
                prost_reflect::Cardinality::Optional => "optional",
                prost_reflect::Cardinality::Required => "required",
                prost_reflect::Cardinality::Repeated => "repeated",
            };

            fields.insert(
                field.name().to_string(),
                json!({
                    "type": field_type,
                    "cardinality": cardinality,
                }),
            );
        }

        json!({
            "type": "object",
            "fields": fields,
        })
    }
}
