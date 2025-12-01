//! SVN protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct SvnProtocol;

impl SvnProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SvnProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new() // SVN has no async actions
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_greeting_action(),
            send_success_action(),
            send_failure_action(),
            send_list_action(),
            send_response_action(),
            close_connection_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "SVN"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_svn_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>SVN"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["svn", "subversion"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(3690))
            .implementation("Manual SVN protocol implementation (custom format)")
            .llm_control("SVN commands (get-latest-rev, get-dir, get-file, update, commit)")
            .e2e_testing("svn command-line client")
            .notes("Simplified SVN protocol for testing, not full implementation")
            .build()
    }
    fn description(&self) -> &'static str {
        "SVN (Subversion) version control server"
    }
    fn example_prompt(&self) -> &'static str {
        "SVN server on port 3690 - respond to repository commands with fake data"
    }
    fn group_name(&self) -> &'static str {
        "Infrastructure"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode
            json!({
                "type": "open_server",
                "port": 3690,
                "base_stack": "svn",
                "instruction": "SVN server. Respond to commands with standard repository layout (trunk, branches, tags). Latest revision is 42."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 3690,
                "base_stack": "svn",
                "event_handlers": [
                    {
                        "event": "svn_greeting",
                        "script": "return {type='send_svn_greeting', min_version=2, max_version=2, mechanisms={'ANONYMOUS'}}"
                    },
                    {
                        "event": "svn_command",
                        "script": "if event.command == 'get-latest-rev' then return {type='send_svn_success', data='42'} else return {type='send_svn_list', items={{name='trunk', kind='dir', revision=1}, {name='branches', kind='dir', revision=1}}} end"
                    }
                ]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 3690,
                "base_stack": "svn",
                "event_handlers": [{
                    "event": "svn_greeting",
                    "static_response": [{
                        "type": "send_svn_greeting",
                        "min_version": 2,
                        "max_version": 2,
                        "mechanisms": ["ANONYMOUS"]
                    }]
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SvnProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::svn::SvnServer;
            SvnServer::spawn_with_llm_actions(
                ctx.listen_addr,
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
            "send_svn_greeting" => self.execute_send_greeting(action),
            "send_svn_success" => self.execute_send_success(action),
            "send_svn_failure" => self.execute_send_failure(action),
            "send_svn_list" => self.execute_send_list(action),
            "send_svn_response" => self.execute_send_response(action),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown SVN action: {}", action_type)),
        }
    }
}

impl SvnProtocol {
    fn execute_send_greeting(&self, action: serde_json::Value) -> Result<ActionResult> {
        let min_version = action
            .get("min_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as u32;

        let max_version = action
            .get("max_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as u32;

        let mechanisms = action
            .get("mechanisms")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_else(|| vec!["ANONYMOUS"]);

        let _realm = action
            .get("realm")
            .and_then(|v| v.as_str())
            .unwrap_or("svn");

        // SVN protocol greeting format (simplified)
        // ( success ( 2 2 ( ) ( edit-pipeline svndiff1 absent-entries ) ) )
        let mut response = format!("( success ( {} {} ( ", min_version, max_version);

        // Mechanisms list
        for (i, mech) in mechanisms.iter().enumerate() {
            if i > 0 {
                response.push(' ');
            }
            response.push_str(mech);
        }
        response.push_str(" ) ( edit-pipeline svndiff1 ) ) )\n");

        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_success(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("success");

        let data = action.get("data");

        let mut response = String::from("( success ( ");

        if let Some(data_val) = data {
            // If data is provided, include it in the response
            if let Some(data_str) = data_val.as_str() {
                response.push_str(data_str);
            } else if let Some(data_array) = data_val.as_array() {
                for (i, item) in data_array.iter().enumerate() {
                    if i > 0 {
                        response.push(' ');
                    }
                    if let Some(s) = item.as_str() {
                        response.push_str(s);
                    } else {
                        response.push_str(&item.to_string());
                    }
                }
            }
        } else {
            // Default success response
            response.push_str(message);
        }

        response.push_str(" ) )\n");

        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_failure(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_code = action
            .get("error_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(210000) as u32;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Operation failed");

        // SVN protocol error format
        let response = format!(
            "( failure ( ( {} 0 0 0 \"{}\" 0 0 ) ) )\n",
            error_code, message
        );

        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_list(&self, action: serde_json::Value) -> Result<ActionResult> {
        let items = action
            .get("items")
            .and_then(|v| v.as_array())
            .context("Missing 'items' array")?;

        let mut response = String::from("( success ( ( ");

        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                response.push_str(" ( ");
            }

            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                response.push_str(&format!("\"{}\" ", name));

                let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("file");
                response.push_str(kind);

                if let Some(size) = item.get("size").and_then(|v| v.as_u64()) {
                    response.push_str(&format!(" {} ", size));
                }

                if let Some(rev) = item.get("revision").and_then(|v| v.as_u64()) {
                    response.push_str(&format!("rev:{} ", rev));
                }
            }

            if i > 0 {
                response.push_str(" ) ");
            }
        }

        response.push_str(" ) ) )\n");

        Ok(ActionResult::Output(response.into_bytes()))
    }

    fn execute_send_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = action
            .get("response")
            .and_then(|v| v.as_str())
            .context("Missing 'response' parameter")?;

        // Ensure response ends with newline for SVN protocol
        let mut data = response.to_string();
        if !data.ends_with('\n') {
            data.push('\n');
        }

        Ok(ActionResult::Output(data.into_bytes()))
    }
}

fn send_greeting_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_svn_greeting".to_string(),
        description: "Send SVN protocol greeting with version and capabilities".to_string(),
        parameters: vec![
            Parameter {
                name: "min_version".to_string(),
                type_hint: "number".to_string(),
                description: "Minimum protocol version (default: 2)".to_string(),
                required: false,
            },
            Parameter {
                name: "max_version".to_string(),
                type_hint: "number".to_string(),
                description: "Maximum protocol version (default: 2)".to_string(),
                required: false,
            },
            Parameter {
                name: "mechanisms".to_string(),
                type_hint: "array".to_string(),
                description: "Authentication mechanisms (default: [\"ANONYMOUS\"])".to_string(),
                required: false,
            },
            Parameter {
                name: "realm".to_string(),
                type_hint: "string".to_string(),
                description: "Authentication realm (default: \"svn\")".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_svn_greeting",
            "min_version": 2,
            "max_version": 2,
            "mechanisms": ["ANONYMOUS"],
            "realm": "svn"
        }),
    }
}

fn send_success_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_svn_success".to_string(),
        description: "Send SVN success response".to_string(),
        parameters: vec![
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Success message (default: \"success\")".to_string(),
                required: false,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "string or array".to_string(),
                description: "Optional data to include in success response".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_svn_success",
            "message": "success",
            "data": "123"
        }),
    }
}

fn send_failure_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_svn_failure".to_string(),
        description: "Send SVN error/failure response".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code".to_string(),
                type_hint: "number".to_string(),
                description: "SVN error code (default: 210000)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_svn_failure",
            "error_code": 210000,
            "message": "Repository not found"
        }),
    }
}

fn send_list_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_svn_list".to_string(),
        description: "Send SVN directory listing".to_string(),
        parameters: vec![Parameter {
            name: "items".to_string(),
            type_hint: "array".to_string(),
            description: "Array of items with name, kind, size, revision fields".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_svn_list",
            "items": [
                {"name": "trunk", "kind": "dir", "revision": 1},
                {"name": "branches", "kind": "dir", "revision": 1},
                {"name": "tags", "kind": "dir", "revision": 1}
            ]
        }),
    }
}

fn send_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_svn_response".to_string(),
        description: "Send custom SVN protocol response".to_string(),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "string".to_string(),
            description: "SVN protocol response text".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_svn_response",
            "response": "( success ( 42 ) )"
        }),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the SVN connection".to_string(),
        parameters: vec![],
        example: json!({"type": "close_connection"}),
    }
}

/// SVN greeting event - triggered when client first connects
pub static SVN_GREETING_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "svn_greeting",
        "SVN client connected, send protocol greeting",
        json!({
            "type": "send_svn_greeting",
            "min_version": 2,
            "max_version": 2
        })
    )
    .with_parameters(vec![])
    .with_actions(vec![send_greeting_action(), close_connection_action()])
});

/// SVN command event - triggered when client sends a command
pub static SVN_COMMAND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("svn_command", "SVN client sent a protocol command", json!({"type": "placeholder", "event_id": "svn_command"}))
        .with_parameters(vec![
            Parameter {
                name: "command_line".to_string(),
                type_hint: "string".to_string(),
                description: "The full command line received".to_string(),
                required: true,
            },
            Parameter {
                name: "command".to_string(),
                type_hint: "string".to_string(),
                description: "The parsed command name".to_string(),
                required: true,
            },
            Parameter {
                name: "args".to_string(),
                type_hint: "array".to_string(),
                description: "Command arguments".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![
            send_success_action(),
            send_failure_action(),
            send_list_action(),
            send_response_action(),
            close_connection_action(),
        ])
});

pub fn get_svn_event_types() -> Vec<EventType> {
    vec![SVN_GREETING_EVENT.clone(), SVN_COMMAND_EVENT.clone()]
}
