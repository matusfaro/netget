//! Prompt building for LLM interactions
//!
//! This module provides two main prompt builders:
//! 1. User input handler - interprets user commands and manages the server
//! 2. Network event handler - handles incoming network events based on instructions

use crate::network::connection::ConnectionId;
use crate::state::app_state::AppState;

/// Builder for constructing LLM prompts
pub struct PromptBuilder;

impl PromptBuilder {
    /// Build prompt for handling user input
    /// This prompt interprets what the user wants and returns appropriate actions
    pub async fn build_user_input_prompt(
        state: &AppState,
        user_input: &str,
    ) -> String {
        // Get current state
        let mode = state.get_mode().await;
        let base_stack = state.get_base_stack().await;
        let port = state.get_port().await;
        let instruction = state.get_instruction().await;
        let memory = state.get_memory().await;

        let current_state = if mode == crate::state::app_state::Mode::Server {
            format!(
                r#"Current server state:
- Mode: Server
- Stack: {}
- Port: {:?}
- Current instruction: {}
- Global memory: {}
"#,
                base_stack,
                port,
                if instruction.is_empty() { "(none)" } else { &instruction },
                if memory.is_empty() { "(empty)" } else { &memory }
            )
        } else {
            "No server currently running.".to_string()
        };

        format!(
            r#"You are NetGet, an LLM-controlled network application assistant.

{}

User input: "{}"

Interpret what the user wants and respond with appropriate actions.

IMPORTANT: Always respond with a JSON object with this structure:
{{
  "message": "A helpful response to show the user",
  "actions": [
    // One or more actions (optional)
  ]
}}

Available actions:

1. open_server - Start a new server:
{{
  "type": "open_server",
  "port": 8080,  // Port number
  "base_stack": "tcp",  // Stack: tcp, http, udp, snmp, dns, dhcp, ntp, ssh, irc
  "send_first": true,  // True if server sends data first (FTP, SMTP), false if it waits for client (HTTP)
  "initial_memory": null,  // Optional initial memory
  "instruction": "Detailed instructions for handling network events..."
}}

2. update_instruction - Update the current server instruction:
{{
  "type": "update_instruction",
  "instruction": "New combined instruction..."
}}
Note: Combine the old instruction with the new requirement.

3. close_server - Stop the current server:
{{
  "type": "close_server"
}}

4. show_message - Display a message:
{{
  "type": "show_message",
  "message": "Message to display"
}}

5. change_model - Switch LLM model:
{{
  "type": "change_model",
  "model": "model_name"
}}

Examples:

User: "start an FTP server"
Response:
{{
  "message": "Starting FTP server on port 21...",
  "actions": [
    {{
      "type": "open_server",
      "port": 21,
      "base_stack": "tcp",
      "send_first": true,
      "instruction": "You are an FTP server. Respond to FTP commands:\n- USER: Accept any username\n- PASS: Accept any password\n- PWD: Return current directory\n- LIST: Return file listing\n- RETR: Return file contents\n- QUIT: Close connection\nSend appropriate FTP response codes."
    }}
  ]
}}

User: "always return 404"
Response:
{{
  "message": "Updating server to always return HTTP 404...",
  "actions": [
    {{
      "type": "update_instruction",
      "instruction": "For all HTTP requests, return status 404 with 'Not Found' message."
    }}
  ]
}}

User: "but return 200 for /health"
Response:
{{
  "message": "Updated server to return 200 for /health, 404 for everything else.",
  "actions": [
    {{
      "type": "update_instruction",
      "instruction": "For HTTP requests: If path is /health, return status 200 with 'OK'. For all other paths, return status 404 with 'Not Found'."
    }}
  ]
}}

Response (JSON only):"#,
            current_state,
            user_input
        )
    }

    /// Build prompt for handling network events
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `connection_id` - Connection identifier
    /// * `connection_memory` - Per-connection memory
    /// * `event_description` - Description of the event (built by protocol-specific code)
    /// * `prompt_config` - (stack_context, output_format) tuple from protocol's get_llm_prompt_config()
    pub async fn build_network_event_prompt(
        state: &AppState,
        _connection_id: ConnectionId,
        connection_memory: &str,
        event_description: &str,
        prompt_config: (&str, &str),
    ) -> String {
        let base_stack = state.get_base_stack().await;
        let port = state.get_port().await.unwrap_or(0);
        let instruction = state.get_instruction().await;
        let global_memory = state.get_memory().await;

        let (stack_context, output_format) = prompt_config;

        format!(
            r#"You are controlling a network server application.

Server configuration:
- Stack: {}
- Port: {}
- Global memory: {}
- Connection memory: {}

{}

Event: {}

User's instruction for handling events:
{}

Based on the instruction and the event, determine the appropriate response.

{}

Response (JSON only):"#,
            base_stack,
            port,
            if global_memory.is_empty() { "(empty)" } else { &global_memory },
            if connection_memory.is_empty() { "(empty)" } else { connection_memory },
            stack_context,
            event_description,
            if instruction.is_empty() { "No specific instruction provided. Use your best judgment based on the protocol." } else { &instruction },
            output_format
        )
    }

}