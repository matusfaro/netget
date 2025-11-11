//! Shared HTTP request/response handling logic

use std::collections::HashMap;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use tokio::sync::mpsc;
use tracing::{debug, error, trace};
use crate::llm::ActionResult;
use std::convert::Infallible;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Extracted request data common to HTTP and HTTP/2
#[derive(Debug)]
pub struct RequestData {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body_bytes: Bytes,
}

/// Extract request data from hyper Request
pub async fn extract_request_data(
    req: Request<Incoming>,
    protocol_label: &str,
    status_tx: &mpsc::UnboundedSender<String>,
) -> RequestData {
    // Extract request details first for logging
    let method = req.method().to_string();
    let uri = req.uri().to_string();
    let version = format!("{:?}", req.version());

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Read body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            Bytes::new()
        }
    };

    // DEBUG: Log request summary to both file and TUI
    debug!(
        "{} request (action-based): {} {} {} ({} bytes)",
        protocol_label,
        method,
        uri,
        version,
        body_bytes.len(),
    );
    console_debug!(status_tx, "[DEBUG] {} request: {} {} {} ({} bytes)");

    // TRACE: Log full request details
    trace!("{} request headers:", protocol_label);
    for (name, value) in &headers {
        console_trace!(status_tx, "[TRACE] {} header: {}: {}", protocol_label, name, value);
    }
    if !body_bytes.is_empty() {
        if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
            // Try to pretty-print if it's JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body_str) {
                let pretty = serde_json::to_string_pretty(&json).unwrap_or(body_str.to_string());
                console_trace!(status_tx, "[TRACE] {} request body (JSON):\r\n{}", protocol_label, pretty.replace('\n', "\r\n"));
            } else {
                console_trace!(status_tx, "[TRACE] {} request body:\r\n{}", protocol_label, body_str.replace('\n', "\r\n"));
            }
        } else {
            console_trace!(status_tx, "[TRACE] {} request body (binary): {} bytes", protocol_label, body_bytes.len());
        }
    }

    RequestData {
        method,
        uri,
        version,
        headers,
        body_bytes,
    }
}

/// Build HTTP response from LLM execution results
pub fn build_response(
    protocol_results: Vec<ActionResult>,
    protocol_label: &str,
    method: &str,
    uri: &str,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Default response in case nothing was produced
    let mut status_code = 200;
    let mut response_headers = HashMap::new();
    let mut response_body = String::new();

    for protocol_result in protocol_results {
        if let ActionResult::Output(output_data) = protocol_result {
            // Parse the output as JSON containing HTTP response fields
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&output_data) {
                if let Some(status) = json_value.get("status").and_then(|v| v.as_u64()) {
                    status_code = status as u16;
                }
                if let Some(headers_obj) = json_value.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in headers_obj {
                        if let Some(v_str) = v.as_str() {
                            response_headers.insert(k.clone(), v_str.to_string());
                        }
                    }
                }
                if let Some(body) = json_value.get("body").and_then(|v| v.as_str()) {
                    response_body = body.to_string();
                }
            }
        }
    }

    console_info!(status_tx, "→ {} {} {} → {} ({} bytes)");

    // Build the HTTP response
    let mut response = Response::builder().status(status_code);

    // Add headers
    for (name, value) in response_headers {
        response = response.header(name, value);
    }

    Ok(response.body(Full::new(Bytes::from(response_body))).unwrap())
}

/// Build error response for LLM failures
pub fn build_error_response(
    error: anyhow::Error,
    protocol_label: &str,
    method: &str,
    uri: &str,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    console_error!(status_tx, "✗ LLM error for {} {}: {}", method, uri, error);

    Ok(Response::builder()
        .status(500)
        .body(Full::new(Bytes::from("Internal Server Error")))
        .unwrap())
}
