//! Shared HTTP action execution logic

use crate::llm::actions::protocol_trait::ActionResult;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;

/// Parsed HTTP response data
#[derive(Debug)]
pub struct HttpResponseData {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// Execute HTTP response action (shared by HTTP and HTTP/2)
pub fn execute_http_response_action(action: serde_json::Value) -> Result<ActionResult> {
    let status = action
        .get("status")
        .and_then(|v| v.as_u64())
        .context("Missing or invalid 'status' parameter")? as u16;

    let headers = action
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<HashMap<String, String>>()
        })
        .unwrap_or_default();

    let body = action
        .get("body")
        .and_then(|v| v.as_str())
        .context("Missing 'body' parameter")?;

    // Return structured data that caller will convert to HTTP response
    let response_data = json!({
        "status": status,
        "headers": headers,
        "body": body
    });

    Ok(ActionResult::Output(
        serde_json::to_vec(&response_data).context("Failed to serialize HTTP response")?,
    ))
}
