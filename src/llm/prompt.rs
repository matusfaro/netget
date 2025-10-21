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
    ) -> String {
        let mode = state.get_mode().await;
        let protocol = state.get_protocol_type().await;
        let instructions = state.get_instructions().await;
        let state_summary = state.get_summary().await;

        let data_preview = if data.len() > 100 {
            format!("{} bytes (preview: {:?}...)", data.len(), &data[..100])
        } else {
            format!("{:?}", data)
        };

        let instructions_text = if instructions.is_empty() {
            "No specific instructions provided yet.".to_string()
        } else {
            instructions.join("\n")
        };

        format!(
            r#"You are controlling a network server/client application.

Current State:
{}

Mode: {}
Protocol: {}

User Instructions:
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
  "log_message": "optional debug message"
}}

Fields:
- "output": The raw text/bytes to send. Use actual newlines (\n), carriage returns (\r), etc. Set to null or omit if no response needed.
- "close_connection": Set to true to close this specific connection
- "wait_for_more": Set to true if you need more data before responding (e.g., incomplete HTTP headers)
- "shutdown_server": Set to true to shut down the entire server
- "log_message": Optional string for debugging/logging

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
            state_summary, mode, protocol, instructions_text, connection_id, data_preview
        )
    }

    /// Build a prompt for handling user commands
    pub async fn build_user_command_prompt(
        state: &AppState,
        user_input: &str,
    ) -> String {
        let mode = state.get_mode().await;
        let protocol = state.get_protocol_type().await;
        let instructions = state.get_instructions().await;
        let state_summary = state.get_summary().await;

        let instructions_text = if instructions.is_empty() {
            "No previous instructions.".to_string()
        } else {
            instructions.join("\n")
        };

        format!(
            r#"You are controlling a network server/client application.

Current State:
{}

Mode: {}
Protocol: {}

Previous User Instructions:
{}

New User Command:
{}

Analyze the user command and respond with a JSON object containing the action to take:
{{
    "action": "listen" | "connect" | "close" | "update_protocol" | "add_file" | "status" | "change_model",
    "parameters": {{...}}
}}

Examples:
- "Listen on port 21 via FTP protocol" -> {{"action": "listen", "parameters": {{"port": 21, "protocol": "ftp"}}}}
- "Listen on TCP port 1234 and echo back everything" -> {{"action": "listen", "parameters": {{"port": 1234, "protocol": "echo"}}}}
- "Also serve a new file data.txt with content 'hello'" -> {{"action": "add_file", "parameters": {{"name": "data.txt", "content": "hello"}}}}
- "Close the current connection" -> {{"action": "close", "parameters": {{}}}}

Response:"#,
            state_summary, mode, protocol, instructions_text, user_input
        )
    }

    /// Build a prompt for connection established
    pub async fn build_connection_established_prompt(
        state: &AppState,
        connection_id: ConnectionId,
    ) -> String {
        let mode = state.get_mode().await;
        let protocol = state.get_protocol_type().await;
        let instructions = state.get_instructions().await;

        let instructions_text = if instructions.is_empty() {
            "No specific instructions provided yet.".to_string()
        } else {
            instructions.join("\n")
        };

        format!(
            r#"You are controlling a network server/client application.

Mode: {}
Protocol: {}

User Instructions:
{}

Event: New Connection Established
Connection ID: {}

Should any initial data be sent to the client? (e.g., FTP welcome message)

IMPORTANT: Respond with a JSON object with the following structure:
{{
  "output": "data to send over the wire (or null if no output)",
  "close_connection": false,
  "wait_for_more": false,
  "shutdown_server": false,
  "log_message": "optional debug message"
}}

Examples:
- FTP welcome: {{"output": "220 Welcome to FTP Server\r\n"}}
- No initial response: {{}}

Response (JSON only):"#,
            mode, protocol, instructions_text, connection_id
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
        let instructions = state.get_instructions().await;
        let state_summary = state.get_summary().await;

        let instructions_text = if instructions.is_empty() {
            "No specific instructions provided yet.".to_string()
        } else {
            instructions.join("\n")
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
  "log_message": "optional debug message"
}}

Fields:
- "status": HTTP status code (e.g., 200, 404, 500)
- "headers": Object containing response headers (e.g., {{"Content-Type": "application/json"}})
- "body": The response body as a string
- "log_message": Optional string for debugging/logging

Examples:
- Simple HTML: {{"status": 200, "headers": {{"Content-Type": "text/html"}}, "body": "<html><body>Hello!</body></html>"}}
- JSON API: {{"status": 200, "headers": {{"Content-Type": "application/json"}}, "body": "{{\\"message\\": \\"success\\"}}"}}
- Not found: {{"status": 404, "headers": {{"Content-Type": "text/plain"}}, "body": "Not Found"}}
- Echo request: {{"status": 200, "headers": {{"Content-Type": "text/plain"}}, "body": "You requested: {}"}}

Response (JSON only):"#,
            state_summary, mode, instructions_text, connection_id, method, uri, headers_text, body_text, uri
        )
    }
}
