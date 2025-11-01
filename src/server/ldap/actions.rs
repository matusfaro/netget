//! LDAP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// LDAP protocol action handler
pub struct LdapProtocol;

impl LdapProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_ldap_bind_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message_id = action
            .get("message_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as i32;

        let success = action
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let result_code = if success { 0 } else { 49 }; // 0 = success, 49 = invalidCredentials

        debug!("LDAP sending bind response: success={}, message={}", success, message);

        let response = encode_bind_response(message_id, result_code, message);
        Ok(ActionResult::Output(response))
    }

    fn execute_ldap_search_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message_id = action
            .get("message_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as i32;

        let entries = action
            .get("entries")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let result_code = action
            .get("result_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;

        debug!("LDAP sending search response: {} entries, result_code={}", entries.len(), result_code);

        // Build response with search entries + search done
        let mut response = Vec::new();

        // Send SearchResultEntry for each entry
        for entry in entries {
            let dn = entry
                .get("dn")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let attributes = entry
                .get("attributes")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();

            response.extend_from_slice(&encode_search_entry(message_id, dn, attributes));
        }

        // Send SearchResultDone
        response.extend_from_slice(&encode_search_done(message_id, result_code, ""));

        Ok(ActionResult::Output(response))
    }

    fn execute_ldap_add_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message_id = action
            .get("message_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as i32;

        let success = action
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let result_code = action
            .get("result_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(if success { 0 } else { 68 }) as u8; // 68 = entryAlreadyExists

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        debug!("LDAP sending add response: success={}, result_code={}", success, result_code);

        let response = encode_ldap_result(message_id, 0x69, result_code, message); // 0x69 = AddResponse
        Ok(ActionResult::Output(response))
    }

    fn execute_ldap_modify_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message_id = action
            .get("message_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as i32;

        let success = action
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let result_code = action
            .get("result_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(if success { 0 } else { 32 }) as u8; // 32 = noSuchObject

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        debug!("LDAP sending modify response: success={}, result_code={}", success, result_code);

        let response = encode_ldap_result(message_id, 0x67, result_code, message); // 0x67 = ModifyResponse
        Ok(ActionResult::Output(response))
    }

    fn execute_ldap_delete_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message_id = action
            .get("message_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as i32;

        let success = action
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let result_code = action
            .get("result_code")
            .and_then(|v| v.as_u64())
            .unwrap_or(if success { 0 } else { 32 }) as u8; // 32 = noSuchObject

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        debug!("LDAP sending delete response: success={}, result_code={}", success, result_code);

        let response = encode_ldap_result(message_id, 0x6B, result_code, message); // 0x6B = DelResponse
        Ok(ActionResult::Output(response))
    }
}

impl Server for LdapProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::ldap::LdapServer;
            let _send_first = ctx.startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            LdapServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }


    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "send_first".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether the server should send the first message after connection (not typically needed for this protocol)".to_string(),
                required: false,
                example: serde_json::json!(false),
            },
        ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // LDAP doesn't need async actions for now
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ldap_bind_response_action(),
            ldap_search_response_action(),
            ldap_add_response_action(),
            ldap_modify_response_action(),
            ldap_delete_response_action(),
            wait_for_more_action(),
            close_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "ldap_bind_response" => self.execute_ldap_bind_response(action),
            "ldap_search_response" => self.execute_ldap_search_response(action),
            "ldap_add_response" => self.execute_ldap_add_response(action),
            "ldap_modify_response" => self.execute_ldap_modify_response(action),
            "ldap_delete_response" => self.execute_ldap_delete_response(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown LDAP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "LDAP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_ldap_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>LDAP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ldap", "directory server"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::new(
            crate::protocol::metadata::DevelopmentState::Alpha
        )
    }

    fn description(&self) -> &'static str {
        "LDAP directory server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an LDAP directory server on port 389"
    }

    fn group_name(&self) -> &'static str {
        "Application Protocols"
    }
}

// ============================================================================
// Action Definitions
// ============================================================================

fn ldap_bind_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ldap_bind_response".to_string(),
        description: "Respond to LDAP bind (authentication) request".to_string(),
        parameters: vec![
            Parameter {
                name: "message_id".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP message ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "success".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether bind was successful".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Optional diagnostic message".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "ldap_bind_response",
            "message_id": 1,
            "success": true,
            "message": "Bind successful"
        }),
    }
}

fn ldap_search_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ldap_search_response".to_string(),
        description: "Respond to LDAP search request with directory entries".to_string(),
        parameters: vec![
            Parameter {
                name: "message_id".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP message ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "entries".to_string(),
                type_hint: "array".to_string(),
                description: "Array of directory entries matching the search".to_string(),
                required: true,
            },
            Parameter {
                name: "result_code".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP result code (0 = success, default: 0)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "ldap_search_response",
            "message_id": 2,
            "entries": [
                {
                    "dn": "cn=john,ou=people,dc=example,dc=com",
                    "attributes": {
                        "cn": ["john"],
                        "mail": ["john@example.com"],
                        "objectClass": ["person", "inetOrgPerson"]
                    }
                }
            ],
            "result_code": 0
        }),
    }
}

fn ldap_add_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ldap_add_response".to_string(),
        description: "Respond to LDAP add (create entry) request".to_string(),
        parameters: vec![
            Parameter {
                name: "message_id".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP message ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "success".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether add was successful".to_string(),
                required: true,
            },
            Parameter {
                name: "result_code".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP result code (0 = success, 68 = entryAlreadyExists)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Optional diagnostic message".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "ldap_add_response",
            "message_id": 3,
            "success": true,
            "result_code": 0,
            "message": "Entry added successfully"
        }),
    }
}

fn ldap_modify_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ldap_modify_response".to_string(),
        description: "Respond to LDAP modify (update entry) request".to_string(),
        parameters: vec![
            Parameter {
                name: "message_id".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP message ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "success".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether modify was successful".to_string(),
                required: true,
            },
            Parameter {
                name: "result_code".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP result code (0 = success, 32 = noSuchObject)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Optional diagnostic message".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "ldap_modify_response",
            "message_id": 4,
            "success": true,
            "result_code": 0,
            "message": "Entry modified successfully"
        }),
    }
}

fn ldap_delete_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "ldap_delete_response".to_string(),
        description: "Respond to LDAP delete (remove entry) request".to_string(),
        parameters: vec![
            Parameter {
                name: "message_id".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP message ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "success".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether delete was successful".to_string(),
                required: true,
            },
            Parameter {
                name: "result_code".to_string(),
                type_hint: "number".to_string(),
                description: "LDAP result code (0 = success, 32 = noSuchObject)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Optional diagnostic message".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "ldap_delete_response",
            "message_id": 5,
            "success": true,
            "result_code": 0,
            "message": "Entry deleted successfully"
        }),
    }
}

fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data before responding".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the LDAP connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}

// ============================================================================
// Action Constants
// ============================================================================

pub static LDAP_BIND_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| ldap_bind_response_action());
pub static LDAP_SEARCH_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| ldap_search_response_action());
pub static LDAP_ADD_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| ldap_add_response_action());
pub static LDAP_MODIFY_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| ldap_modify_response_action());
pub static LDAP_DELETE_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| ldap_delete_response_action());
pub static WAIT_FOR_MORE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| wait_for_more_action());
pub static CLOSE_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| close_connection_action());

// ============================================================================
// Event Type Constants
// ============================================================================

/// LDAP bind event - triggered when client attempts to authenticate
pub static LDAP_BIND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ldap_bind",
        "LDAP bind (authentication) request received"
    )
    .with_parameters(vec![
        Parameter {
            name: "message_id".to_string(),
            type_hint: "number".to_string(),
            description: "LDAP message ID".to_string(),
            required: true,
        },
        Parameter {
            name: "version".to_string(),
            type_hint: "number".to_string(),
            description: "LDAP protocol version (typically 3)".to_string(),
            required: true,
        },
        Parameter {
            name: "dn".to_string(),
            type_hint: "string".to_string(),
            description: "Distinguished Name for authentication".to_string(),
            required: true,
        },
        Parameter {
            name: "password".to_string(),
            type_hint: "string".to_string(),
            description: "Password for simple authentication".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        LDAP_BIND_RESPONSE_ACTION.clone(),
        CLOSE_CONNECTION_ACTION.clone(),
    ])
});

/// LDAP search event - triggered when client performs a directory search
pub static LDAP_SEARCH_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ldap_search",
        "LDAP search request received"
    )
    .with_parameters(vec![
        Parameter {
            name: "message_id".to_string(),
            type_hint: "number".to_string(),
            description: "LDAP message ID".to_string(),
            required: true,
        },
        Parameter {
            name: "base_dn".to_string(),
            type_hint: "string".to_string(),
            description: "Base DN for search (starting point)".to_string(),
            required: true,
        },
        Parameter {
            name: "authenticated".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether client is authenticated".to_string(),
            required: true,
        },
        Parameter {
            name: "bind_dn".to_string(),
            type_hint: "string".to_string(),
            description: "DN of authenticated user (empty if not authenticated)".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        LDAP_SEARCH_RESPONSE_ACTION.clone(),
        CLOSE_CONNECTION_ACTION.clone(),
    ])
});

/// LDAP unbind event - triggered when client closes connection
pub static LDAP_UNBIND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ldap_unbind",
        "LDAP unbind (disconnect) request received"
    )
    .with_parameters(vec![
        Parameter {
            name: "bind_dn".to_string(),
            type_hint: "string".to_string(),
            description: "DN of authenticated user (empty if not authenticated)".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![])
});

/// Get LDAP event types
pub fn get_ldap_event_types() -> Vec<EventType> {
    vec![
        LDAP_BIND_EVENT.clone(),
        LDAP_SEARCH_EVENT.clone(),
        LDAP_UNBIND_EVENT.clone(),
    ]
}

// ============================================================================
// BER Encoding Helpers
// ============================================================================

fn encode_ber_length(length: usize) -> Vec<u8> {
    if length < 128 {
        vec![length as u8]
    } else if length < 256 {
        vec![0x81, length as u8]
    } else if length < 65536 {
        vec![0x82, (length >> 8) as u8, length as u8]
    } else {
        vec![0x83, (length >> 16) as u8, (length >> 8) as u8, length as u8]
    }
}

fn encode_ber_integer(value: i32) -> Vec<u8> {
    let mut result = vec![0x02]; // INTEGER tag

    if value >= 0 && value < 128 {
        result.push(0x01); // length
        result.push(value as u8);
    } else {
        result.push(0x04); // length (4 bytes)
        result.extend_from_slice(&value.to_be_bytes());
    }

    result
}

fn encode_ber_string(s: &str) -> Vec<u8> {
    let mut result = vec![0x04]; // OCTET STRING tag
    let bytes = s.as_bytes();
    result.extend_from_slice(&encode_ber_length(bytes.len()));
    result.extend_from_slice(bytes);
    result
}

fn encode_ldap_message(msg_id: i32, protocol_op: Vec<u8>) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend_from_slice(&encode_ber_integer(msg_id));
    content.extend_from_slice(&protocol_op);

    let mut message = vec![0x30]; // SEQUENCE tag
    message.extend_from_slice(&encode_ber_length(content.len()));
    message.extend_from_slice(&content);

    message
}

fn encode_bind_response(msg_id: i32, result_code: u8, diagnostic_message: &str) -> Vec<u8> {
    let mut bind_resp = Vec::new();

    // resultCode (ENUMERATED)
    bind_resp.push(0x0A);
    bind_resp.push(0x01);
    bind_resp.push(result_code);

    // matchedDN (empty)
    bind_resp.push(0x04);
    bind_resp.push(0x00);

    // diagnosticMessage
    bind_resp.extend_from_slice(&encode_ber_string(diagnostic_message));

    // Wrap in BindResponse APPLICATION tag [1]
    let mut bind_msg = vec![0x61];
    bind_msg.extend_from_slice(&encode_ber_length(bind_resp.len()));
    bind_msg.extend_from_slice(&bind_resp);

    encode_ldap_message(msg_id, bind_msg)
}

fn encode_search_entry(msg_id: i32, dn: &str, attributes: serde_json::Map<String, serde_json::Value>) -> Vec<u8> {
    // SearchResultEntry ::= [APPLICATION 4] SEQUENCE {
    //     objectName LDAPDN,
    //     attributes PartialAttributeList }

    let mut entry_content = Vec::new();

    // objectName (DN)
    entry_content.extend_from_slice(&encode_ber_string(dn));

    // attributes (SEQUENCE OF)
    let mut attrs_content = Vec::new();
    for (attr_name, attr_values) in attributes {
        // PartialAttribute ::= SEQUENCE {
        //     type AttributeDescription,
        //     vals SET OF value AttributeValue }

        let mut attr_content = Vec::new();

        // type (attribute name)
        attr_content.extend_from_slice(&encode_ber_string(&attr_name));

        // vals (SET OF)
        let mut vals_content = Vec::new();
        if let Some(arr) = attr_values.as_array() {
            for val in arr {
                if let Some(s) = val.as_str() {
                    vals_content.extend_from_slice(&encode_ber_string(s));
                }
            }
        }

        let mut vals = vec![0x31]; // SET tag
        vals.extend_from_slice(&encode_ber_length(vals_content.len()));
        vals.extend_from_slice(&vals_content);
        attr_content.extend_from_slice(&vals);

        // Wrap in SEQUENCE
        let mut attr = vec![0x30];
        attr.extend_from_slice(&encode_ber_length(attr_content.len()));
        attr.extend_from_slice(&attr_content);
        attrs_content.extend_from_slice(&attr);
    }

    let mut attrs = vec![0x30]; // SEQUENCE tag
    attrs.extend_from_slice(&encode_ber_length(attrs_content.len()));
    attrs.extend_from_slice(&attrs_content);
    entry_content.extend_from_slice(&attrs);

    // Wrap in SearchResultEntry APPLICATION tag [4]
    let mut entry_msg = vec![0x64];
    entry_msg.extend_from_slice(&encode_ber_length(entry_content.len()));
    entry_msg.extend_from_slice(&entry_content);

    encode_ldap_message(msg_id, entry_msg)
}

fn encode_search_done(msg_id: i32, result_code: u8, diagnostic_message: &str) -> Vec<u8> {
    let mut result = Vec::new();

    // resultCode (ENUMERATED)
    result.push(0x0A);
    result.push(0x01);
    result.push(result_code);

    // matchedDN (empty)
    result.push(0x04);
    result.push(0x00);

    // diagnosticMessage
    result.extend_from_slice(&encode_ber_string(diagnostic_message));

    // Wrap in SearchResultDone APPLICATION tag [5]
    let mut search_msg = vec![0x65];
    search_msg.extend_from_slice(&encode_ber_length(result.len()));
    search_msg.extend_from_slice(&result);

    encode_ldap_message(msg_id, search_msg)
}

fn encode_ldap_result(msg_id: i32, app_tag: u8, result_code: u8, diagnostic_message: &str) -> Vec<u8> {
    let mut result = Vec::new();

    // resultCode (ENUMERATED)
    result.push(0x0A);
    result.push(0x01);
    result.push(result_code);

    // matchedDN (empty)
    result.push(0x04);
    result.push(0x00);

    // diagnosticMessage
    result.extend_from_slice(&encode_ber_string(diagnostic_message));

    // Wrap in APPLICATION tag
    let mut msg = vec![app_tag];
    msg.extend_from_slice(&encode_ber_length(result.len()));
    msg.extend_from_slice(&result);

    encode_ldap_message(msg_id, msg)
}
