//! Prompt building for LLM interactions

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

Based on the protocol and user instructions, what data should be sent back (if any)?

IMPORTANT: Respond with ONLY the raw text to send - NOT a debug representation, NOT wrapped in quotes, NOT prefixed with b".
Just output the exact characters that should be sent over the wire.
Use actual newlines (not \n), actual carriage returns (not \r).
If no response is needed, respond with "NO_RESPONSE".
If you need to close the connection, respond with "CLOSE_CONNECTION".

For FTP protocol, respond with proper FTP response codes (e.g., "220 Welcome\r\n").
For Echo protocol, echo back the exact same data.
For other protocols, follow the protocol specification.

Response:"#,
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

IMPORTANT: Respond with ONLY the raw text to send - NOT a debug representation, NOT wrapped in quotes, NOT prefixed with b".
Just output the exact characters that should be sent over the wire.
Use actual newlines (not \n), actual carriage returns (not \r).
If nothing should be sent, respond with "NO_RESPONSE".

For FTP protocol, send a proper welcome message (e.g., "220 Welcome\r\n").

Response:"#,
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
}
