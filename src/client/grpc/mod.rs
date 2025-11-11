//! gRPC client implementation
pub mod actions;

pub use actions::GrpcClientProtocol;

use anyhow::{Context, Result};
use bytes::Bytes;
use http::Request;
use http_body_util::BodyExt;
use prost::Message as ProstMessage;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, ReflectMessage, Value as ProtoValue, MapKey};
use prost_types::FileDescriptorSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tonic::transport::{Channel, Endpoint};
use tower::{Service, ServiceExt};
use tracing::{error, info, debug};

use crate::client::grpc::actions::{
    GRPC_CLIENT_CONNECTED_EVENT, GRPC_CLIENT_ERROR_EVENT, GRPC_CLIENT_RESPONSE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client as ClientTrait, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::{Event, StartupParams};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// gRPC client connection state
#[derive(Debug, Clone)]
enum ConnectionState {
    Idle,
    Processing,
}

/// Shared client data
struct GrpcClientData {
    channel: Channel,
    descriptor_pool: Arc<DescriptorPool>,
    state: ConnectionState,
}

/// gRPC client that connects to remote gRPC servers
pub struct GrpcClient;

impl GrpcClient {
    /// Connect to a gRPC server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        info!("gRPC client {} connecting to {}", client_id, remote_addr);

        // Parse startup parameters
        let proto_schema = startup_params
            .as_ref()
            .map(|p| p.get_string("proto_schema"))
            .context("Missing required startup parameter: proto_schema")?;

        let use_tls = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_bool("use_tls"))
            .unwrap_or(false);

        // Load protobuf schema
        let descriptor_pool = load_schema(&proto_schema).await
            .context("Failed to load protobuf schema")?;

        // List available services
        let services: Vec<String> = descriptor_pool
            .services()
            .map(|s| s.full_name().to_string())
            .collect();

        info!("gRPC client {} loaded schema with services: {:?}", client_id, services);

        // Build gRPC channel
        let uri = if use_tls {
            format!("https://{}", remote_addr)
        } else {
            format!("http://{}", remote_addr)
        };

        let channel = Endpoint::from_shared(uri.clone())
            .context("Invalid gRPC endpoint")?
            .connect()
            .await
            .context("Failed to connect to gRPC server")?;

        info!("gRPC client {} connected to {}", client_id, remote_addr);

        let grpc_client_data = Arc::new(Mutex::new(GrpcClientData {
            channel,
            descriptor_pool: Arc::new(descriptor_pool),
            state: ConnectionState::Idle,
        }));

        // Store client in protocol_data
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "grpc_client".to_string(),
                    serde_json::json!("initialized"),
                );
                client.set_protocol_field(
                    "server_addr".to_string(),
                    serde_json::json!(remote_addr),
                );
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] gRPC client {} ready for {} (services: {})",
            client_id,
            remote_addr,
            services.join(", ")
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        let protocol = Arc::new(GrpcClientProtocol::new());
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &GRPC_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "server_addr": remote_addr,
                    "services": services,
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute actions
                    for action in actions {
                        let grpc_data = grpc_client_data.clone();
                        let proto = protocol.clone();
                        if let Err(e) = Box::pin(execute_grpc_action(
                            client_id,
                            action,
                            grpc_data,
                            &app_state,
                            &llm_client,
                            &status_tx,
                            &proto,
                        ))
                        .await
                        {
                            error!("Failed to execute gRPC action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for gRPC client {}: {}", client_id, e);
                }
            }
        }

        // Spawn a background task that monitors for client disconnection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("gRPC client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (gRPC manages connections internally)
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

/// Load protobuf schema from various formats
async fn load_schema(schema_input: &str) -> Result<DescriptorPool> {
    use base64::{Engine as _, engine::general_purpose};

    // Try to decode as base64 FileDescriptorSet
    if let Ok(bytes) = general_purpose::STANDARD.decode(schema_input) {
        if let Ok(fds) = FileDescriptorSet::decode(&bytes[..]) {
            return DescriptorPool::from_file_descriptor_set(fds)
                .context("Failed to create descriptor pool from FileDescriptorSet");
        }
    }

    // Try as file path
    if std::path::Path::new(schema_input).exists() {
        let proto_content = tokio::fs::read_to_string(schema_input)
            .await
            .context("Failed to read .proto file")?;
        return compile_proto_text(&proto_content).await;
    }

    // Try as inline proto text
    if schema_input.contains("syntax") && schema_input.contains("proto") {
        return compile_proto_text(schema_input).await;
    }

    Err(anyhow::anyhow!(
        "Invalid proto_schema format. Expected base64 FileDescriptorSet, .proto file path, or inline .proto text"
    ))
}

/// Compile .proto text to descriptor pool using protoc
async fn compile_proto_text(proto_text: &str) -> Result<DescriptorPool> {
    // Write proto to temp file
    let temp_dir = tempfile::tempdir()?;
    let proto_path = temp_dir.path().join("schema.proto");
    tokio::fs::write(&proto_path, proto_text).await?;

    // Run protoc to compile
    let output = tokio::process::Command::new("protoc")
        .arg("--descriptor_set_out=/dev/stdout")
        .arg("--include_imports")
        .arg(proto_path.to_str().unwrap())
        .current_dir(temp_dir.path())
        .output()
        .await
        .context("Failed to run protoc (is it installed?)")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "protoc failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let fds = FileDescriptorSet::decode(&output.stdout[..])
        .context("Failed to decode protoc output")?;

    DescriptorPool::from_file_descriptor_set(fds)
        .context("Failed to create descriptor pool")
}

/// Execute a gRPC client action
async fn execute_grpc_action(
    client_id: ClientId,
    action: serde_json::Value,
    grpc_client_data: Arc<Mutex<GrpcClientData>>,
    app_state: &AppState,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<GrpcClientProtocol>,
) -> Result<()> {
    // Parse action using the protocol's execute_action method
    let action_result = protocol.as_ref().execute_action(action.clone())?;

    match action_result {
        ClientActionResult::Custom { name, data } if name == "grpc_call" => {
            let service = data["service"]
                .as_str()
                .context("Missing service in grpc_call")?;
            let method = data["method"]
                .as_str()
                .context("Missing method in grpc_call")?;
            let request = &data["request"];
            let metadata = data.get("metadata").and_then(|v| v.as_object());

            make_grpc_call(
                client_id,
                service,
                method,
                request.clone(),
                metadata.cloned(),
                grpc_client_data,
                app_state,
                llm_client,
                status_tx,
                protocol,
            )
            .await?;
        }
        ClientActionResult::Disconnect => {
            info!("gRPC client {} disconnecting", client_id);
            app_state
                .update_client_status(client_id, ClientStatus::Disconnected)
                .await;
            let _ = status_tx.send(format!("[CLIENT] gRPC client {} disconnected", client_id));
        }
        ClientActionResult::WaitForMore => {
            debug!("gRPC client {} waiting", client_id);
        }
        _ => {
            return Err(anyhow::anyhow!("Unexpected action result for gRPC client"));
        }
    }

    Ok(())
}

/// Make a gRPC call
async fn make_grpc_call(
    client_id: ClientId,
    service: &str,
    method: &str,
    request_json: serde_json::Value,
    metadata: Option<serde_json::Map<String, serde_json::Value>>,
    grpc_client_data: Arc<Mutex<GrpcClientData>>,
    app_state: &AppState,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
    protocol: &Arc<GrpcClientProtocol>,
) -> Result<()> {
    // Check if client is in idle state
    {
        let data = grpc_client_data.lock().await;
        if matches!(data.state, ConnectionState::Processing) {
            info!("gRPC client {} is busy, skipping request", client_id);
            return Ok(());
        }
    }

    // Set state to Processing
    {
        let mut data = grpc_client_data.lock().await;
        data.state = ConnectionState::Processing;
    }

    info!("gRPC client {} calling {}/{}", client_id, service, method);

    // Get descriptor pool and find method
    let (input_desc, output_desc) = {
        let data = grpc_client_data.lock().await;
        let method_desc = data
            .descriptor_pool
            .get_service_by_name(service)
            .and_then(|s| s.methods().find(|m| m.name() == method))
            .context(format!("Method {}/{} not found in schema", service, method))?;

        let input_desc = method_desc.input();
        let output_desc = method_desc.output();
        (input_desc, output_desc)
    };

    // Convert JSON request to protobuf
    let request_msg = json_to_dynamic_message(&request_json, &input_desc)
        .context("Failed to convert request JSON to protobuf")?;

    // Encode request
    let request_bytes = request_msg.encode_to_vec();

    info!("gRPC client {} sending {}-byte request to {}/{}", client_id, request_bytes.len(), service, method);

    // Build gRPC request path
    let path = format!("/{}/{}", service, method);

    // Get channel
    let channel = {
        let data = grpc_client_data.lock().await;
        data.channel.clone()
    };

    // Create HTTP request with gRPC framing
    use http::HeaderValue;

    let mut request_builder = Request::builder()
        .method("POST")
        .uri(path.clone())
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("grpc-accept-encoding", "identity");

    // Add custom metadata
    if let Some(meta) = metadata {
        for (key, value) in meta {
            if let Some(val_str) = value.as_str() {
                if let Ok(header_value) = HeaderValue::from_str(val_str) {
                    request_builder = request_builder.header(key.as_str(), header_value);
                }
            }
        }
    }

    // Encode gRPC message with 5-byte header (compression flag + length)
    let mut grpc_message = Vec::with_capacity(5 + request_bytes.len());
    grpc_message.push(0); // No compression
    grpc_message.extend_from_slice(&(request_bytes.len() as u32).to_be_bytes());
    grpc_message.extend_from_slice(&request_bytes);

    // Create body using UnsyncBoxBody which is compatible with tonic
    use http_body_util::combinators::UnsyncBoxBody;
    let full_body = http_body_util::Full::new(Bytes::from(grpc_message));
    let body = UnsyncBoxBody::new(full_body.map_err(|_: std::convert::Infallible| tonic::Status::internal("infallible error")));
    let http_request = request_builder.body(body)
        .context("Failed to build HTTP request")?;

    // Make the call using the channel
    let result = call_grpc_unary(&channel, http_request).await;

    // Reset to idle
    {
        let mut data = grpc_client_data.lock().await;
        data.state = ConnectionState::Idle;
    }

    match result {
        Ok(response_bytes) => {
            // Decode response
            let response_msg = DynamicMessage::decode(output_desc.clone(), &response_bytes[..])
                .context("Failed to decode gRPC response")?;

            // Convert to JSON
            let response_json = dynamic_message_to_json(&response_msg)?;

            info!("gRPC client {} received response for {}/{}", client_id, service, method);

            // Call LLM with response
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let event = Event::new(
                    &GRPC_CLIENT_RESPONSE_RECEIVED_EVENT,
                    serde_json::json!({
                        "service": service,
                        "method": method,
                        "response": response_json,
                    }),
                );

                let memory = app_state
                    .get_memory_for_client(client_id)
                    .await
                    .unwrap_or_default();

                match call_llm_for_client(
                    llm_client,
                    app_state,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    status_tx,
                )
                .await
                {
                    Ok(ClientLlmResult {
                        actions,
                        memory_updates,
                    }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            let grpc_data = grpc_client_data.clone();
                            let proto = protocol.clone();
                            if let Err(e) = Box::pin(execute_grpc_action(
                                client_id,
                                action,
                                grpc_data,
                                app_state,
                                llm_client,
                                status_tx,
                                &proto,
                            ))
                            .await
                            {
                                error!("Failed to execute gRPC action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for gRPC client {}: {}", client_id, e);
                    }
                }
            }

            Ok(())
        }
        Err(e) => {
            error!("gRPC client {} call failed: {}", client_id, e);

            // Call LLM with error
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let event = Event::new(
                    &GRPC_CLIENT_ERROR_EVENT,
                    serde_json::json!({
                        "service": service,
                        "method": method,
                        "code": "UNKNOWN",
                        "message": e.to_string(),
                    }),
                );

                let memory = app_state
                    .get_memory_for_client(client_id)
                    .await
                    .unwrap_or_default();

                let _ = call_llm_for_client(
                    llm_client,
                    app_state,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    status_tx,
                )
                .await;
            }

            Err(e)
        }
    }
}

/// Make a unary gRPC call using tonic channel
async fn call_grpc_unary(
    channel: &Channel,
    request: Request<http_body_util::combinators::UnsyncBoxBody<Bytes, tonic::Status>>,
) -> Result<Vec<u8>> {
    // Clone the channel to get a service we can call
    let mut client = channel.clone();

    // Call the service
    let response = client
        .ready()
        .await
        .context("gRPC channel not ready")?
        .call(request)
        .await
        .context("gRPC call failed")?;

    // Check gRPC status in headers
    let status_code = response
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(-1);

    if status_code != 0 && status_code != -1 {
        let status_message = response
            .headers()
            .get("grpc-message")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("Unknown error");

        return Err(anyhow::anyhow!(
            "gRPC error: status={}, message={}",
            status_code,
            status_message
        ));
    }

    // Read response body
    let body = response.into_body();
    let body_bytes = body.collect().await
        .context("Failed to read response body")?
        .to_bytes();

    // Decode gRPC framing (skip 5-byte header)
    if body_bytes.len() < 5 {
        return Err(anyhow::anyhow!("Response too short"));
    }

    let message_bytes = body_bytes.slice(5..);
    Ok(message_bytes.to_vec())
}

/// Convert JSON to dynamic protobuf message
fn json_to_dynamic_message(
    json: &serde_json::Value,
    descriptor: &MessageDescriptor,
) -> Result<DynamicMessage> {
    let mut msg = DynamicMessage::new(descriptor.clone());

    if let Some(obj) = json.as_object() {
        for (field_name, value) in obj {
            if let Some(field) = descriptor.get_field_by_name(field_name) {
                let proto_value = json_to_proto_value(value, &field)?;
                msg.set_field(&field, proto_value);
            }
        }
    }

    Ok(msg)
}

/// Convert JSON value to protobuf value
fn json_to_proto_value(
    json: &serde_json::Value,
    field: &prost_reflect::FieldDescriptor,
) -> Result<ProtoValue> {
    use prost_reflect::Kind;

    match field.kind() {
        Kind::Double => Ok(ProtoValue::F64(json.as_f64().unwrap_or(0.0))),
        Kind::Float => Ok(ProtoValue::F32(json.as_f64().unwrap_or(0.0) as f32)),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => {
            Ok(ProtoValue::I32(json.as_i64().unwrap_or(0) as i32))
        }
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => {
            Ok(ProtoValue::I64(json.as_i64().unwrap_or(0)))
        }
        Kind::Uint32 | Kind::Fixed32 => Ok(ProtoValue::U32(json.as_u64().unwrap_or(0) as u32)),
        Kind::Uint64 | Kind::Fixed64 => Ok(ProtoValue::U64(json.as_u64().unwrap_or(0))),
        Kind::Bool => Ok(ProtoValue::Bool(json.as_bool().unwrap_or(false))),
        Kind::String => Ok(ProtoValue::String(
            json.as_str().unwrap_or("").to_string(),
        )),
        Kind::Bytes => {
            use base64::{Engine as _, engine::general_purpose};
            let s = json.as_str().unwrap_or("");
            let bytes = general_purpose::STANDARD.decode(s).unwrap_or_default();
            Ok(ProtoValue::Bytes(bytes.into()))
        }
        Kind::Message(msg_desc) => {
            let msg = json_to_dynamic_message(json, &msg_desc)?;
            Ok(ProtoValue::Message(msg))
        }
        Kind::Enum(enum_desc) => {
            if let Some(number) = json.as_i64() {
                Ok(ProtoValue::EnumNumber(number as i32))
            } else if let Some(name) = json.as_str() {
                if let Some(value) = enum_desc.get_value_by_name(name) {
                    Ok(ProtoValue::EnumNumber(value.number()))
                } else {
                    Ok(ProtoValue::EnumNumber(0))
                }
            } else {
                Ok(ProtoValue::EnumNumber(0))
            }
        }
    }
}

/// Convert dynamic protobuf message to JSON
fn dynamic_message_to_json(msg: &DynamicMessage) -> Result<serde_json::Value> {
    let mut map = serde_json::Map::new();

    for field in msg.descriptor().fields() {
        if msg.has_field(&field) {
            let value = msg.get_field(&field);
            let json_value = proto_value_to_json(&value)?;
            map.insert(field.name().to_string(), json_value);
        }
    }

    Ok(serde_json::Value::Object(map))
}

/// Convert protobuf value to JSON
fn proto_value_to_json(value: &ProtoValue) -> Result<serde_json::Value> {
    use base64::{Engine as _, engine::general_purpose};

    Ok(match value {
        ProtoValue::Bool(b) => serde_json::Value::Bool(*b),
        ProtoValue::I32(i) => serde_json::Value::Number((*i).into()),
        ProtoValue::I64(i) => serde_json::Value::Number((*i).into()),
        ProtoValue::U32(u) => serde_json::Value::Number((*u).into()),
        ProtoValue::U64(u) => serde_json::Value::Number((*u).into()),
        ProtoValue::F32(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(*f as f64).unwrap_or(serde_json::Number::from(0)),
        ),
        ProtoValue::F64(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(*f).unwrap_or(serde_json::Number::from(0)),
        ),
        ProtoValue::String(s) => serde_json::Value::String(s.clone()),
        ProtoValue::Bytes(b) => serde_json::Value::String(general_purpose::STANDARD.encode(b)),
        ProtoValue::EnumNumber(n) => serde_json::Value::Number((*n).into()),
        ProtoValue::Message(msg) => dynamic_message_to_json(msg)?,
        ProtoValue::List(list) => {
            let items: Result<Vec<_>> = list.iter().map(proto_value_to_json).collect();
            serde_json::Value::Array(items?)
        }
        ProtoValue::Map(map) => {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map.iter() {
                let key_str = map_key_to_string(k);
                json_map.insert(key_str, proto_value_to_json(v)?);
            }
            serde_json::Value::Object(json_map)
        }
    })
}

/// Convert MapKey to string
fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::Bool(b) => b.to_string(),
        MapKey::I32(i) => i.to_string(),
        MapKey::I64(i) => i.to_string(),
        MapKey::U32(u) => u.to_string(),
        MapKey::U64(u) => u.to_string(),
        MapKey::String(s) => s.clone(),
    }
}
