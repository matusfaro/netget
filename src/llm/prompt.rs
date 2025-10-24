//! Prompt building for LLM interactions
//!
//! This module provides two main prompt builders:
//! 1. User input handler - interprets user commands and manages the server
//! 2. Network event handler - handles incoming network events based on instructions

use crate::network::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use crate::llm::actions::{ActionDefinition, get_user_input_common_actions, generate_base_stack_documentation};

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
        let server_id = state.get_first_server_id().await
            .unwrap_or(ServerId::new(1)); // Fallback to ID 1 if no servers

        Self::build_network_event_prompt_for_server(
            state,
            server_id,
            _connection_id,
            event_description,
            protocol_prompt,
        ).await
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
                server.base_stack.to_string(),
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
            if memory.is_empty() { "(empty)" } else { &memory },
            stack_context,
            event_description,
            if instruction.is_empty() { "No specific instruction provided. Use your best judgment based on the protocol." } else { &instruction },
            output_format
        )
    }

    // ============================================================================
    // NEW ACTION-BASED PROMPT SYSTEM
    // ============================================================================

    /// Build unified prompt with action system
    ///
    /// # Arguments
    /// * `state` - Application state for context
    /// * `server_id` - Optional server ID for context
    /// * `trigger_reason` - Why this prompt is being called (e.g., "User said: X" or "TCP data received")
    /// * `instructions` - How to handle the situation
    /// * `available_actions` - List of actions the LLM can use
    pub async fn build_action_prompt(
        state: &AppState,
        server_id: Option<ServerId>,
        trigger_reason: &str,
        instructions: &str,
        available_actions: Vec<ActionDefinition>,
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
                    server.base_stack,
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
                    server.id.as_u32(), server.base_stack, server.port, server.status
                ));
            }
            state_text
        } else {
            "No servers currently running.".to_string()
        };

        // Build actions section
        let actions_text = if available_actions.is_empty() {
            "No actions available.".to_string()
        } else {
            let mut text = String::from("Available actions:\n\n");
            for (i, action) in available_actions.iter().enumerate() {
                text.push_str(&format!("{}. {}\n\n", i + 1, action.to_prompt_text()));
            }
            text
        };

        // Generate base stack documentation
        let base_stack_docs = generate_base_stack_documentation();

        format!(
            r#"You are NetGet, an LLM-controlled network application assistant.

{}

Trigger: {}

Instructions: {}

{}

{}

IMPORTANT: Respond with a JSON object containing an "actions" array.
The "actions" field MUST ALWAYS be an array, even for a single action.

For a single action:
{{
  "actions": [
    {{"type": "action_name", "param1": "value1", ...}}
  ]
}}

For multiple actions:
{{
  "actions": [
    {{"type": "first_action", "param1": "value1", ...}},
    {{"type": "second_action", "param2": "value2", ...}}
  ]
}}

The actions will be executed in order.

Response (JSON only):"#,
            current_state, trigger_reason, instructions, actions_text, base_stack_docs
        )
    }

    /// Build prompt for user input using new action system
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
        let mut actions = get_user_input_common_actions();
        actions.extend(protocol_async_actions);

        let trigger = format!("User input: \"{}\"", user_input);
        let instructions =
            "Interpret what the user wants and respond with appropriate actions.";

        Self::build_action_prompt(state, None, &trigger, instructions, actions).await
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
        let server_id = state.get_first_server_id().await
            .unwrap_or(ServerId::new(1)); // Fallback to ID 1 if no servers

        Self::build_network_event_action_prompt_for_server(
            state,
            server_id,
            event_description,
            protocol_sync_actions,
        ).await
    }

    /// Build prompt for network events using new action system
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `server_id` - ID of the server handling this event
    /// * `event_description` - Description of the network event
    /// * `protocol_sync_actions` - Sync actions from protocol (with context)
    pub async fn build_network_event_action_prompt_for_server(
        state: &AppState,
        server_id: ServerId,
        event_description: &str,
        all_actions: Vec<ActionDefinition>,
    ) -> String {
        // Note: all_actions already contains common + protocol + custom actions
        // They are pre-assembled by the action_helper, so we don't add common actions here
        let instruction = state.get_instruction(server_id).await.unwrap_or_default();
        let instructions = if instruction.is_empty() {
            "No specific instruction provided. Use your best judgment based on the protocol and event."
        } else {
            &instruction
        };

        let trigger = format!("Event: {}", event_description);

        Self::build_action_prompt(state, Some(server_id), &trigger, instructions, all_actions).await
    }

    // ========================================================================
}