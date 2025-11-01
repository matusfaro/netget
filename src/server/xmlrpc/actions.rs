//! XML-RPC protocol actions implementation
//!
//! This module implements the action system for XML-RPC.
//! The LLM controls all XML-RPC responses through these actions, including:
//! - Method execution responses (success values, faults)
//! - Introspection (system.listMethods, system.methodHelp, system.methodSignature)
//! - Extensions (nil values, i8/64-bit integers, system.multicall)

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::{Event, EventType, metadata::ProtocolMetadata, metadata::DevelopmentState};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

use super::{MethodCall, XmlRpcValue, generate_fault, generate_success_response};

/// XML-RPC protocol action handler
pub struct XmlRpcProtocol;

impl XmlRpcProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_success_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let value_type = action
            .get("value_type")
            .and_then(|v| v.as_str())
            .context("Missing 'value_type' parameter")?;

        let value = action
            .get("value")
            .context("Missing 'value' parameter")?;

        // Convert JSON value to XmlRpcValue
        let xmlrpc_value = match value_type {
            "int" | "i4" => {
                let i = value.as_i64().context("Invalid integer value")? as i32;
                XmlRpcValue::Int(i)
            }
            "i8" => {
                let i = value.as_i64().context("Invalid i8 value")?;
                XmlRpcValue::I8(i)
            }
            "boolean" | "bool" => {
                let b = value.as_bool().context("Invalid boolean value")?;
                XmlRpcValue::Boolean(b)
            }
            "string" => {
                let s = value.as_str().context("Invalid string value")?;
                XmlRpcValue::String(s.to_string())
            }
            "double" => {
                let d = value.as_f64().context("Invalid double value")?;
                XmlRpcValue::Double(d)
            }
            "array" => {
                let arr = value.as_array().context("Invalid array value")?;
                let items: Vec<XmlRpcValue> = arr
                    .iter()
                    .map(|v| self.json_to_xmlrpc_value(v))
                    .collect::<Result<Vec<_>>>()?;
                XmlRpcValue::Array(items)
            }
            "struct" => {
                let obj = value.as_object().context("Invalid struct value")?;
                let members: Vec<(String, XmlRpcValue)> = obj
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), self.json_to_xmlrpc_value(v)?)))
                    .collect::<Result<Vec<_>>>()?;
                XmlRpcValue::Struct(members)
            }
            "nil" | "null" => XmlRpcValue::Nil,
            _ => return Err(anyhow::anyhow!("Unknown value_type: {}", value_type)),
        };

        let xml = generate_success_response(&xmlrpc_value);

        debug!("XML-RPC success response generated ({} bytes)", xml.len());
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_fault_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action
            .get("fault_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(-32603) as i32;

        let message = action
            .get("fault_string")
            .and_then(|v| v.as_str())
            .context("Missing 'fault_string' parameter")?;

        let xml = generate_fault(code, message);

        debug!("XML-RPC fault response: {} - {}", code, message);
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_list_methods_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let methods = action
            .get("methods")
            .and_then(|v| v.as_array())
            .context("Missing 'methods' parameter")?;

        let method_list: Vec<XmlRpcValue> = methods
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| XmlRpcValue::String(s.to_string()))
            .collect();

        let response_value = XmlRpcValue::Array(method_list);
        let xml = generate_success_response(&response_value);

        debug!("XML-RPC system.listMethods response ({} methods)", methods.len());
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_method_help_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let help_text = action
            .get("help_text")
            .and_then(|v| v.as_str())
            .context("Missing 'help_text' parameter")?;

        let response_value = XmlRpcValue::String(help_text.to_string());
        let xml = generate_success_response(&response_value);

        debug!("XML-RPC system.methodHelp response");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_method_signature_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let signatures = action
            .get("signatures")
            .and_then(|v| v.as_array())
            .context("Missing 'signatures' parameter")?;

        // Convert signatures array to XML-RPC array of arrays
        let sig_list: Vec<XmlRpcValue> = signatures
            .iter()
            .filter_map(|v| v.as_array())
            .map(|arr| {
                let types: Vec<XmlRpcValue> = arr
                    .iter()
                    .filter_map(|t| t.as_str())
                    .map(|s| XmlRpcValue::String(s.to_string()))
                    .collect();
                XmlRpcValue::Array(types)
            })
            .collect();

        let response_value = XmlRpcValue::Array(sig_list);
        let xml = generate_success_response(&response_value);

        debug!("XML-RPC system.methodSignature response");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    /// Helper: Convert JSON value to XmlRpcValue (auto-detect type)
    fn json_to_xmlrpc_value(&self, value: &serde_json::Value) -> Result<XmlRpcValue> {
        match value {
            serde_json::Value::Null => Ok(XmlRpcValue::Nil),
            serde_json::Value::Bool(b) => Ok(XmlRpcValue::Boolean(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                        Ok(XmlRpcValue::Int(i as i32))
                    } else {
                        Ok(XmlRpcValue::I8(i))
                    }
                } else if let Some(f) = n.as_f64() {
                    Ok(XmlRpcValue::Double(f))
                } else {
                    Err(anyhow::anyhow!("Invalid number"))
                }
            }
            serde_json::Value::String(s) => Ok(XmlRpcValue::String(s.clone())),
            serde_json::Value::Array(arr) => {
                let items: Vec<XmlRpcValue> = arr
                    .iter()
                    .map(|v| self.json_to_xmlrpc_value(v))
                    .collect::<Result<Vec<_>>>()?;
                Ok(XmlRpcValue::Array(items))
            }
            serde_json::Value::Object(obj) => {
                let members: Vec<(String, XmlRpcValue)> = obj
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), self.json_to_xmlrpc_value(v)?)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(XmlRpcValue::Struct(members))
            }
        }
    }
}

impl Server for XmlRpcProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::xmlrpc::XmlRpcServer;
            XmlRpcServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // XML-RPC is purely request-response, no async actions needed
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            success_response_action(),
            fault_response_action(),
            list_methods_response_action(),
            method_help_response_action(),
            method_signature_response_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field")?;

        match action_type {
            "xmlrpc_success_response" => self.execute_success_response(action),
            "xmlrpc_fault_response" => self.execute_fault_response(action),
            "xmlrpc_list_methods_response" => self.execute_list_methods_response(action),
            "xmlrpc_method_help_response" => self.execute_method_help_response(action),
            "xmlrpc_method_signature_response" => self.execute_method_signature_response(action),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "XML-RPC"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![XMLRPC_METHOD_CALL_EVENT.clone()]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>XML-RPC"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["xmlrpc", "xml-rpc", "xml rpc"]
    }

    fn metadata(&self) -> ProtocolMetadata {
        ProtocolMetadata::new(DevelopmentState::Beta)
    }

    fn description(&self) -> &'static str {
        "XML-RPC server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an XML-RPC server on port 8080 with methods add(a,b) and greet(name)"
    }

    fn group_name(&self) -> &'static str {
        "AI & API Protocols"
    }
}

// Action definitions

fn success_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "xmlrpc_success_response".to_string(),
        description: "Send XML-RPC success response with a value".to_string(),
        parameters: vec![
            Parameter {
                name: "value_type".to_string(),
                type_hint: "string".to_string(),
                description: "Type of the return value (int, i8, boolean, string, double, array, struct, nil)".to_string(),
                required: true,
            },
            Parameter {
                name: "value".to_string(),
                type_hint: "any".to_string(),
                description: "The actual value to return (type must match value_type)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "xmlrpc_success_response",
            "value_type": "int",
            "value": 42
        }),
    }
}

fn fault_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "xmlrpc_fault_response".to_string(),
        description: "Send XML-RPC fault/error response".to_string(),
        parameters: vec![
            Parameter {
                name: "fault_code".to_string(),
                type_hint: "number".to_string(),
                description: "Fault code (standard codes: -32700 parse error, -32600 invalid request, -32601 method not found, -32602 invalid params, -32603 internal error)".to_string(),
                required: true,
            },
            Parameter {
                name: "fault_string".to_string(),
                type_hint: "string".to_string(),
                description: "Human-readable error message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "xmlrpc_fault_response",
            "fault_code": -32601,
            "fault_string": "Method not found"
        }),
    }
}

fn list_methods_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "xmlrpc_list_methods_response".to_string(),
        description: "Respond to system.listMethods introspection with array of method names".to_string(),
        parameters: vec![Parameter {
            name: "methods".to_string(),
            type_hint: "array".to_string(),
            description: "Array of available method names (strings)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "xmlrpc_list_methods_response",
            "methods": ["add", "subtract", "system.listMethods", "system.methodHelp"]
        }),
    }
}

fn method_help_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "xmlrpc_method_help_response".to_string(),
        description: "Respond to system.methodHelp introspection with documentation string".to_string(),
        parameters: vec![Parameter {
            name: "help_text".to_string(),
            type_hint: "string".to_string(),
            description: "Documentation/help text for the requested method".to_string(),
            required: true,
        }],
        example: json!({
            "type": "xmlrpc_method_help_response",
            "help_text": "add(a, b) - Returns the sum of two numbers"
        }),
    }
}

fn method_signature_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "xmlrpc_method_signature_response".to_string(),
        description: "Respond to system.methodSignature introspection with array of signatures".to_string(),
        parameters: vec![Parameter {
            name: "signatures".to_string(),
            type_hint: "array".to_string(),
            description: "Array of signature arrays. Each signature is [return_type, param1_type, param2_type, ...]. Multiple signatures indicate overloads.".to_string(),
            required: true,
        }],
        example: json!({
            "type": "xmlrpc_method_signature_response",
            "signatures": [["int", "int", "int"]]
        }),
    }
}

// Event type definitions

/// XML-RPC method call event
pub static XMLRPC_METHOD_CALL_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "xmlrpc_method_call",
        "XML-RPC method call received from client",
    )
    .with_parameters(vec![
        Parameter {
            name: "method_name".to_string(),
            type_hint: "string".to_string(),
            description: "Name of the RPC method being called".to_string(),
            required: true,
        },
        Parameter {
            name: "params".to_string(),
            type_hint: "array".to_string(),
            description: "Array of parameter values (can be integers, strings, booleans, arrays, structs, etc.)".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        success_response_action(),
        fault_response_action(),
        list_methods_response_action(),
        method_help_response_action(),
        method_signature_response_action(),
    ])
});

/// Create event from method call
pub fn create_method_call_event(method_call: &MethodCall) -> Event {
    // Convert XmlRpcValue params to JSON
    let params_json: Vec<serde_json::Value> = method_call
        .params
        .iter()
        .map(xmlrpc_value_to_json)
        .collect();

    Event::new(&XMLRPC_METHOD_CALL_EVENT, json!({
        "method_name": method_call.method_name,
        "params": params_json,
    }))
}

/// Convert XmlRpcValue to JSON for LLM
fn xmlrpc_value_to_json(value: &XmlRpcValue) -> serde_json::Value {
    match value {
        XmlRpcValue::Int(i) => json!(i),
        XmlRpcValue::I8(i) => json!(i),
        XmlRpcValue::Boolean(b) => json!(b),
        XmlRpcValue::String(s) => json!(s),
        XmlRpcValue::Double(d) => json!(d),
        XmlRpcValue::DateTime(dt) => json!(dt),
        XmlRpcValue::Base64(bytes) => {
            use base64::Engine;
            json!(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
        XmlRpcValue::Array(arr) => {
            let items: Vec<serde_json::Value> = arr.iter().map(xmlrpc_value_to_json).collect();
            json!(items)
        }
        XmlRpcValue::Struct(members) => {
            let obj: serde_json::Map<String, serde_json::Value> = members
                .iter()
                .map(|(k, v)| (k.clone(), xmlrpc_value_to_json(v)))
                .collect();
            json!(obj)
        }
        XmlRpcValue::Nil => json!(null),
    }
}
