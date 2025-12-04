//! MCP protocol actions implementation
//!
//! Defines LLM actions for controlling MCP server responses.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

/// MCP protocol action handler
pub struct McpProtocol;

impl McpProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for McpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new() // MCP is HTTP-based, no async actions needed
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            mcp_initialize_response_action(),
            mcp_resources_list_response_action(),
            mcp_resources_read_response_action(),
            mcp_resources_subscribe_response_action(),
            mcp_tools_list_response_action(),
            mcp_tools_call_response_action(),
            mcp_prompts_list_response_action(),
            mcp_prompts_get_response_action(),
            mcp_completion_response_action(),
            mcp_error_response_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "MCP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_mcp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>MCP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["mcp", "model-context-protocol", "model context protocol"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("axum HTTP, custom JSON-RPC 2.0, session management")
            .llm_control("All capabilities: resources, tools, prompts")
            .e2e_testing("MCP clients / TypeScript SDK")
            .notes("HTTP POST only, no WebSocket/SSE, in-memory sessions")
            .build()
    }
    fn description(&self) -> &'static str {
        "Model Context Protocol server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an MCP (Model Context Protocol) server on port 8000"
    }
    fn group_name(&self) -> &'static str {
        "AI & API"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: instruction-based
            json!({
                "type": "open_server",
                "port": 8000,
                "base_stack": "mcp",
                "instruction": "MCP server with resources (file:///README.md), tools (calculate, search), and prompts (code-review). Initialize with proper capabilities"
            }),
            // Script mode: event_handlers with script handler
            json!({
                "type": "open_server",
                "port": 8000,
                "base_stack": "mcp",
                "event_handlers": [{
                    "event_pattern": "mcp_tools_call",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "tool_name = event.get('name', '')\nargs = event.get('arguments', {})\nif tool_name == 'calculate':\n    result = str(eval(args.get('expression', '0')))\n    action('mcp_tools_call_response', response={'content': [{'type': 'text', 'text': result}]})\nelse:\n    action('mcp_error_response', code=-32601, message='Tool not found')"
                    }
                }]
            }),
            // Static mode: event_handlers with static actions
            json!({
                "type": "open_server",
                "port": 8000,
                "base_stack": "mcp",
                "event_handlers": [{
                    "event_pattern": "mcp_initialize",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "mcp_initialize_response",
                            "response": {
                                "protocolVersion": "2024-11-05",
                                "capabilities": {"resources": {}, "tools": {}, "prompts": {}},
                                "serverInfo": {"name": "netget-mcp", "version": "0.1.0"}
                            }
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for McpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::mcp::McpServer;
            McpServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "mcp_initialize_response" => self.execute_mcp_initialize_response(action),
            "mcp_resources_list_response" => self.execute_mcp_resources_list_response(action),
            "mcp_resources_read_response" => self.execute_mcp_resources_read_response(action),
            "mcp_resources_subscribe_response" => {
                self.execute_mcp_resources_subscribe_response(action)
            }
            "mcp_tools_list_response" => self.execute_mcp_tools_list_response(action),
            "mcp_tools_call_response" => self.execute_mcp_tools_call_response(action),
            "mcp_prompts_list_response" => self.execute_mcp_prompts_list_response(action),
            "mcp_prompts_get_response" => self.execute_mcp_prompts_get_response(action),
            "mcp_completion_response" => self.execute_mcp_completion_response(action),
            "mcp_error_response" => self.execute_mcp_error_response(action),
            _ => Err(anyhow::anyhow!("Unknown MCP action: {}", action_type)),
        }
    }
}

impl McpProtocol {
    fn execute_mcp_initialize_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_initialize".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_resources_list_response(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_resources_list".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_resources_read_response(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_resources_read".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_resources_subscribe_response(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_resources_subscribe".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_tools_list_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_tools_list".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_tools_call_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_tools_call".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_prompts_list_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_prompts_list".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_prompts_get_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_prompts_get".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_completion_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_completion".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action
            .get("code")
            .and_then(|v| v.as_i64())
            .context("Missing 'code' parameter")? as i32;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?
            .to_string();

        let data = action.get("data").cloned();

        Ok(ActionResult::Custom {
            name: "mcp_error".to_string(),
            data: json!({
                "code": code,
                "message": message,
                "data": data,
            }),
        })
    }
}

/// Initialize response action
fn mcp_initialize_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_initialize_response".to_string(),
        description: "Return initialization response with server capabilities".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description:
                "Initialize response object with protocolVersion, capabilities, and serverInfo"
                    .to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_initialize_response",
            "response": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "resources": {"subscribe": true},
                    "tools": {},
                    "prompts": {}
                },
                "serverInfo": {
                    "name": "netget-mcp",
                    "version": "0.1.0"
                }
            }
        }),
        log_template: None,
    }
}

/// Resources list response action
fn mcp_resources_list_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_resources_list_response".to_string(),
        description: "Return list of available resources".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Resources list response with array of resource definitions".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_resources_list_response",
            "response": {"resources": []}
        }),
        log_template: None,
    }
}

/// Resources read response action
fn mcp_resources_read_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_resources_read_response".to_string(),
        description: "Return resource content".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Resource content with contents array (text or blob)".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_resources_read_response",
            "response": {"contents": [{"uri": "file:///example", "text": "content"}]}
        }),
        log_template: None,
    }
}

/// Resources subscribe response action
fn mcp_resources_subscribe_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_resources_subscribe_response".to_string(),
        description: "Confirm resource subscription".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Empty object to confirm subscription".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_resources_subscribe_response",
            "response": {}
        }),
        log_template: None,
    }
}

/// Tools list response action
fn mcp_tools_list_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_tools_list_response".to_string(),
        description: "Return list of available tools".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description:
                "Tools list response with array of tool definitions including JSON schemas"
                    .to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_tools_list_response",
            "response": {"tools": []}
        }),
        log_template: None,
    }
}

/// Tools call response action
fn mcp_tools_call_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_tools_call_response".to_string(),
        description: "Return tool execution result".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Tool execution result with content array (text or image or resource)"
                .to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_tools_call_response",
            "response": {"content": [{"type": "text", "text": "result"}]}
        }),
        log_template: None,
    }
}

/// Prompts list response action
fn mcp_prompts_list_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_prompts_list_response".to_string(),
        description: "Return list of available prompts".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Prompts list response with array of prompt definitions".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_prompts_list_response",
            "response": {"prompts": []}
        }),
        log_template: None,
    }
}

/// Prompts get response action
fn mcp_prompts_get_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_prompts_get_response".to_string(),
        description: "Return formatted prompt template".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Prompt with messages array containing role/content pairs. Example: {\"description\": \"Code review prompt\", \"messages\": [{\"role\": \"user\", \"content\": {\"type\": \"text\", \"text\": \"Review this code: function foo() { return 42; }\"}}]}".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_prompts_get_response",
            "response": {"messages": []}
        }),
        log_template: None,
    }
}

/// Completion response action
fn mcp_completion_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_completion_response".to_string(),
        description: "Return text completion suggestions".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "object".to_string(),
            description: "Completion response with values array and pagination info".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_completion_response",
            "response": {"completion": {"values": [], "total": 0, "hasMore": false}}
        }),
        log_template: None,
    }
}

/// Error response action
fn mcp_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "mcp_error_response".to_string(),
        description: "Return JSON-RPC error".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "integer".to_string(),
                description: "JSON-RPC error code (-32700 to -32603 for standard errors)"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "any".to_string(),
                description: "Optional additional error data".to_string(),
                required: false,
            },
        ],
        example: serde_json::json!({
            "type": "mcp_error_response",
            "code": -32601,
            "message": "Method not found"
        }),
        log_template: None,
    }
}

/// MCP initialize event
pub static MCP_INITIALIZE_EVENT: std::sync::LazyLock<EventType> =
    std::sync::LazyLock::new(|| {
        EventType::new("mcp_initialize", "Client sends initialize request to negotiate capabilities", json!({"type": "placeholder", "event_id": "mcp_initialize"}))
            .with_actions(vec![
                mcp_initialize_response_action(),
                mcp_error_response_action(),
            ])
            .with_log_template(
                LogTemplate::new()
                    .with_info("MCP initialize")
                    .with_debug("MCP initialize")
                    .with_trace("MCP initialize: {json_pretty(.)}"),
            )
    });

/// MCP resources list event
pub static MCP_RESOURCES_LIST_EVENT: std::sync::LazyLock<EventType> =
    std::sync::LazyLock::new(|| {
        EventType::new("mcp_resources_list", "Client requests list of available resources", json!({"type": "placeholder", "event_id": "mcp_resources_list"}))
            .with_actions(vec![
                mcp_resources_list_response_action(),
                mcp_error_response_action(),
            ])
            .with_log_template(
                LogTemplate::new()
                    .with_info("MCP resources list")
                    .with_debug("MCP resources list")
                    .with_trace("MCP resources list: {json_pretty(.)}"),
            )
    });

/// MCP resources read event
pub static MCP_RESOURCES_READ_EVENT: std::sync::LazyLock<EventType> =
    std::sync::LazyLock::new(|| {
        EventType::new("mcp_resources_read", "Client requests resource content by URI", json!({"type": "placeholder", "event_id": "mcp_resources_read"}))
            .with_actions(vec![
                mcp_resources_read_response_action(),
                mcp_error_response_action(),
            ])
            .with_log_template(
                LogTemplate::new()
                    .with_info("MCP resources read")
                    .with_debug("MCP resources read")
                    .with_trace("MCP resources read: {json_pretty(.)}"),
            )
    });

/// MCP tools list event
pub static MCP_TOOLS_LIST_EVENT: std::sync::LazyLock<EventType> =
    std::sync::LazyLock::new(|| {
        EventType::new("mcp_tools_list", "Client requests list of available tools", json!({"type": "placeholder", "event_id": "mcp_tools_list"}))
            .with_actions(vec![
                mcp_tools_list_response_action(),
                mcp_error_response_action(),
            ])
            .with_log_template(
                LogTemplate::new()
                    .with_info("MCP tools list")
                    .with_debug("MCP tools list")
                    .with_trace("MCP tools list: {json_pretty(.)}"),
            )
    });

/// MCP tools call event
pub static MCP_TOOLS_CALL_EVENT: std::sync::LazyLock<EventType> =
    std::sync::LazyLock::new(|| {
        EventType::new("mcp_tools_call", "Client executes a tool with parameters", json!({"type": "placeholder", "event_id": "mcp_tools_call"}))
            .with_actions(vec![
                mcp_tools_call_response_action(),
                mcp_error_response_action(),
            ])
            .with_log_template(
                LogTemplate::new()
                    .with_info("MCP tools call")
                    .with_debug("MCP tools call")
                    .with_trace("MCP tools call: {json_pretty(.)}"),
            )
    });

/// MCP prompts list event
pub static MCP_PROMPTS_LIST_EVENT: std::sync::LazyLock<EventType> =
    std::sync::LazyLock::new(|| {
        EventType::new("mcp_prompts_list", "Client requests list of available prompts", json!({"type": "placeholder", "event_id": "mcp_prompts_list"}))
            .with_actions(vec![
                mcp_prompts_list_response_action(),
                mcp_error_response_action(),
            ])
            .with_log_template(
                LogTemplate::new()
                    .with_info("MCP prompts list")
                    .with_debug("MCP prompts list")
                    .with_trace("MCP prompts list: {json_pretty(.)}"),
            )
    });

/// MCP prompts get event
pub static MCP_PROMPTS_GET_EVENT: std::sync::LazyLock<EventType> =
    std::sync::LazyLock::new(|| {
        EventType::new("mcp_prompts_get", "Client requests formatted prompt template", json!({"type": "placeholder", "event_id": "mcp_prompts_get"}))
            .with_actions(vec![
                mcp_prompts_get_response_action(),
                mcp_error_response_action(),
            ])
            .with_log_template(
                LogTemplate::new()
                    .with_info("MCP prompts get")
                    .with_debug("MCP prompts get")
                    .with_trace("MCP prompts get: {json_pretty(.)}"),
            )
    });

/// Get MCP event types
fn get_mcp_event_types() -> Vec<EventType> {
    vec![
        EventType::new("mcp_initialize", "Client sends initialize request to negotiate capabilities", json!({"type": "placeholder", "event_id": "mcp_initialize"}))
            .with_actions(vec![
                mcp_initialize_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_resources_list", "Client requests list of available resources", json!({"type": "placeholder", "event_id": "mcp_resources_list"}))
            .with_actions(vec![
                mcp_resources_list_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_resources_read", "Client requests resource content by URI", json!({"type": "placeholder", "event_id": "mcp_resources_read"}))
            .with_actions(vec![
                mcp_resources_read_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_resources_subscribe", "Client subscribes to resource updates", json!({"type": "placeholder", "event_id": "mcp_resources_subscribe"}))
            .with_actions(vec![
                mcp_resources_subscribe_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_tools_list", "Client requests list of available tools", json!({"type": "placeholder", "event_id": "mcp_tools_list"}))
            .with_actions(vec![
                mcp_tools_list_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_tools_call", "Client executes a tool with parameters", json!({"type": "placeholder", "event_id": "mcp_tools_call"}))
            .with_actions(vec![
                mcp_tools_call_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_prompts_list", "Client requests list of available prompts", json!({"type": "placeholder", "event_id": "mcp_prompts_list"}))
            .with_actions(vec![
                mcp_prompts_list_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_prompts_get", "Client requests formatted prompt template", json!({"type": "placeholder", "event_id": "mcp_prompts_get"}))
            .with_actions(vec![
                mcp_prompts_get_response_action(),
                mcp_error_response_action(),
            ]),
        EventType::new("mcp_completion", "Client requests text completion suggestions", json!({"type": "placeholder", "event_id": "mcp_completion"}))
            .with_actions(vec![
                mcp_completion_response_action(),
                mcp_error_response_action(),
            ]),
    ]
}
