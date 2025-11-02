//! Prompt building for LLM interactions
//!
//! This module provides two main prompt builders:
//! 1. User input handler - interprets user commands and manages the server
//! 2. Network event handler - handles incoming network events based on instructions

use crate::llm::actions::{
    generate_base_stack_documentation, get_all_tool_actions, get_user_input_common_actions,
    ActionDefinition,
};
use crate::llm::ollama_client::Message;
use crate::privilege::SystemCapabilities;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::ServerId;

/// Builder for constructing LLM prompts
pub struct PromptBuilder;

impl PromptBuilder {
    // ============================================================================
    // SECTION BUILDERS - These build individual sections of prompts
    // ============================================================================

    /// Build the role/identity section
    fn build_role_section() -> String {
        "You are NetGet, an LLM-controlled network server application. You control network servers by responding with JSON actions. NetGet has built-in servers for 50+ protocols (HTTP, SSH, S3, DNS, etc.) - use the available actions to control them directly.".to_string()
    }

    /// Build the role section for legacy network events
    fn build_legacy_role_section() -> String {
        "You are controlling a network server application.".to_string()
    }

    /// Build current state section (server state + system capabilities)
    async fn build_current_state_section(state: &AppState, server_id: Option<ServerId>) -> String {
        let mode = state.get_mode().await;
        let servers = state.get_all_servers().await;
        let system_caps = state.get_system_capabilities().await;

        let mut current_state = if let Some(sid) = server_id {
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

        // Append system capabilities
        current_state.push_str(&Self::build_system_capabilities_section(system_caps));
        current_state
    }

    /// Build legacy server configuration section (for old network event prompts)
    async fn build_legacy_server_config_section(state: &AppState, server_id: ServerId) -> String {
        let server = state.get_server(server_id).await;

        let (base_stack, port, memory) = if let Some(server) = server {
            (server.protocol_name, server.port, server.memory)
        } else {
            ("Unknown".to_string(), 0, String::new())
        };

        format!(
            r#"Server configuration:
- Server ID: #{}
- Stack: {}
- Port: {}
- Memory: {}
"#,
            server_id.as_u32(),
            base_stack,
            port,
            if memory.is_empty() {
                "(empty)"
            } else {
                &memory
            }
        )
    }

    /// Build system capabilities section
    fn build_system_capabilities_section(caps: SystemCapabilities) -> String {
        format!(
            "\nSystem capabilities:\n- Privileged ports (<1024): {}\n- Raw socket access: {}\n",
            if caps.can_bind_privileged_ports {
                "Available"
            } else {
                "Not available"
            },
            if caps.has_raw_socket_access {
                "Available"
            } else {
                "Not available"
            }
        )
    }

    /// Build instructions section
    fn build_instructions_section(instructions: &str) -> String {
        if instructions.is_empty() {
            String::new()
        } else {
            format!("Instructions: {}\n", instructions)
        }
    }

    /// Build actions section (formatted list of available actions)
    fn build_actions_section(actions: &[ActionDefinition]) -> String {
        if actions.is_empty() {
            return "No actions available.".to_string();
        }

        // Separate tool actions from regular actions
        let (tool_actions, regular_actions): (Vec<_>, Vec<_>) =
            actions.iter().partition(|a| a.is_tool());

        let mut text = String::new();

        // Show tool actions first if any exist
        if !tool_actions.is_empty() {
            text.push_str(
                "Available tools (these will return information and let you respond again):\n\n",
            );
            for (i, action) in tool_actions.iter().enumerate() {
                text.push_str(&format!("{}. {}\n\n", i + 1, action.to_prompt_text()));
            }
            text.push_str("\n");
        }

        // Then show regular actions
        if !regular_actions.is_empty() {
            text.push_str("Available actions for you to respond with:\n\n");
            for (i, action) in regular_actions.iter().enumerate() {
                text.push_str(&format!("{}. {}\n\n", i + 1, action.to_prompt_text()));
            }
            text.push_str("\nIMPORTANT: These actions control NetGet directly. Respond with these actions in JSON format to execute commands.\n\n");
        }

        text
    }

    /// Build base stack documentation section
    fn build_base_stack_docs_section(include_disabled: bool) -> String {
        generate_base_stack_documentation(include_disabled)
    }

    /// Build scripting section (scripting mode capabilities)
    fn build_scripting_section(selected_mode: crate::state::app_state::ScriptingMode) -> String {
        let selected_env = match selected_mode {
            crate::state::app_state::ScriptingMode::On => "python, javascript, go (LLM chooses)".to_string(),
            _ => selected_mode.as_str().to_lowercase(),
        };

        let selected_lang = match selected_mode {
            crate::state::app_state::ScriptingMode::On => "python/javascript/go".to_string(),
            _ => selected_mode.as_str().to_lowercase(),
        };

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

Scripts MUST output a JSON object with an "actions" key containing an array of action objects.
Use the SAME action types that are available to you (e.g., send_http_response, ssh_auth_decision).
DO NOT write raw protocol code (like res.writeHead() or socket operations).

Example 1 - SSH authentication (Python):
import json, sys
data = json.load(sys.stdin)
username = data['event']['username']
allowed = (username == 'alice')
print(json.dumps({{"actions": [{{"type": "ssh_auth_decision", "allowed": allowed}}]}}))

Example 2 - HTTP response (JavaScript):
const data = JSON.parse(require('fs').readFileSync(0, 'utf-8'));
const pathname = data.event.path;
const response = pathname.endsWith('.html')
  ? {{"status": 200, "headers": {{"Content-Type": "text/html"}}, "body": "<h1>Hello</h1>"}}
  : {{"status": 404, "body": "Not Found"}};
console.log(JSON.stringify({{"actions": [{{"type": "send_http_response", ...response}}]}}));

Scripts return a JSON object with actions array: {{"actions": [{{"type": "...", ...}}]}}
Scripts must complete within {} seconds or they will be terminated.
You can update scripts on running servers using the update_script action.
"#,
            selected_env,
            selected_lang,
            crate::scripting::SCRIPT_TIMEOUT_SECS
        )
    }

    /// Build memory usage section (for network events)
    fn build_memory_section() -> String {
        r#"MEMORY USAGE:
- If the protocol needs to track state (like SSH current directory, session data, file listings), use memory
- Use set_memory to completely replace memory when state changes significantly
- Use append_memory to add incremental state information
- Memory is a STRING (not an object). Use newlines to separate values. Example: "cwd: /home\nuser: alice\nfiles: a.txt,b.txt"
- Common use cases: SSH current directory tracking, session state, connection counters, file system state
"#
        .to_string()
    }

    /// Build response format section (JSON examples)
    fn build_response_format_section() -> String {
        r#"RESPONSE FORMAT:
Respond with ONLY valid JSON. Your entire response must be parseable JSON.
Format: {{"actions": [...]}}
The array can contain one or more actions, executed in order.
You can mix regular actions and tool calls in the same response.

CRITICAL: Start with {{ and end with }}. Pure JSON only.

Example response:
{"actions": [{"type": "show_message", "message": "Hello"}]}

Invalid (will fail to parse):
Here's what I'll do:
{"actions": [...]}

Invalid (will fail to parse):
```json
{"actions": [...]}
```

Valid (correct format):
{"actions": [{"type": "open_server", "port": 8080, "base_stack": "http", "instruction": "..."}]}
"#
            .to_string()
    }

    /// Build retry message for parse errors (minimal, reusable)
    ///
    /// Used when LLM returns invalid JSON. Shows the required format with one example.
    /// This is much shorter than repeating the entire prompt.
    ///
    /// # Arguments
    /// * `error` - The parse error that occurred
    pub fn build_retry_prompt(error: &str) -> String {
        format!(
            r#"ERROR: Invalid response format.

Parse error: {}

REQUIRED: Respond with ONLY valid JSON. Pure JSON only.

Start your response with {{ and end with }}.

Use this exact format:
{{"actions": [{{"type": "action_name", "param1": "value1"}}]}}

Example for opening HTTP server:
{{"actions": [{{"type": "open_server", "port": 8080, "base_stack": "http", "instruction": "Echo server"}}]}}

Now respond to the ORIGINAL request using correct JSON format."#,
            error
        )
    }

    /// Build format reminder message (added before every LLM call)
    ///
    /// This is a short system message added at the end of the conversation
    /// to remind the LLM about the required response format.
    pub fn build_format_reminder() -> String {
        r#"CRITICAL REMINDER: Respond with a JSON object with an "actions" key: {"actions": [{"type": "...", ...}, ...]}"#.to_string()
    }

    /// Filter actions based on scripting mode
    fn filter_actions_by_scripting_mode(
        actions: Vec<ActionDefinition>,
        has_scripting: bool,
    ) -> Vec<ActionDefinition> {
        if has_scripting {
            actions
        } else {
            // Remove script-related actions and parameters when LLM mode is selected
            actions
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
                                "script_language"
                                    | "script_path"
                                    | "script_inline"
                                    | "script_handles"
                            )
                        });
                        Some(action)
                    } else {
                        Some(action)
                    }
                })
                .collect()
        }
    }

    // ============================================================================
    // LEGACY NETWORK EVENT PROMPT (to be deprecated)
    // ============================================================================

    /// Build prompt for handling network events (LEGACY - uses old format)
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

        let instruction = if let Some(server) = server {
            server.instruction
        } else {
            String::new()
        };

        let (stack_context, output_format) = protocol_prompt;

        // Build using section builders
        let role = Self::build_legacy_role_section();
        let server_config = Self::build_legacy_server_config_section(state, server_id).await;
        let event_section = format!("\n{}\n\nEvent: {}\n", stack_context, event_description);
        let instruction_text = if instruction.is_empty() {
            "No specific instruction provided. Use your best judgment based on the protocol."
        } else {
            &instruction
        };
        let memory = Self::build_memory_section();

        format!(
            "{}\n\n{}{}\nUser's instruction for handling events:\n{}\n\nBased on the instruction and the event, determine the appropriate response.\n\n{}\n{}\n\nResponse (JSON only):",
            role,
            server_config,
            event_section,
            instruction_text,
            memory,
            output_format
        )
    }

    // ============================================================================
    // NEW ACTION-BASED PROMPT SYSTEM
    // ============================================================================

    /// Build unified prompt with action system (SYSTEM PROMPT ONLY)
    ///
    /// This builds the SYSTEM prompt only. The trigger/event should be provided
    /// as a separate USER message by the caller.
    ///
    /// # Arguments
    /// * `state` - Application state for context
    /// * `server_id` - Optional server ID for context
    /// * `instructions` - How to handle the situation
    /// * `available_actions` - List of actions the LLM can use
    /// * `include_base_stacks` - Whether to include full base stack documentation
    pub async fn build_action_prompt(
        state: &AppState,
        server_id: Option<ServerId>,
        instructions: &str,
        available_actions: Vec<ActionDefinition>,
        include_base_stacks: bool,
    ) -> String {
        // Get selected scripting mode
        let selected_mode = state.get_selected_scripting_mode().await;
        let has_scripting = selected_mode != crate::state::app_state::ScriptingMode::Off;

        // Build sections using section builders
        let role = Self::build_role_section();
        let current_state = Self::build_current_state_section(state, server_id).await;
        let instructions_section = Self::build_instructions_section(instructions);

        // Filter actions based on scripting mode
        let filtered_actions =
            Self::filter_actions_by_scripting_mode(available_actions, has_scripting);
        let actions_section = Self::build_actions_section(&filtered_actions);

        // Conditionally generate base stack documentation
        let include_disabled = state.get_include_disabled_protocols().await;
        let base_stack_docs = if include_base_stacks {
            Self::build_base_stack_docs_section(include_disabled)
        } else {
            String::new()
        };

        // Build scripting section if applicable
        let scripting_section = if include_base_stacks && has_scripting {
            Self::build_scripting_section(selected_mode)
        } else {
            String::new()
        };

        let response_format = Self::build_response_format_section();

        // Assemble final prompt (NO trigger - that goes in user message)
        format!(
            "{}\n\n{}{}\n{}\n{}{}{}",
            role,
            current_state,
            instructions_section,
            actions_section,
            base_stack_docs,
            scripting_section,
            response_format
        )
    }

    /// Build system prompt for user input using new action system
    ///
    /// This builds the SYSTEM prompt only (without the user input trigger).
    /// The caller should add the user input as a separate user message.
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `protocol_async_actions` - Optional async actions from active protocol
    pub async fn build_user_input_system_prompt(
        state: &AppState,
        protocol_async_actions: Vec<ActionDefinition>,
    ) -> String {
        let selected_mode = state.get_selected_scripting_mode().await;
        let scripting_env = state.get_scripting_env().await;
        let mut actions = get_user_input_common_actions(selected_mode, &scripting_env);

        // Add tool actions
        let web_search_mode = state.get_web_search_mode().await;
        actions.extend(get_all_tool_actions(web_search_mode));

        // Add protocol async actions
        actions.extend(protocol_async_actions);

        let web_search_available = web_search_mode != crate::state::app_state::WebSearchMode::Off;
        let tool_examples = if web_search_available {
            "read_file and web_search"
        } else {
            "read_file"
        };
        let instructions = format!(
            r#"Interpret what the user wants and respond with appropriate actions.
You can use tools like {} to gather information before responding.

CRITICAL: You control NetGet directly by returning JSON actions. When the user asks to "open a server" or "start a service", use the open_server action with the appropriate base_stack (http, ssh, s3, etc.). NetGet has built-in support for 50+ protocols - use them directly via actions.

Your ENTIRE response must be valid JSON in this format: {{"actions": [...]}}
No explanations, no markdown, no code blocks - only JSON.
            "#,
            tool_examples
        );

        Self::build_action_prompt(state, None, &instructions, actions, true).await
    }

    /// Convert a prompt string to conversation messages
    ///
    /// Splits a prompt into system and user messages suitable for conversation-based API.
    /// The prompt is expected to have a system instruction part and a user input part.
    ///
    /// For simplicity, this treats the entire prompt as a system message initially.
    /// TODO: Parse prompts better to separate system vs user content.
    pub fn prompt_to_messages(prompt: String) -> Vec<Message> {
        // For now, treat the whole prompt as system message
        // This is a transitional approach while we migrate to conversation-based prompts
        vec![Message::system(prompt)]
    }

    /// Build prompt for network events using new action system (SYSTEM PROMPT ONLY)
    ///
    /// This builds the SYSTEM prompt only for a network event. The caller should provide
    /// the event description and context as a separate USER message.
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `server_id` - ID of the server handling this event
    /// * `all_actions` - All actions (common + protocol + custom, pre-assembled)
    pub async fn build_network_event_action_prompt_for_server(
        state: &AppState,
        server_id: ServerId,
        mut all_actions: Vec<ActionDefinition>,
    ) -> String {
        // Add tool actions to network events
        let web_search_mode = state.get_web_search_mode().await;
        all_actions.extend(get_all_tool_actions(web_search_mode));

        // Note: all_actions already contains common + protocol + custom actions
        // They are pre-assembled by the action_helper, so we don't add common actions here
        let instruction = state.get_instruction(server_id).await.unwrap_or_default();
        let web_search_available = web_search_mode != crate::state::app_state::WebSearchMode::Off;
        let tool_examples = if web_search_available {
            "read_file and web_search"
        } else {
            "read_file"
        };

        let instructions_str = if instruction.is_empty() {
            format!(
                "Respond to the request with a set of actions. You may use these tools: {}",
                tool_examples
            )
        } else {
            instruction
        };

        // Network events don't need base stack docs (server already running, handling specific event)
        Self::build_action_prompt(state, Some(server_id), &instructions_str, all_actions, false)
            .await
    }

    /// Build event trigger message for network events
    ///
    /// This builds the USER message containing the event description and context.
    /// Should be used with build_network_event_action_prompt_for_server.
    ///
    /// # Arguments
    /// * `event_description` - Description of the network event
    /// * `context_json` - Structured context data (protocol-specific parameters)
    pub fn build_event_trigger_message(
        event_description: &str,
        context_json: serde_json::Value,
    ) -> String {
        if context_json.is_null() || context_json == serde_json::json!({}) {
            format!("Event: {}", event_description)
        } else {
            format!(
                "Event: {}\n\nContext data:\n{}",
                event_description,
                serde_json::to_string_pretty(&context_json)
                    .unwrap_or_else(|_| context_json.to_string())
            )
        }
    }

    /// Build prompt for scheduled task execution
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `task` - The scheduled task to execute
    /// * `protocol_actions` - Protocol-specific actions (if server-scoped)
    pub async fn build_task_execution_prompt(
        state: &crate::state::AppState,
        task: &crate::state::ScheduledTask,
        protocol_actions: Vec<crate::llm::actions::ActionDefinition>,
    ) -> String {
        use crate::llm::actions::{
            get_all_tool_actions, get_network_event_common_actions, get_user_input_common_actions,
        };
        use crate::state::task::TaskScope;

        let selected_mode = state.get_selected_scripting_mode().await;
        let scripting_env = state.get_scripting_env().await;

        let (server_id, actions, trigger, instructions) = match &task.scope {
            TaskScope::Global => {
                // Global task: use user input actions
                let mut actions = get_user_input_common_actions(selected_mode, &scripting_env);

                // Add tool actions
                let web_search_mode = state.get_web_search_mode().await;
                actions.extend(get_all_tool_actions(web_search_mode));

                let trigger = format!(
                    "Scheduled task '{}' triggered (created {} ago)",
                    task.name,
                    crate::state::task::format_duration(task.created_at.elapsed())
                );

                let instructions = &task.instruction;

                (None, actions, trigger, instructions.clone())
            }
            TaskScope::Server(sid) => {
                // Server-scoped task: use server instruction + protocol actions
                let server = state.get_server(*sid).await;
                if server.is_none() {
                    // Server no longer exists - return error prompt
                    return format!(
                        r#"ERROR: Server #{} no longer exists. Task '{}' cannot execute.

Return: [{{"type": "show_message", "message": "Task '{}' cancelled - server no longer exists"}}]"#,
                        sid.as_u32(),
                        task.name,
                        task.name
                    );
                }

                let mut actions = get_network_event_common_actions();
                actions.extend(protocol_actions);

                // Add tool actions
                let web_search_mode = state.get_web_search_mode().await;
                actions.extend(get_all_tool_actions(web_search_mode));

                let trigger = format!(
                    "Scheduled task '{}' triggered on server #{} (created {} ago)",
                    task.name,
                    sid.as_u32(),
                    crate::state::task::format_duration(task.created_at.elapsed())
                );

                // Combine server instruction with task instruction
                let server_instruction = state.get_instruction(*sid).await.unwrap_or_default();
                let combined = if server_instruction.is_empty() {
                    task.instruction.clone()
                } else {
                    format!(
                        "{}\n\nScheduled task: {}",
                        server_instruction, task.instruction
                    )
                };

                (Some(*sid), actions, trigger, combined)
            }
            TaskScope::Connection(sid, cid) => {
                // Connection-scoped task: use server instruction + protocol actions + connection context
                let server = state.get_server(*sid).await;
                if server.is_none() {
                    // Server no longer exists - return error prompt
                    return format!(
                        r#"ERROR: Server #{} no longer exists. Task '{}' cannot execute.

Return: [{{"type": "show_message", "message": "Task '{}' cancelled - server no longer exists"}}]"#,
                        sid.as_u32(),
                        task.name,
                        task.name
                    );
                }

                // Check if connection still exists
                let server_instance = server.unwrap();
                if !server_instance.connections.contains_key(cid) {
                    // Connection closed - task should have been cleaned up, but just in case
                    return format!(
                        r#"ERROR: Connection {} on server #{} no longer exists. Task '{}' cannot execute.

Return: [{{"type": "show_message", "message": "Task '{}' cancelled - connection closed"}}]"#,
                        cid,
                        sid.as_u32(),
                        task.name,
                        task.name
                    );
                }

                let mut actions = get_network_event_common_actions();
                actions.extend(protocol_actions);

                // Add tool actions
                let web_search_mode = state.get_web_search_mode().await;
                actions.extend(get_all_tool_actions(web_search_mode));

                // Get connection info for context
                let conn_info = server_instance.connections.get(cid).unwrap();
                let idle_duration = conn_info.last_activity.elapsed();

                let trigger = format!(
                    "Scheduled task '{}' triggered for connection {} on server #{} (created {} ago)\n\
                     Connection: {} → {}\n\
                     Bytes sent/received: {}/{}\n\
                     Packets sent/received: {}/{}\n\
                     Last activity: {:?} ago\n\
                     Status: {:?}",
                    task.name,
                    cid,
                    sid.as_u32(),
                    crate::state::task::format_duration(task.created_at.elapsed()),
                    conn_info.remote_addr,
                    conn_info.local_addr,
                    conn_info.bytes_sent,
                    conn_info.bytes_received,
                    conn_info.packets_sent,
                    conn_info.packets_received,
                    idle_duration,
                    conn_info.status
                );

                // Combine server instruction with task instruction
                let server_instruction = state.get_instruction(*sid).await.unwrap_or_default();
                let combined = if server_instruction.is_empty() {
                    task.instruction.clone()
                } else {
                    format!(
                        "{}\n\nScheduled task: {}",
                        server_instruction, task.instruction
                    )
                };

                (Some(*sid), actions, trigger, combined)
            }
        };

        // Add context data to trigger if present
        let full_trigger = if let Some(ctx) = &task.context {
            format!(
                "{}\n\nTask context:\n{}",
                trigger,
                serde_json::to_string_pretty(ctx).unwrap_or_else(|_| ctx.to_string())
            )
        } else {
            trigger
        };

        // Add previous error if this is a retry
        let instructions_with_error = if let Some(error) = &task.last_error {
            format!(
                "{}\n\nPREVIOUS EXECUTION ERROR:\nThe last execution failed with: {}\nAttempt to handle or resolve this issue.",
                instructions,
                error
            )
        } else {
            instructions
        };

        let system_prompt = Self::build_action_prompt(
            state,
            server_id,
            &instructions_with_error,
            actions,
            false, // Don't include base stack docs for tasks
        )
        .await;

        // Return system prompt + trigger as user message
        // TODO: This should be refactored to return (system_prompt, user_message) tuple
        // For now, we keep the trigger in the prompt for backwards compatibility
        format!("{}\n\nTrigger: {}", system_prompt, full_trigger)
    }

    // ========================================================================
}
