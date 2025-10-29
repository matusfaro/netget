//! Action summary formatting for logging
//!
//! This module provides utilities to create concise, human-readable summaries
//! of LLM actions for logging purposes.

use super::common::CommonAction;
use super::tools::ToolAction;
use serde_json::Value;

/// Generate a summary of an action for logging
///
/// Returns a brief description like:
/// - "show_message"
/// - "open_server: HTTP:8080 \"Act as REST API...\""
/// - "read_file: schema.json (full, 1024 lines)"
pub fn summarize_action(action: &Value) -> String {
    // Try to parse as common action
    if let Ok(common) = CommonAction::from_json(action) {
        return summarize_common_action(&common);
    }

    // Try to parse as tool action
    if let Ok(tool) = ToolAction::from_json(action) {
        return tool.describe();
    }

    // Try to extract type for protocol-specific actions
    if let Some(action_type) = action.get("type").and_then(|t| t.as_str()) {
        return summarize_protocol_action(action_type, action);
    }

    // Fallback
    "unknown_action".to_string()
}

/// Summarize a common action
fn summarize_common_action(action: &CommonAction) -> String {
    match action {
        CommonAction::ShowMessage { message } => {
            let preview = if message.len() > 40 {
                format!("{}...", &message[..37])
            } else {
                message.clone()
            };
            format!("show_message: \"{}\"", preview)
        }
        CommonAction::OpenServer {
            port,
            base_stack,
            instruction,
            ..
        } => {
            let instr_preview = if instruction.len() > 30 {
                format!("{}...", &instruction[..27])
            } else {
                instruction.clone()
            };
            format!("open_server: {}:{} \"{}\"", base_stack, port, instr_preview)
        }
        CommonAction::CloseServer { server_id } => {
            if let Some(id) = server_id {
                format!("close_server: #{}", id)
            } else {
                "close_server: all".to_string()
            }
        }
        CommonAction::UpdateInstruction { instruction } => {
            let preview = if instruction.len() > 30 {
                format!("{}...", &instruction[..27])
            } else {
                instruction.clone()
            };
            format!("update_instruction: \"{}\"", preview)
        }
        CommonAction::ChangeModel { model } => {
            format!("change_model: {}", model)
        }
        CommonAction::SetMemory { value } => {
            let preview = if value.len() > 30 {
                format!("{}...", &value[..27])
            } else {
                value.clone()
            };
            format!("set_memory: \"{}\"", preview)
        }
        CommonAction::AppendMemory { value } => {
            let preview = if value.len() > 30 {
                format!("{}...", &value[..27])
            } else {
                value.clone()
            };
            format!("append_memory: \"{}\"", preview)
        }
        CommonAction::AppendToLog {
            output_name,
            content,
        } => {
            let preview = if content.len() > 30 {
                format!("{}...", &content[..27])
            } else {
                content.clone()
            };
            format!("append_to_log: {} \"{}\"", output_name, preview)
        }
    }
}

/// Summarize a protocol-specific action
fn summarize_protocol_action(action_type: &str, action: &Value) -> String {
    match action_type {
        // TCP actions
        "send_tcp_data" => {
            if let Some(data) = action.get("data").and_then(|d| d.as_str()) {
                let preview = if data.len() > 30 {
                    format!("{}...", &data[..27])
                } else {
                    data.to_string()
                };
                format!("send_tcp_data: \"{}\"", preview)
            } else {
                "send_tcp_data".to_string()
            }
        }
        "send_to_connection" => {
            if let Some(conn_id) = action.get("connection_id").and_then(|c| c.as_str()) {
                format!("send_to_connection: {}", conn_id)
            } else {
                "send_to_connection".to_string()
            }
        }
        "close_tcp_connection" => "close_tcp_connection".to_string(),

        // HTTP actions
        "send_http_response" => {
            if let Some(status) = action.get("status").and_then(|s| s.as_u64()) {
                format!("send_http_response: {}", status)
            } else {
                "send_http_response".to_string()
            }
        }

        // UDP actions
        "send_udp_response" => "send_udp_response".to_string(),
        "send_to_address" => {
            if let Some(addr) = action.get("address").and_then(|a| a.as_str()) {
                format!("send_to_address: {}", addr)
            } else {
                "send_to_address".to_string()
            }
        }

        // DNS actions
        "send_dns_response" => {
            if let Some(record_type) = action.get("record_type").and_then(|r| r.as_str()) {
                format!("send_dns_response: {}", record_type)
            } else {
                "send_dns_response".to_string()
            }
        }

        // DHCP actions
        "send_dhcp_response" => {
            if let Some(msg_type) = action.get("message_type").and_then(|m| m.as_str()) {
                format!("send_dhcp_response: {}", msg_type)
            } else {
                "send_dhcp_response".to_string()
            }
        }

        // NTP actions
        "send_ntp_response" => "send_ntp_response".to_string(),

        // SNMP actions
        "send_snmp_response" => "send_snmp_response".to_string(),
        "send_trap" => {
            if let Some(oid) = action.get("trap_oid").and_then(|o| o.as_str()) {
                format!("send_trap: {}", oid)
            } else {
                "send_trap".to_string()
            }
        }

        // SSH actions
        "send_ssh_data" => "send_ssh_data".to_string(),
        "send_ssh_exit" => "send_ssh_exit".to_string(),

        // IRC actions
        "send_irc_message" => "send_irc_message".to_string(),

        // Generic fallback
        _ => action_type.to_string(),
    }
}

/// Generate a summary of all actions in a response
///
/// Returns a string like:
/// "3 actions: → read_file: schema.json (full), → open_server: HTTP:8080 \"...\", → show_message"
pub fn summarize_actions(actions: &[Value]) -> String {
    if actions.is_empty() {
        return "0 actions".to_string();
    }

    let summaries: Vec<String> = actions
        .iter()
        .map(|a| format!("→ {}", summarize_action(a)))
        .collect();

    format!("{} actions: {}", actions.len(), summaries.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_summarize_show_message() {
        let action = json!({
            "type": "show_message",
            "message": "Server started successfully"
        });
        assert_eq!(
            summarize_action(&action),
            "show_message: \"Server started successfully\""
        );
    }

    #[test]
    fn test_summarize_open_server() {
        let action = json!({
            "type": "open_server",
            "port": 8080,
            "base_stack": "http",
            "instruction": "Act as REST API server"
        });
        assert_eq!(
            summarize_action(&action),
            "open_server: http:8080 \"Act as REST API server\""
        );
    }

    #[test]
    fn test_summarize_read_file() {
        let action = json!({
            "type": "read_file",
            "path": "schema.json",
            "mode": "full"
        });
        assert_eq!(summarize_action(&action), "read_file: schema.json (full)");
    }

    #[test]
    fn test_summarize_actions() {
        let actions = vec![
            json!({"type": "show_message", "message": "Test"}),
            json!({"type": "read_file", "path": "test.txt", "mode": "head", "lines": 10}),
        ];
        let summary = summarize_actions(&actions);
        assert!(summary.starts_with("2 actions:"));
        assert!(summary.contains("show_message"));
        assert!(summary.contains("read_file"));
    }
}
