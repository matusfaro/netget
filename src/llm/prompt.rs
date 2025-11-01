//! Prompt building for LLM interactions
//!
//! This module provides two main prompt builders:
//! 1. User input handler - interprets user commands and manages the server
//! 2. Network event handler - handles incoming network events based on instructions

use crate::llm::actions::{
    generate_base_stack_documentation, get_all_tool_actions, get_user_input_common_actions,
    ActionDefinition,
};
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::ServerId;

/// Builder for constructing LLM prompts
pub struct PromptBuilder;

impl PromptBuilder {
    /// Build prompt for handling network events (legacy - no server_id)
    ///
    /// DEPRECATED: Use build_network_event_prompt_for_server instead
    /// This uses the first available server's context
    #[allow(dead_code)]
    pub async fn build_network_event_prompt(
        state: &AppState,
        _connection_id: ConnectionId,
        event_description: &str,
        protocol_prompt: (&str, &str),
    ) -> String {
        // Get first server ID as fallback
        let server_id = state
            .get_first_server_id()
            .await
            .unwrap_or(ServerId::new(1)); // Fallback to ID 1 if no servers

        Self::build_network_event_prompt_for_server(
            state,
            server_id,
            _connection_id,
            event_description,
            protocol_prompt,
        )
        .await
    }

    /// Build prompt for handling network events
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `server_id` - ID of the server handling this event
    /// * `connection_id` - Connection identifier
    /// * `event_description` - Description of the event (built by protocol-specific code)
    /// * `protocol_prompt` - (stack_context, output_format) tuple from protocol's get_llm_protocol_prompt()
    pub async fn build_network_event_prompt_for_server(
        state: &AppState,
        server_id: ServerId,
        _connection_id: ConnectionId,
        event_description: &str,
        protocol_prompt: (&str, &str),
    ) -> String {
        let server = state.get_server(server_id).await;

        let (base_stack, port, instruction, memory) = if let Some(server) = server {
            (
                server.protocol_name,
                server.port,
                server.instruction,
                server.memory,
            )
        } else {
            // Fallback if server not found
            ("Unknown".to_string(), 0, String::new(), String::new())
        };

        let (stack_context, output_format) = protocol_prompt;

        format!(
            r#"You are controlling a network server application.

Server configuration:
- Server ID: #{}
- Stack: {}
- Port: {}
- Memory: {}

{}

Event: {}

User's instruction for handling events:
{}

Based on the instruction and the event, determine the appropriate response.

MEMORY USAGE:
- If the protocol needs to track state (like SSH current directory, session data, file listings), use memory
- Use set_memory to completely replace memory when state changes significantly
- Use append_memory to add incremental state information
- Memory is a STRING (not an object). Use newlines to separate values. Example: "cwd: /home\nuser: alice\nfiles: a.txt,b.txt"
- Common use cases: SSH current directory tracking, session state, connection counters, file system state

{}

Response (JSON only):"#,
            server_id.as_u32(),
            base_stack,
            port,
            if memory.is_empty() {
                "(empty)"
            } else {
                &memory
            },
            stack_context,
            event_description,
            if instruction.is_empty() {
                "No specific instruction provided. Use your best judgment based on the protocol."
            } else {
                &instruction
            },
            output_format
        )
    }

    // ============================================================================
    // NEW ACTION-BASED PROMPT SYSTEM
    // ============================================================================

    /// Build unified prompt with action system
    ///
    /// This builds the INITIAL prompt only. In message-based conversation mode,
    /// tool results and subsequent turns are appended to the conversation history
    /// by generate_with_tools, NOT by rebuilding this prompt.
    ///
    /// # Arguments
    /// * `state` - Application state for context
    /// * `server_id` - Optional server ID for context
    /// * `trigger_reason` - Why this prompt is being called (e.g., "User said: X" or "TCP data received")
    /// * `instructions` - How to handle the situation
    /// * `available_actions` - List of actions the LLM can use
    /// * `include_base_stacks` - Whether to include full base stack documentation
    pub async fn build_action_prompt(
        state: &AppState,
        server_id: Option<ServerId>,
        trigger_reason: &str,
        instructions: &str,
        available_actions: Vec<ActionDefinition>,
        include_base_stacks: bool,
    ) -> String {
        // Get current state
        let mode = state.get_mode().await;
        let servers = state.get_all_servers().await;

        let current_state = if let Some(sid) = server_id {
            // Specific server context
            if let Some(server) = servers.iter().find(|s| s.id == sid) {
                format!(
                    r#"Current server state:
- Server ID: #{}
- Stack: {}
- Port: {}
- Status: {}
- Memory: {}
"#,
                    server.id.as_u32(),
                    server.protocol_name,
                    server.port,
                    server.status,
                    if server.memory.is_empty() {
                        "(empty)"
                    } else {
                        &server.memory
                    }
                )
            } else {
                "Server not found.".to_string()
            }
        } else if mode == crate::state::app_state::Mode::Server && !servers.is_empty() {
            // All servers context
            let mut state_text = String::from("Current servers:\n");
            for server in &servers {
                state_text.push_str(&format!(
                    "- Server #{}: {} on port {} ({})\n",
                    server.id.as_u32(),
                    server.protocol_name,
                    server.port,
                    server.status
                ));
            }
            state_text
        } else {
            "No servers currently running.".to_string()
        };

        // Build actions section (will be filtered later based on scripting availability)
        let actions_text_fn = |actions: &[ActionDefinition]| {
            if actions.is_empty() {
                "No actions available.".to_string()
            } else {
                let mut text = String::from("Available actions:\n\n");
                for (i, action) in actions.iter().enumerate() {
                    text.push_str(&format!("{}. {}\n\n", i + 1, action.to_prompt_text()));
                }
                text
            }
        };

        // Conditionally generate base stack documentation
        let include_disabled = state.get_include_disabled_protocols().await;
        let base_stack_docs = if include_base_stacks {
            // For user input when starting servers, include full documentation
            generate_base_stack_documentation(include_disabled)
        } else if server_id.is_some() {
            // For network events, don't include (server already running)
            String::new()
        } else {
            // For user input with running servers, show abbreviated list
            let stacks = crate::protocol::registry::registry().available_protocols();
            format!("\nAvailable protocol stacks: {}\n", stacks.join(", "))
        };

        // Get selected scripting mode
        let selected_mode = state.get_selected_scripting_mode().await;
        let has_scripting = selected_mode != crate::state::app_state::ScriptingMode::Llm;

        // Filter actions based on selected scripting mode
        let filtered_actions = if has_scripting {
            available_actions
        } else {
            // Remove script-related actions and parameters when LLM mode is selected
            available_actions
                .into_iter()
                .filter_map(|mut action| {
                    if action.name == "update_script" {
                        // Remove update_script action entirely when in LLM mode
                        None
                    } else if action.name == "open_server" {
                        // Remove script parameters from open_server
                        action.parameters.retain(|p| {
                            !matches!(
                                p.name.as_str(),
                                "script_language" | "script_path" | "script_inline" | "script_handles"
                            )
                        });
                        Some(action)
                    } else {
                        Some(action)
                    }
                })
                .collect()
        };

        // Note: Event types are no longer included in general prompts.
        // When using call_llm_with_event_type(), the EventType's to_prompt_description()
        // is used directly as the event description, which includes all event-specific actions.
        let event_types_info = String::new();

        // Build final actions text after filtering
        let actions_text = actions_text_fn(&filtered_actions);

        let scripting_info = if include_base_stacks && has_scripting {
            let selected_lang = selected_mode.as_str().to_lowercase();
            // Script template will be shown in protocol-specific contexts
            // when call_llm_with_event_type() is used
            let script_template = String::new();

                format!(
                    r#"

SCRIPT-BASED RESPONSES:
Selected environment: {}

Scripts are appropriate for:
- Complex SSH authentication logic (checking multiple conditions)
- Multi-step protocols requiring state machines
- When user explicitly asks for "scripted" or "programmatic" behavior

To use scripts in open_server, include:
- script_inline: "your {} script code here"
- script_handles: ["ssh_auth", "ssh_banner"] or ["all"] (optional, defaults to ["all"])

CRITICAL: Scripts must return ACTIONS in JSON format, NOT raw protocol responses.
The script receives context via stdin and must print actions to stdout.

Scripts receive JSON input via stdin with this structure:
{{
  "event_type_id": "ssh_auth",
  "server": {{"id": 1, "port": 2222, "stack": "ETH>IP>TCP>SSH", "memory": "", "instruction": "..."}},
  "connection": {{"id": "conn_123", "remote_addr": "127.0.0.1:54321", "bytes_sent": 0, "bytes_received": 0}},
  "event": {{"username": "alice", "auth_type": "password"}}
}}

Scripts MUST output JSON with an "actions" array containing action objects.
Use the SAME action types that are available to you (e.g., send_http_response, ssh_auth_decision).
DO NOT write raw protocol code (like res.writeHead() or socket operations).

Example 1 - SSH authentication (Python):
import json, sys
data = json.load(sys.stdin)
# Check if this is an SSH auth event
if data['event_type_id'] != 'ssh_auth':
    # Not our event type, return fallback to LLM
    print(json.dumps({{"fallback_to_llm": true, "fallback_reason": "Not an ssh_auth event"}}))
    sys.exit(0)
username = data['event']['username']
allowed = (username == 'alice')
print(json.dumps({{"actions": [{{"type": "ssh_auth_decision", "allowed": allowed}}]}}))

Example 2 - HTTP response (JavaScript):
const data = JSON.parse(require('fs').readFileSync(0, 'utf-8'));
// Check if this is an HTTP request event
if (data.event_type_id !== 'http_request') {{
    // Not our event type, return fallback to LLM
    console.log(JSON.stringify({{fallback_to_llm: true, fallback_reason: "Not an http_request event"}}));
    process.exit(0);
}}
const pathname = data.event.path;
const response = pathname.endsWith('.html')
  ? {{"status": 200, "headers": {{"Content-Type": "text/html"}}, "body": "<h1>Hello</h1>"}}
  : {{"status": 404, "body": "Not Found"}};
console.log(JSON.stringify({{"actions": [{{"type": "send_http_response", ...response}}]}}));

Scripts must complete within {} seconds or they will be terminated.
Scripts can return {{"fallback_to_llm": true}} to delegate complex cases to LLM.
You can update scripts on running servers using the update_script action.
{}
"#,
                selected_lang,
                selected_lang,
                crate::scripting::SCRIPT_TIMEOUT_SECS,
                script_template
            )
        } else {
            String::new()
        };

        format!(
            r#"You are NetGet, an LLM-controlled network application assistant.

{}

Trigger: {}

Instructions: {}

{}

{}{}{}

RESPONSE FORMAT:
Respond with JSON: {{"actions": [...]}}
The "actions" array can contain one or more actions, executed in order.
You can mix regular actions and tool calls in the same response.

Example with tools:
{{
  "actions": [
    {{"type": "read_file", "path": "schema.json", "mode": "full"}},
    {{"type": "show_message", "message": "Reading schema..."}}
  ]
}}

Example with server:
{{
  "actions": [
    {{"type": "open_server", "port": 8080, "base_stack": "http", "instruction": "Act as REST API"}},
    {{"type": "show_message", "message": "Server started"}}
  ]
}}

Response (JSON only):"#,
            current_state, trigger_reason, instructions, actions_text, event_types_info, base_stack_docs, scripting_info
        )
    }

    /// Build prompt for user input using new action system
    ///
    /// This builds the INITIAL prompt for user input. Subsequent turns in the conversation
    /// will append to the message history automatically (handled by generate_with_tools).
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `user_input` - What the user typed
    /// * `protocol_async_actions` - Optional async actions from active protocol
    pub async fn build_user_input_action_prompt(
        state: &AppState,
        user_input: &str,
        protocol_async_actions: Vec<ActionDefinition>,
    ) -> String {
        let selected_mode = state.get_selected_scripting_mode().await;
        let mut actions = get_user_input_common_actions(selected_mode);

        // Add tool actions
        let web_search_mode = state.get_web_search_mode().await;
        actions.extend(get_all_tool_actions(web_search_mode));

        // Add protocol async actions
        actions.extend(protocol_async_actions);

        let trigger = format!("User input: \"{}\"", user_input);
        let web_search_available = web_search_mode != crate::state::app_state::WebSearchMode::Off;
        let instructions = if web_search_available {
            "Interpret what the user wants and respond with appropriate actions. You can use tools like read_file and web_search to gather information before responding.\n\nIMPORTANT: If the user requests changes to an existing server (e.g., 'add a new endpoint', 'update the response', 'change the behavior'), DO NOT open another server on the same port. Instead, update the existing server's instruction using the update_instruction action. Only use open_server when explicitly asked to create a NEW server on a DIFFERENT port."
        } else {
            "Interpret what the user wants and respond with appropriate actions. You can use tools like read_file to gather information before responding.\n\nIMPORTANT: If the user requests changes to an existing server (e.g., 'add a new endpoint', 'update the response', 'change the behavior'), DO NOT open another server on the same port. Instead, update the existing server's instruction using the update_instruction action. Only use open_server when explicitly asked to create a NEW server on a DIFFERENT port."
        };

        // Include full base stack docs only if no servers are running
        // (user might want to start a new server)
        let servers = state.get_all_servers().await;
        let include_base_stacks = servers.is_empty();

        Self::build_action_prompt(
            state,
            None,
            &trigger,
            instructions,
            actions,
            include_base_stacks,
        )
        .await
    }

    /// Build prompt for network events using new action system (legacy - no server_id)
    ///
    /// DEPRECATED: Use build_network_event_action_prompt_for_server instead
    /// This uses the first available server's context
    #[allow(dead_code)]
    pub async fn build_network_event_action_prompt(
        state: &AppState,
        event_description: &str,
        protocol_sync_actions: Vec<ActionDefinition>,
    ) -> String {
        // Get first server ID as fallback
        let server_id = state
            .get_first_server_id()
            .await
            .unwrap_or(ServerId::new(1)); // Fallback to ID 1 if no servers

        Self::build_network_event_action_prompt_for_server(
            state,
            server_id,
            event_description,
            serde_json::json!({}), // Empty context
            protocol_sync_actions,
        )
        .await
    }

    /// Build prompt for network events using new action system
    ///
    /// This builds the INITIAL prompt for a network event. Subsequent turns in the conversation
    /// will append to the message history automatically (handled by generate_with_tools).
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `server_id` - ID of the server handling this event
    /// * `event_description` - Description of the network event
    /// * `context_json` - Structured context data (protocol-specific parameters)
    /// * `all_actions` - All actions (common + protocol + custom, pre-assembled)
    pub async fn build_network_event_action_prompt_for_server(
        state: &AppState,
        server_id: ServerId,
        event_description: &str,
        context_json: serde_json::Value,
        mut all_actions: Vec<ActionDefinition>,
    ) -> String {
        // Add tool actions to network events
        let web_search_mode = state.get_web_search_mode().await;
        all_actions.extend(get_all_tool_actions(web_search_mode));

        // Note: all_actions already contains common + protocol + custom actions
        // They are pre-assembled by the action_helper, so we don't add common actions here
        let instruction = state.get_instruction(server_id).await.unwrap_or_default();
        let web_search_available = web_search_mode != crate::state::app_state::WebSearchMode::Off;
        let instructions_str = if instruction.is_empty() {
            if web_search_available {
                "No specific instruction provided. Use your best judgment based on the protocol and event. You can use tools like read_file and web_search to gather information before responding."
            } else {
                "No specific instruction provided. Use your best judgment based on the protocol and event. You can use tools like read_file to gather information before responding."
            }
        } else {
            &instruction
        };

        // Build trigger with structured context
        let trigger = if context_json.is_null() || context_json == serde_json::json!({}) {
            format!("Event: {}", event_description)
        } else {
            format!(
                "Event: {}\n\nContext data:\n{}",
                event_description,
                serde_json::to_string_pretty(&context_json)
                    .unwrap_or_else(|_| context_json.to_string())
            )
        };

        // Network events don't need base stack docs (server already running, handling specific event)
        Self::build_action_prompt(
            state,
            Some(server_id),
            &trigger,
            instructions_str,
            all_actions,
            false,
        )
        .await
    }

    // ========================================================================
}
