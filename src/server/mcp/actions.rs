//! MCP protocol actions implementation
//!
//! Defines LLM actions for controlling MCP server responses.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
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
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

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

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "mcp_initialize_response" => self.execute_mcp_initialize_response(action),
            "mcp_resources_list_response" => self.execute_mcp_resources_list_response(action),
            "mcp_resources_read_response" => self.execute_mcp_resources_read_response(action),
            "mcp_resources_subscribe_response" => self.execute_mcp_resources_subscribe_response(action),
            "mcp_tools_list_response" => self.execute_mcp_tools_list_response(action),
            "mcp_tools_call_response" => self.execute_mcp_tools_call_response(action),
            "mcp_prompts_list_response" => self.execute_mcp_prompts_list_response(action),
            "mcp_prompts_get_response" => self.execute_mcp_prompts_get_response(action),
            "mcp_completion_response" => self.execute_mcp_completion_response(action),
            "mcp_error_response" => self.execute_mcp_error_response(action),
            _ => Err(anyhow::anyhow!("Unknown MCP action: {}", action_type)),
        }
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
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

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

    fn execute_mcp_resources_list_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_resources_list".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_resources_read_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .context("Missing 'response' parameter")?
            .clone();

        Ok(ActionResult::Custom {
            name: "mcp_resources_read".to_string(),
            data: json!({"response": response}),
        })
    }

    fn execute_mcp_resources_subscribe_response(&self, action: serde_json::Value) -> Result<ActionResult> {
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
            description: "Initialize response object with protocolVersion, capabilities, and serverInfo".to_string(),
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
            description: "Tools list response with array of tool definitions including JSON schemas".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_tools_list_response",
            "response": {"tools": []}
        }),
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
            description: "Tool execution result with content array (text or image or resource)".to_string(),
            required: true,
        }],
        example: serde_json::json!({
            "type": "mcp_tools_call_response",
            "response": {"content": [{"type": "text", "text": "result"}]}
        }),
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
                description: "JSON-RPC error code (-32700 to -32603 for standard errors)".to_string(),
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
    }
}

/// MCP initialize event
pub static MCP_INITIALIZE_EVENT: std::sync::LazyLock<EventType> = std::sync::LazyLock::new(|| EventType {
    id: "mcp_initialize".to_string(),
    description: "Client sends initialize request to negotiate capabilities".to_string(),
    actions: vec![
        mcp_initialize_response_action(),
        mcp_error_response_action(),
    ],
    parameters: vec![],
});

/// MCP resources list event
pub static MCP_RESOURCES_LIST_EVENT: std::sync::LazyLock<EventType> = std::sync::LazyLock::new(|| EventType {
    id: "mcp_resources_list".to_string(),
    description: "Client requests list of available resources".to_string(),
    actions: vec![
        mcp_resources_list_response_action(),
        mcp_error_response_action(),
    ],
    parameters: vec![],
});

/// MCP resources read event
pub static MCP_RESOURCES_READ_EVENT: std::sync::LazyLock<EventType> = std::sync::LazyLock::new(|| EventType {
    id: "mcp_resources_read".to_string(),
    description: "Client requests resource content by URI".to_string(),
    actions: vec![
        mcp_resources_read_response_action(),
        mcp_error_response_action(),
    ],
    parameters: vec![],
});

/// MCP tools list event
pub static MCP_TOOLS_LIST_EVENT: std::sync::LazyLock<EventType> = std::sync::LazyLock::new(|| EventType {
    id: "mcp_tools_list".to_string(),
    description: "Client requests list of available tools".to_string(),
    actions: vec![
        mcp_tools_list_response_action(),
        mcp_error_response_action(),
    ],
    parameters: vec![],
});

/// MCP tools call event
pub static MCP_TOOLS_CALL_EVENT: std::sync::LazyLock<EventType> = std::sync::LazyLock::new(|| EventType {
    id: "mcp_tools_call".to_string(),
    description: "Client executes a tool with parameters".to_string(),
    actions: vec![
        mcp_tools_call_response_action(),
        mcp_error_response_action(),
    ],
    parameters: vec![],
});

/// MCP prompts list event
pub static MCP_PROMPTS_LIST_EVENT: std::sync::LazyLock<EventType> = std::sync::LazyLock::new(|| EventType {
    id: "mcp_prompts_list".to_string(),
    description: "Client requests list of available prompts".to_string(),
    actions: vec![
        mcp_prompts_list_response_action(),
        mcp_error_response_action(),
    ],
    parameters: vec![],
});

/// MCP prompts get event
pub static MCP_PROMPTS_GET_EVENT: std::sync::LazyLock<EventType> = std::sync::LazyLock::new(|| EventType {
    id: "mcp_prompts_get".to_string(),
    description: "Client requests formatted prompt template".to_string(),
    actions: vec![
        mcp_prompts_get_response_action(),
        mcp_error_response_action(),
    ],
    parameters: vec![],
});

/// Get MCP event types
fn get_mcp_event_types() -> Vec<EventType> {
    vec![
        EventType {
            id: "mcp_initialize".to_string(),
            description: "Client sends initialize request to negotiate capabilities".to_string(),
            actions: vec![
                mcp_initialize_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_resources_list".to_string(),
            description: "Client requests list of available resources".to_string(),
            actions: vec![
                mcp_resources_list_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_resources_read".to_string(),
            description: "Client requests resource content by URI".to_string(),
            actions: vec![
                mcp_resources_read_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_resources_subscribe".to_string(),
            description: "Client subscribes to resource updates".to_string(),
            actions: vec![
                mcp_resources_subscribe_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_tools_list".to_string(),
            description: "Client requests list of available tools".to_string(),
            actions: vec![
                mcp_tools_list_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_tools_call".to_string(),
            description: "Client executes a tool with parameters".to_string(),
            actions: vec![
                mcp_tools_call_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_prompts_list".to_string(),
            description: "Client requests list of available prompts".to_string(),
            actions: vec![
                mcp_prompts_list_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_prompts_get".to_string(),
            description: "Client requests formatted prompt template".to_string(),
            actions: vec![
                mcp_prompts_get_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
        EventType {
            id: "mcp_completion".to_string(),
            description: "Client requests text completion suggestions".to_string(),
            actions: vec![
                mcp_completion_response_action(),
                mcp_error_response_action(),
            ],
            parameters: vec![],
        },
    ]
}
