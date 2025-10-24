//! Prompt building for LLM interactions
//!
//! This module provides two main prompt builders:
//! 1. User input handler - interprets user commands and manages the server
//! 2. Network event handler - handles incoming network events based on instructions

use crate::network::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use crate::llm::actions::{ActionDefinition, get_user_input_common_actions};

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
        let servers = state.get_all_servers().await;

        let current_state = if mode == crate::state::app_state::Mode::Server && !servers.is_empty() {
            let mut state_text = String::from("Current servers:\n");
            for server in &servers {
                state_text.push_str(&format!(
                    "- Server #{}: {} on port {}, status: {}\n",
                    server.id.as_u32(),
                    server.base_stack,
                    server.port,
                    server.status
                ));
                if !server.instruction.is_empty() {
                    state_text.push_str(&format!("  Instruction: {}\n", server.instruction));
                }
                if !server.memory.is_empty() {
                    state_text.push_str(&format!("  Memory: {}\n", server.memory));
                }
            }
            state_text
        } else {
            "No servers currently running.".to_string()
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
  "base_stack": "tcp",  // Stack: "tcp" (raw TCP), "http stack" (HTTP server), "udp", "dns", "dhcp", "ntp", "snmp", "ssh", "irc", "proxy", "webdav", "nfs"
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

4. show_message - Display a message to the user controlling NetGet:
{{
  "type": "show_message",
  "message": "Message to display"
}}

5. change_model - Switch LLM model:
{{
  "type": "change_model",
  "model": "model_name"
}}

6. set_memory - Replace entire global memory (use for protocols needing persistent state):
{{
  "type": "set_memory",
  "value": "state as string"
}}

7. append_memory - Add to existing memory:
{{
  "type": "append_memory",
  "value": "additional state info"
}}

MEMORY USAGE GUIDANCE:
- Use initial_memory when creating servers that need to track state (SSH current dir, file listings, session data)
- Use set_memory to completely replace memory (resetting state)
- Use append_memory to incrementally add state information
- Memory is a STRING, not an object. Use newlines to separate multiple values.

Examples:

User: "start an HTTP server on port 8080"
Response:
{{
  "message": "Starting HTTP server on port 8080...",
  "actions": [
    {{
      "type": "open_server",
      "port": 8080,
      "base_stack": "http stack",
      "send_first": false,
      "instruction": "You are an HTTP server. Respond to all requests with status 200 and a friendly message."
    }}
  ]
}}

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

User: "start an SSH server with a virtual filesystem"
Response:
{{
  "message": "Starting SSH server on port 22...",
  "actions": [
    {{
      "type": "open_server",
      "port": 22,
      "base_stack": "ssh",
      "send_first": false,
      "initial_memory": "cwd: /home/user\nfiles: README.md,data.txt,script.sh\nuser: guest",
      "instruction": "You are an SSH server. Track the current working directory in memory. Support shell commands:\n- pwd: Show current directory from memory\n- ls: List files from memory\n- cd: Change directory and update memory\n- cat: Display file contents\nUse set_memory or append_memory to track state changes like directory navigation."
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

        format!(
            r#"You are NetGet, an LLM-controlled network application assistant.

{}

Trigger: {}

Instructions: {}

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
            current_state, trigger_reason, instructions, actions_text
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
    // Proxy-Specific Prompts
    // ========================================================================

    /// Build prompt for HTTP request interception (MITM mode or HTTP)
    #[cfg(feature = "proxy")]
    pub async fn build_proxy_http_request_prompt(
        state: &AppState,
        server_id: ServerId,
        request_info: &crate::network::proxy_filter::FullRequestInfo,
        all_actions: Vec<ActionDefinition>,
    ) -> String {
        let instruction = state.get_instruction(server_id).await.unwrap_or_default();
        let instructions = if instruction.is_empty() {
            "Decide whether to pass, block, or modify this HTTP request."
        } else {
            &instruction
        };

        // Format request info for display
        let body_preview = if request_info.body.len() > 500 {
            format!("{}... ({} bytes total)",
                String::from_utf8_lossy(&request_info.body[..500]),
                request_info.body.len())
        } else {
            String::from_utf8_lossy(&request_info.body).to_string()
        };

        let headers_text: String = request_info.headers.iter()
            .map(|(k, v)| format!("  {}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        let event_description = format!(
            "HTTP {} {} from {}\n\n\
             Host: {}\n\
             Path: {}\n\
             Full URL: {}\n\n\
             Headers:\n{}\n\n\
             Body:\n{}",
            request_info.method,
            request_info.path,
            request_info.client_addr,
            request_info.host,
            request_info.path,
            request_info.url,
            headers_text,
            body_preview
        );

        let trigger = format!("Intercepted HTTP Request:\n{}", event_description);

        Self::build_action_prompt(state, Some(server_id), &trigger, instructions, all_actions).await
    }

    /// Build prompt for HTTPS connection decision (pass-through mode, no MITM)
    #[cfg(feature = "proxy")]
    pub async fn build_proxy_https_connection_prompt(
        state: &AppState,
        server_id: ServerId,
        conn_info: &crate::network::proxy_filter::HttpsConnectionInfo,
        all_actions: Vec<ActionDefinition>,
    ) -> String {
        let instruction = state.get_instruction(server_id).await.unwrap_or_default();
        let instructions = if instruction.is_empty() {
            "Decide whether to allow or block this HTTPS connection. Note: In pass-through mode, you cannot see or modify the encrypted content."
        } else {
            &instruction
        };

        let event_description = format!(
            "HTTPS CONNECT Request:\n\n\
             Destination: {}:{}\n\
             SNI: {}\n\
             Client: {}\n\n\
             Note: This connection uses TLS encryption. Without MITM certificate, \
             you can only allow or block the connection, but cannot inspect or modify the traffic.",
            conn_info.destination_host,
            conn_info.destination_port,
            conn_info.sni.as_ref().unwrap_or(&"(not available)".to_string()),
            conn_info.client_addr
        );

        let trigger = format!("HTTPS Connection Request:\n{}", event_description);

        Self::build_action_prompt(state, Some(server_id), &trigger, instructions, all_actions).await
    }

    /// Build prompt for HTTP response interception (MITM mode)
    #[cfg(feature = "proxy")]
    pub async fn build_proxy_http_response_prompt(
        state: &AppState,
        server_id: ServerId,
        response_info: &crate::network::proxy_filter::FullResponseInfo,
        all_actions: Vec<ActionDefinition>,
    ) -> String {
        let instruction = state.get_instruction(server_id).await.unwrap_or_default();
        let instructions = if instruction.is_empty() {
            "Decide whether to pass, block, or modify this HTTP response before returning it to the client."
        } else {
            &instruction
        };

        // Format response info for display
        let body_preview = if response_info.body.len() > 500 {
            format!("{}... ({} bytes total)",
                String::from_utf8_lossy(&response_info.body[..500]),
                response_info.body.len())
        } else {
            String::from_utf8_lossy(&response_info.body).to_string()
        };

        let headers_text: String = response_info.headers.iter()
            .map(|(k, v)| format!("  {}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        let event_description = format!(
            "HTTP Response (Status: {})\n\
             From request: {} {}\n\n\
             Headers:\n{}\n\n\
             Body:\n{}",
            response_info.status,
            response_info.request_host,
            response_info.request_path,
            headers_text,
            body_preview
        );

        let trigger = format!("Intercepted HTTP Response:\n{}", event_description);

        Self::build_action_prompt(state, Some(server_id), &trigger, instructions, all_actions).await
    }

}