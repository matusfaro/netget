//! JSON-RPC 2.0 message types for MCP
//!
//! Implements the JSON-RPC 2.0 specification used by the Model Context Protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 version string
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request method name
    pub method: String,
    /// Optional request parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Request identifier (for matching responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
}

/// JSON-RPC 2.0 response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Success result (mutually exclusive with error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error result (mutually exclusive with result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// Request identifier (matches request id)
    pub id: Option<RequestId>,
}

/// JSON-RPC 2.0 notification message (no response expected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Notification method name
    pub method: String,
    /// Optional notification parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Optional additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Request/Response ID (can be string, number, or null)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Number(n) => write!(f, "{}", n),
        }
    }
}

/// Standard JSON-RPC 2.0 error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Parse error - Invalid JSON
    ParseError = -32700,
    /// Invalid Request - Missing required fields
    InvalidRequest = -32600,
    /// Method not found - Unknown method
    MethodNotFound = -32601,
    /// Invalid params - Parameter validation failed
    InvalidParams = -32602,
    /// Internal error - Server-side failure
    InternalError = -32603,
}

impl ErrorCode {
    /// Get the standard error message for this code
    pub fn message(&self) -> &'static str {
        match self {
            Self::ParseError => "Parse error",
            Self::InvalidRequest => "Invalid Request",
            Self::MethodNotFound => "Method not found",
            Self::InvalidParams => "Invalid params",
            Self::InternalError => "Internal error",
        }
    }

    /// Convert to i32
    pub fn as_i32(&self) -> i32 {
        *self as i32
    }
}

impl JsonRpcError {
    /// Create a new error from an error code
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code: code.as_i32(),
            message: code.message().to_string(),
            data: None,
        }
    }

    /// Create an error with additional data
    pub fn with_data(code: ErrorCode, data: Value) -> Self {
        Self {
            code: code.as_i32(),
            message: code.message().to_string(),
            data: Some(data),
        }
    }

    /// Create a custom error with specific message
    pub fn custom(code: ErrorCode, message: String) -> Self {
        Self {
            code: code.as_i32(),
            message,
            data: None,
        }
    }
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(id: Option<RequestId>, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(id: Option<RequestId>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

impl JsonRpcRequest {
    /// Create a new request
    pub fn new(method: String, params: Option<Value>, id: Option<RequestId>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method,
            params,
            id,
        }
    }
}

impl JsonRpcNotification {
    /// Create a new notification
    pub fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method,
            params,
        }
    }
}

/// Parse an incoming JSON message into either a request or notification
#[derive(Debug, Clone)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
}

impl JsonRpcMessage {
    /// Parse from JSON value
    pub fn from_value(value: Value) -> Result<Self, JsonRpcError> {
        // Try parsing as request first (has id field)
        if value.get("id").is_some() {
            match serde_json::from_value::<JsonRpcRequest>(value) {
                Ok(req) => Ok(Self::Request(req)),
                Err(e) => Err(JsonRpcError::custom(
                    ErrorCode::InvalidRequest,
                    format!("Failed to parse request: {}", e),
                )),
            }
        } else {
            // Try parsing as notification (no id field)
            match serde_json::from_value::<JsonRpcNotification>(value) {
                Ok(notif) => Ok(Self::Notification(notif)),
                Err(e) => Err(JsonRpcError::custom(
                    ErrorCode::InvalidRequest,
                    format!("Failed to parse notification: {}", e),
                )),
            }
        }
    }

    /// Get the method name
    pub fn method(&self) -> &str {
        match self {
            Self::Request(req) => &req.method,
            Self::Notification(notif) => &notif.method,
        }
    }

    /// Get the parameters
    pub fn params(&self) -> Option<&Value> {
        match self {
            Self::Request(req) => req.params.as_ref(),
            Self::Notification(notif) => notif.params.as_ref(),
        }
    }

    /// Get the request ID (None for notifications)
    pub fn id(&self) -> Option<&RequestId> {
        match self {
            Self::Request(req) => req.id.as_ref(),
            Self::Notification(_) => None,
        }
    }
}
