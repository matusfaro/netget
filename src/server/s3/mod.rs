//! S3-compatible object storage server implementation
//!
//! Implements an S3-compatible REST API on port 9000 (default).
//! The LLM controls all operations and maintains "virtual" data through conversation context.

pub mod actions;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::server::connection::ConnectionId;
use crate::server::S3Protocol;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::state::app_state::AppState;

/// S3 server that delegates API operations to LLM
pub struct S3Server;

impl S3Server {
    /// Spawn the S3 server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("S3 server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] S3 server listening on {}", local_addr));

        let protocol = Arc::new(S3Protocol::new());

        // Spawn server loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("S3 connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!("[INFO] S3 connection from {}", remote_addr));

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: local_addr_conn,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::S3 {
                                recent_operations: Vec::new(), // (operation, bucket, key, time)
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        // Spawn a task to handle this connection
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);

                            // Clone for service closure
                            let status_for_service = status_tx_clone.clone();
                            let app_state_for_service = app_state_clone.clone();

                            // Create a service that handles S3 requests with LLM
                            let service = service_fn(move |req: Request<Incoming>| {
                                let llm_clone = llm_client_clone.clone();
                                let state_clone = app_state_for_service.clone();
                                let status_clone = status_for_service.clone();
                                let protocol_clone = protocol_clone.clone();
                                handle_s3_request_with_llm(
                                    req,
                                    connection_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                    server_id,
                                )
                            });

                            // Serve HTTP/1 on this connection
                            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                                error!("Error serving S3 connection: {:?}", err);
                            }

                            // Mark connection as closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send(format!("[INFO] S3 connection {} closed", connection_id));
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept S3 connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept S3 connection: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single S3 request with LLM
async fn handle_s3_request_with_llm(
    req: Request<Incoming>,
    _connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<S3Protocol>,
    server_id: crate::state::ServerId,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request details
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();

    // Parse bucket and key from path
    // Path format: / (list buckets), /bucket (bucket ops), /bucket/key (object ops)
    let (bucket, key, operation) = parse_s3_path(&method, &path);

    debug!(
        "S3 request: {} {} bucket={:?} key={:?} operation={}",
        method, path, bucket, key, operation
    );
    let _ = status_tx.send(format!(
        "[DEBUG] S3 {} {} operation={}",
        method, path, operation
    ));

    // Read request body (for PUT operations)
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read S3 request body: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to read S3 request body: {}", e));
            Bytes::new()
        }
    };

    if !body_bytes.is_empty() {
        trace!("S3 request body ({} bytes)", body_bytes.len());
        let _ = status_tx.send(format!("[TRACE] S3 request body: {} bytes", body_bytes.len()));
    }

    // Create S3 request event
    let event = crate::protocol::Event::new(
        &actions::S3_REQUEST_EVENT,
        serde_json::json!({
            "operation": operation,
            "bucket": bucket,
            "key": key,
            "request_details": {
                "method": method.as_str(),
                "path": path,
                "body_size": body_bytes.len(),
            }
        }),
    );

    // Call LLM to handle request
    let llm_result = crate::llm::action_helper::call_llm(
        &llm_client,
        &app_state,
        server_id,
        None, // Connection ID not needed for stateless HTTP
        &event,
        protocol.as_ref(),
    ).await;

    // Process LLM result and build HTTP response
    match llm_result {
        Ok(execution_result) => {
            // Look for S3-specific response actions
            for result in execution_result.protocol_results {
                // Try to process this action result as S3 response
                let response = process_s3_action_result(result, &status_tx).await;
                // Return the first successful response
                return response;
            }

            // No S3 actions found, return empty 200 OK
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
        Err(e) => {
            error!("LLM error handling S3 request: {}", e);
            let _ = status_tx.send(format!("[ERROR] LLM error: {}", e));

            // Return 500 error
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/xml")
                .body(Full::new(Bytes::from(format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>InternalError</Code>
  <Message>{}</Message>
</Error>"#,
                    e
                ))))
                .unwrap())
        }
    }
}

/// Parse S3 path into bucket, key, and operation
fn parse_s3_path(method: &Method, path: &str) -> (Option<String>, Option<String>, String) {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();

    match (method, parts.as_slice()) {
        // List buckets: GET /
        (m, []) if m == Method::GET => (None, None, "ListBuckets".to_string()),

        // Bucket operations: GET /bucket, PUT /bucket, DELETE /bucket
        (m, [bucket]) if m == Method::GET => (Some(bucket.to_string()), None, "ListObjects".to_string()),
        (m, [bucket]) if m == Method::PUT => (Some(bucket.to_string()), None, "CreateBucket".to_string()),
        (m, [bucket]) if m == Method::DELETE => (Some(bucket.to_string()), None, "DeleteBucket".to_string()),
        (m, [bucket]) if m == Method::HEAD => (Some(bucket.to_string()), None, "HeadBucket".to_string()),

        // Object operations: GET /bucket/key, PUT /bucket/key, DELETE /bucket/key
        (m, parts) if parts.len() >= 2 && m == Method::GET => {
            let bucket = parts[0].to_string();
            let key = parts[1..].join("/");
            (Some(bucket), Some(key), "GetObject".to_string())
        },
        (m, parts) if parts.len() >= 2 && m == Method::PUT => {
            let bucket = parts[0].to_string();
            let key = parts[1..].join("/");
            (Some(bucket), Some(key), "PutObject".to_string())
        },
        (m, parts) if parts.len() >= 2 && m == Method::DELETE => {
            let bucket = parts[0].to_string();
            let key = parts[1..].join("/");
            (Some(bucket), Some(key), "DeleteObject".to_string())
        },
        (m, parts) if parts.len() >= 2 && m == Method::HEAD => {
            let bucket = parts[0].to_string();
            let key = parts[1..].join("/");
            (Some(bucket), Some(key), "HeadObject".to_string())
        },

        // Unknown
        _ => (None, None, "Unknown".to_string()),
    }
}

/// Process LLM action result and build HTTP response
async fn process_s3_action_result(
    action_result: ActionResult,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    match action_result {
        ActionResult::Custom { name, data } => {
            match name.as_str() {
                "s3_object" => {
                    // Send object content
                    let content = data.get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let content_type = data.get("content_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("application/octet-stream");

                    let etag = data.get("etag")
                        .and_then(|v| v.as_str())
                        .unwrap_or("\"default-etag\"");

                    debug!("Sending S3 object ({} bytes, {})", content.len(), content_type);
                    let _ = status_tx.send(format!("[DEBUG] → S3 object {} bytes", content.len()));

                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", content_type)
                        .header("ETag", etag)
                        .body(Full::new(Bytes::from(content)))
                        .unwrap())
                }
                "s3_object_list" => {
                    // Send list of objects as XML
                    let objects = data.get("objects")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.clone())
                        .unwrap_or_default();

                    let is_truncated = data.get("is_truncated")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let xml = build_list_objects_xml(&objects, is_truncated);

                    debug!("Sending S3 object list ({} objects)", objects.len());
                    let _ = status_tx.send(format!("[DEBUG] → S3 object list: {} objects", objects.len()));

                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/xml")
                        .body(Full::new(Bytes::from(xml)))
                        .unwrap())
                }
                "s3_bucket_list" => {
                    // Send list of buckets as XML
                    let buckets = data.get("buckets")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.clone())
                        .unwrap_or_default();

                    let xml = build_list_buckets_xml(&buckets);

                    debug!("Sending S3 bucket list ({} buckets)", buckets.len());
                    let _ = status_tx.send(format!("[DEBUG] → S3 bucket list: {} buckets", buckets.len()));

                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/xml")
                        .body(Full::new(Bytes::from(xml)))
                        .unwrap())
                }
                "s3_error" => {
                    // Send S3 error response
                    let error_code = data.get("error_code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("InternalError");

                    let message = data.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("An error occurred");

                    let status_code = data.get("status_code")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(500) as u16;

                    let xml = format!(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{}</Code>
  <Message>{}</Message>
</Error>"#,
                        error_code, message
                    );

                    debug!("Sending S3 error: {} ({})", error_code, status_code);
                    let _ = status_tx.send(format!("[DEBUG] → S3 error: {} {}", status_code, error_code));

                    Ok(Response::builder()
                        .status(StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
                        .header("Content-Type", "application/xml")
                        .body(Full::new(Bytes::from(xml)))
                        .unwrap())
                }
                _ => {
                    // Unknown custom action, return empty response
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(Full::new(Bytes::new()))
                        .unwrap())
                }
            }
        }
        _ => {
            // For non-custom actions (NoAction, etc.), return 200 OK with empty body
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
    }
}

/// Build ListBuckets XML response
fn build_list_buckets_xml(buckets: &[serde_json::Value]) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner>
    <DisplayName>netget</DisplayName>
    <ID>netget-user</ID>
  </Owner>
  <Buckets>"#
    );

    for bucket in buckets {
        let name = bucket.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let creation_date = bucket.get("creation_date").and_then(|v| v.as_str()).unwrap_or("2024-01-01T00:00:00.000Z");

        xml.push_str(&format!(
            r#"
    <Bucket>
      <Name>{}</Name>
      <CreationDate>{}</CreationDate>
    </Bucket>"#,
            name, creation_date
        ));
    }

    xml.push_str(
        r#"
  </Buckets>
</ListAllMyBucketsResult>"#
    );

    xml
}

/// Build ListObjects XML response
fn build_list_objects_xml(objects: &[serde_json::Value], is_truncated: bool) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">"#
    );

    xml.push_str(&format!(r#"
  <IsTruncated>{}</IsTruncated>"#, is_truncated));

    for object in objects {
        let key = object.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let size = object.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let last_modified = object.get("last_modified").and_then(|v| v.as_str()).unwrap_or("2024-01-01T00:00:00.000Z");
        let etag = object.get("etag").and_then(|v| v.as_str()).unwrap_or("\"default\"");

        xml.push_str(&format!(
            r#"
  <Contents>
    <Key>{}</Key>
    <Size>{}</Size>
    <LastModified>{}</LastModified>
    <ETag>{}</ETag>
  </Contents>"#,
            key, size, last_modified, etag
        ));
    }

    xml.push_str(
        r#"
</ListBucketResult>"#
    );

    xml
}
