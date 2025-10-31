//! MCP session state management
//!
//! Tracks per-session state for MCP connections including initialization status,
//! capabilities, subscriptions, and registered resources/tools/prompts.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::server::connection::ConnectionId;

/// MCP session state
#[derive(Debug, Clone)]
pub struct McpSession {
    /// Unique session identifier
    pub session_id: String,
    /// Associated connection ID
    pub connection_id: ConnectionId,
    /// Whether the session has completed initialization
    pub initialized: bool,
    /// Server capabilities advertised to client
    pub capabilities: Value,
    /// Active resource subscriptions (URIs)
    pub subscriptions: HashSet<String>,
    /// Registered tools
    pub tools: HashMap<String, ToolDefinition>,
    /// Registered resources
    pub resources: HashMap<String, ResourceDefinition>,
    /// Registered prompts
    pub prompts: HashMap<String, PromptDefinition>,
}

/// Tool definition in MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: Value,
}

/// Resource definition in MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDefinition {
    /// Resource URI (e.g., "file:///path/to/file")
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// MIME type
    pub mime_type: Option<String>,
}

/// Prompt definition in MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDefinition {
    /// Prompt name
    pub name: String,
    /// Prompt description
    pub description: Option<String>,
    /// Prompt arguments (parameter definitions)
    pub arguments: Option<Vec<PromptArgument>>,
}

/// Prompt argument definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Argument name
    pub name: String,
    /// Argument description
    pub description: Option<String>,
    /// Whether argument is required
    pub required: Option<bool>,
}

impl McpSession {
    /// Create a new MCP session
    pub fn new(session_id: String, connection_id: ConnectionId) -> Self {
        Self {
            session_id,
            connection_id,
            initialized: false,
            capabilities: serde_json::json!({}),
            subscriptions: HashSet::new(),
            tools: HashMap::new(),
            resources: HashMap::new(),
            prompts: HashMap::new(),
        }
    }

    /// Mark session as initialized
    pub fn mark_initialized(&mut self) {
        self.initialized = true;
    }

    /// Set server capabilities
    pub fn set_capabilities(&mut self, capabilities: Value) {
        self.capabilities = capabilities;
    }

    /// Subscribe to a resource
    pub fn subscribe(&mut self, uri: String) {
        self.subscriptions.insert(uri);
    }

    /// Unsubscribe from a resource
    pub fn unsubscribe(&mut self, uri: &str) {
        self.subscriptions.remove(uri);
    }

    /// Register a tool
    pub fn register_tool(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Register a resource
    pub fn register_resource(&mut self, resource: ResourceDefinition) {
        self.resources.insert(resource.uri.clone(), resource);
    }

    /// Register a prompt
    pub fn register_prompt(&mut self, prompt: PromptDefinition) {
        self.prompts.insert(prompt.name.clone(), prompt);
    }

    /// Get all tools
    pub fn get_tools(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    /// Get all resources
    pub fn get_resources(&self) -> Vec<&ResourceDefinition> {
        self.resources.values().collect()
    }

    /// Get all prompts
    pub fn get_prompts(&self) -> Vec<&PromptDefinition> {
        self.prompts.values().collect()
    }

    /// Check if resource is subscribed
    pub fn is_subscribed(&self, uri: &str) -> bool {
        self.subscriptions.contains(uri)
    }
}
