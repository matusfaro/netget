//! Prompt building for LLM interactions

use std::collections::HashMap;

use bytes::Bytes;

use crate::network::connection::ConnectionId;
use crate::state::app_state::AppState;

/// Builder for constructing LLM prompts
pub struct PromptBuilder;

impl PromptBuilder {
    /// Build a prompt for handling received data
    pub async fn build_data_received_prompt(
        state: &AppState,
        connection_id: ConnectionId,
        data: &Bytes,
        connection_memory: &str,
    ) -> String {
        let mode = state.get_mode().await;
        let base_stack = state.get_base_stack().await;
        let instruction = state.get_instruction().await;
        let state_summary = state.get_summary().await;

        let data_preview = if data.len() > 100 {
            format!("{} bytes (preview: {:?}...)", data.len(), &data[..100])
        } else {
            format!("{:?}", data)
        };

        let instruction_text = if instruction.is_empty() {
            "No specific instructions provided yet.".to_string()
        } else {
            instruction
        };

        let memory = state.get_memory().await;
        let memory_text = if memory.is_empty() {
            "No memory stored yet.".to_string()
        } else {
            memory
        };

        let conn_memory_text = if connection_memory.is_empty() {
            "No connection memory stored yet.".to_string()
        } else {
            connection_memory.to_string()
        };

        format!(
            r#"You are controlling a network server/client application.

Current State:
{}

Mode: {}
Stack: {}

User Instructions:
{}

Global Memory (shared across all connections):
{}

Connection Memory (specific to this connection):
{}

Event: Data Received
Connection ID: {}
Data: {}

Based on the protocol and user instructions, what should be done?

IMPORTANT: Respond with a JSON object with the following structure:
{{
  "output": "data to send over the wire (or null if no output)",
  "close_connection": false,
  "wait_for_more": false,
  "shutdown_server": false,
  "log_message": "optional debug message",
  "set_memory": "completely replace memory with this text (optional)",
  "append_memory": "append this to existing memory (optional)"
}}

Fields:
- "output": The raw text/bytes to send. Use actual newlines (\n), carriage returns (\r), etc. Set to null or omit if no response needed.
- "close_connection": Set to true to close this specific connection
- "wait_for_more": Set to true if you need more data before responding (e.g., incomplete HTTP headers)
- "shutdown_server": Set to true to shut down the entire server
- "log_message": Optional string for debugging/logging
- "set_memory": Replace entire GLOBAL memory with this text. Persists across all connections. Use for server-wide state (file listings, config, etc.)
- "append_memory": Add to existing GLOBAL memory. Use for logging server-wide events.
- "set_connection_memory": Replace THIS CONNECTION's memory. Persists only for this connection. Use for per-user session data (logged in user, current directory, etc.)
- "append_connection_memory": Add to THIS CONNECTION's memory. Use for logging per-connection events.

Examples:
- FTP welcome: {{"output": "220 Welcome to FTP Server\r\n"}}
- Echo response: {{"output": "Hello\r\n"}}
- Need more data: {{"wait_for_more": true, "log_message": "Waiting for complete HTTP headers"}}
- Close connection: {{"output": "221 Goodbye\r\n", "close_connection": true}}
- No response: {{}}

For FTP protocol, respond with proper FTP response codes.
For Echo protocol, echo back the exact same data.
For other protocols, follow the protocol specification.

Response (JSON only):"#,
            state_summary, mode, base_stack, instruction_text, memory_text, conn_memory_text, connection_id, data_preview
        )
    }

    /// Build a prompt for command interpretation
    /// This prompt asks the LLM to interpret user input and return structured actions
    pub async fn build_command_interpretation_prompt(
        state: &AppState,
        user_input: &str,
    ) -> String {
        let _mode = state.get_mode().await;
        let _base_stack = state.get_base_stack().await;
        let instruction = state.get_instruction().await;
        let state_summary = state.get_summary().await;
        let local_addr = state.get_local_addr().await;

        let current_instruction_text = if instruction.is_empty() {
            "No current instruction.".to_string()
        } else {
            format!("Current instruction: \"{}\"", instruction)
        };

        let server_status = if local_addr.is_some() {
            format!("Server is currently running on {}.", local_addr.unwrap())
        } else {
            "No server currently running.".to_string()
        };

        format!(
            r#"You are an AI assistant controlling a network server/client application. Your job is to interpret user commands and return structured actions.

Current Application State:
{}
{}
{}

User Input:
"{}"

Your task: Analyze this input and determine what actions to take. Return a JSON object with this structure:

{{
  "actions": [
    {{"type": "update_instruction", "instruction": "..."}},
    {{"type": "open_server", "port": 8080, "base_stack": "tcp_raw"}},
    {{"type": "show_message", "message": "..."}},
    etc.
  ],
  "message": "optional message to display to user"
}}

Available action types:

1. **update_instruction**: Update the LLM's instruction for how to behave
   {{"type": "update_instruction", "instruction": "new instruction text"}}

2. **open_server**: Start a server
   {{"type": "open_server", "port": 21, "base_stack": "tcp_raw", "send_banner": true}}
   base_stack options: "tcp_raw", "http", "datalink"
   send_banner: true if protocol sends greeting on connect (FTP, SMTP), false if it waits for client (HTTP, SSH)

3. **open_client**: Connect as a client
   {{"type": "open_client", "address": "127.0.0.1:21", "base_stack": "tcp_raw"}}

4. **close_connection**: Close connection(s)
   {{"type": "close_connection", "connection_id": "conn-1"}}  // or omit connection_id to close all

5. **show_message**: Display a message to the user
   {{"type": "show_message", "message": "Server started successfully"}}

6. **change_model**: Change the Ollama model
   {{"type": "change_model", "model": "llama3.2:latest"}}

Examples:

User: "Listen on port 21 and pretend you are an FTP server"
Response:
{{
  "actions": [
    {{"type": "open_server", "port": 21, "base_stack": "tcp_raw", "send_banner": true}},
    {{"type": "update_instruction", "instruction": "Pretend you are an FTP server"}},
    {{"type": "show_message", "message": "FTP server started on port 21"}}
  ]
}}

User: "For the FTP server, pretend you have a file data.txt with content 'hello'"
Response:
{{
  "actions": [
    {{"type": "update_instruction", "instruction": "Pretend you are an FTP server and you have a file data.txt with content 'hello'"}},
    {{"type": "show_message", "message": "Added file data.txt to FTP server"}}
  ]
}}

User: "Start an HTTP server on port 8080"
Response:
{{
  "actions": [
    {{"type": "open_server", "port": 8080, "base_stack": "http", "send_banner": false}},
    {{"type": "show_message", "message": "HTTP server started on port 8080"}}
  ]
}}

User: "What's the current status?"
Response:
{{
  "message": "{}: {}"
}}

Important guidelines:
- When opening a server, always use base_stack: "tcp_raw" for protocols like FTP, custom TCP, etc.
- Use base_stack: "http" only when user explicitly wants HTTP stack (not just HTTP protocol over TCP)
- When updating instructions, include ALL previous instructions plus the new one - don't lose context
- Actions are executed in order
- You can return zero or more actions
- Always provide helpful messages to confirm what you did

Response (JSON only):"#,
            state_summary,
            current_instruction_text,
            server_status,
            user_input,
            state_summary,
            current_instruction_text
        )
    }

    /// Build a prompt for connection established
    pub async fn build_connection_established_prompt(
        state: &AppState,
        connection_id: ConnectionId,
    ) -> String {
        let mode = state.get_mode().await;
        let base_stack = state.get_base_stack().await;
        let instruction = state.get_instruction().await;

        let instruction_text = if instruction.is_empty() {
            "No specific instructions provided yet.".to_string()
        } else {
            instruction
        };

        let memory = state.get_memory().await;
        let memory_text = if memory.is_empty() {
            "No memory stored yet.".to_string()
        } else {
            memory
        };

        format!(
            r#"You are controlling a network server/client application.

Mode: {}
Stack: {}

User Instructions:
{}

Memory (persistent context):
{}

Event: New Connection Established
Connection ID: {}

Should any initial data be sent to the client? (e.g., FTP welcome message "220 Welcome\r\n", HTTP server banner, etc.)

IMPORTANT: Respond with a JSON object with the following structure:
{{
  "output": "data to send over the wire (or null if no output)",
  "close_connection": false,
  "wait_for_more": false,
  "shutdown_server": false,
  "log_message": "optional debug message",
  "set_memory": "completely replace memory with this text (optional)",
  "append_memory": "append this to existing memory (optional)"
}}

Examples:
- FTP welcome: {{"output": "220 Welcome to FTP Server\r\n"}}
- No initial response: {{}}

Response (JSON only):"#,
            mode, base_stack, instruction_text, memory_text, connection_id
        )
    }

    /// Build a simple status explanation prompt
    pub async fn build_status_prompt(
        state: &AppState,
        event_description: &str,
    ) -> String {
        let state_summary = state.get_summary().await;

        format!(
            r#"You are monitoring a network application.

Current State:
{}

Event: {}

Provide a brief (1-2 sentence) human-readable explanation of what just happened.

Response:"#,
            state_summary, event_description
        )
    }

    /// Build a prompt for handling HTTP requests
    pub async fn build_http_request_prompt(
        state: &AppState,
        connection_id: ConnectionId,
        method: &str,
        uri: &str,
        headers: &HashMap<String, String>,
        body: &Bytes,
    ) -> String {
        let mode = state.get_mode().await;
        let instruction = state.get_instruction().await;
        let state_summary = state.get_summary().await;

        let instruction_text = if instruction.is_empty() {
            "No specific instructions provided yet.".to_string()
        } else {
            instruction
        };

        let memory = state.get_memory().await;
        let memory_text = if memory.is_empty() {
            "No memory stored yet.".to_string()
        } else {
            memory
        };

        let headers_text = if headers.is_empty() {
            "No headers".to_string()
        } else {
            headers
                .iter()
                .map(|(k, v)| format!("  {}: {}", k, v))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let body_text = if body.is_empty() {
            "Empty body".to_string()
        } else if body.len() > 1000 {
            format!(
                "{} bytes (preview: {}...)",
                body.len(),
                String::from_utf8_lossy(&body[..1000])
            )
        } else {
            String::from_utf8_lossy(body).to_string()
        };

        format!(
            r#"You are controlling an HTTP server application.

Current State:
{}

Mode: {}
Stack: HTTP

User Instructions:
{}

Memory (persistent context):
{}

Event: HTTP Request
Connection ID: {}
Method: {}
URI: {}
Headers:
{}
Body:
{}

Based on the user instructions, generate an appropriate HTTP response.

IMPORTANT: Respond with a JSON object with the following structure:
{{
  "status": 200,
  "headers": {{"Content-Type": "text/html"}},
  "body": "response body content",
  "log_message": "optional debug message",
  "set_memory": "completely replace memory with this text (optional)",
  "append_memory": "append this to existing memory (optional)"
}}

Fields:
- "status": HTTP status code (e.g., 200, 404, 500)
- "headers": Object containing response headers (e.g., {{"Content-Type": "application/json"}})
- "body": The response body as a string
- "log_message": Optional string for debugging/logging
- "set_memory": Replace entire memory with this text. Use for storing session data, page visit counts, etc.
- "append_memory": Add to existing memory. Use for logging requests or accumulating data over time.

Examples:
- Simple HTML: {{"status": 200, "headers": {{"Content-Type": "text/html"}}, "body": "<html><body>Hello!</body></html>"}}
- JSON API: {{"status": 200, "headers": {{"Content-Type": "application/json"}}, "body": "{{\\"message\\": \\"success\\"}}"}}
- Not found: {{"status": 404, "headers": {{"Content-Type": "text/plain"}}, "body": "Not Found"}}
- Echo request: {{"status": 200, "headers": {{"Content-Type": "text/plain"}}, "body": "You requested: {}"}}

Response (JSON only):"#,
            state_summary, mode, instruction_text, memory_text, connection_id, method, uri, headers_text, body_text, uri
        )
    }
}
