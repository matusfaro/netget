//! XML-RPC server implementation
//!
//! This module implements an XML-RPC server over HTTP that allows LLM control
//! over RPC method execution, introspection, and response generation.
//!
//! XML-RPC specification: http://xmlrpc.com/spec.md
//!
//! The LLM controls:
//! - Method execution (custom methods defined by user prompt)
//! - Introspection responses (system.listMethods, system.methodHelp, etc.)
//! - Fault generation for errors
//! - Extensions (nil values, i8/64-bit integers, system.multicall)

pub mod actions;

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event as XmlEvent};
use quick_xml::{Reader, Writer};
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState as ServerConnectionState, ConnectionStatus, ProtocolConnectionInfo, ServerId};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

pub use actions::XmlRpcProtocol;

/// XML-RPC server that handles RPC method calls with LLM
pub struct XmlRpcServer;

#[cfg(feature = "xmlrpc")]
impl XmlRpcServer {
    /// Spawn XML-RPC server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("XML-RPC server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "[INFO] XML-RPC server listening on {}",
            local_addr
        ));

        let protocol = Arc::new(XmlRpcProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        debug!(
                            "XML-RPC connection {} from {}",
                            connection_id, remote_addr
                        );
                        let _ = status_tx.send(format!(
                            "→ XML-RPC connection {} from {}",
                            connection_id, remote_addr
                        ));

                        // Track connection in server state
                        let local_addr_conn = stream.local_addr().unwrap_or(listen_addr);
                        let now = std::time::Instant::now();

                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: local_addr_conn,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };

                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        // Spawn connection handler
                        tokio::spawn(async move {
                            let io = hyper_util::rt::TokioIo::new(stream);

                            // Create service function for this connection
                            let service = hyper::service::service_fn(|req| {
                                handle_xmlrpc_request(
                                    req,
                                    connection_id,
                                    server_id,
                                    remote_addr,
                                    llm_clone.clone(),
                                    state_clone.clone(),
                                    status_clone.clone(),
                                    protocol_clone.clone(),
                                )
                            });

                            // Serve HTTP/1 connection
                            if let Err(e) = hyper::server::conn::http1::Builder::new()
                                .serve_connection(io, service)
                                .await
                            {
                                error!(
                                    "XML-RPC connection {} error: {}",
                                    connection_id, e
                                );
                                let _ = status_clone.send(format!(
                                    "[ERROR] XML-RPC connection {} error: {}",
                                    connection_id, e
                                ));
                            }

                            // Mark connection as closed
                            state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_clone.send(format!(
                                "✗ XML-RPC connection {} closed",
                                connection_id
                            ));
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept XML-RPC connection: {}", e);
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to accept XML-RPC connection: {}",
                            e
                        ));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle a single XML-RPC request
#[cfg(feature = "xmlrpc")]
async fn handle_xmlrpc_request(
    req: Request<hyper::body::Incoming>,
    connection_id: ConnectionId,
    server_id: ServerId,
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<XmlRpcProtocol>,
) -> Result<Response<Full<Bytes>>> {
    // Collect request body
    let (parts, body) = req.into_parts();
    let body_bytes = body
        .collect()
        .await
        .context("Failed to read request body")?
        .to_bytes();

    let body_str = String::from_utf8_lossy(&body_bytes);

    debug!(
        "XML-RPC request from {}: {} {} ({} bytes)",
        remote_addr,
        parts.method,
        parts.uri,
        body_bytes.len()
    );
    let _ = status_tx.send(format!(
        "[DEBUG] XML-RPC request: {} {} ({} bytes)",
        parts.method,
        parts.uri,
        body_bytes.len()
    ));

    // Trace full request
    trace!("XML-RPC request body:\n{}", body_str);
    let _ = status_tx.send(format!(
        "[TRACE] XML-RPC request body:\r\n{}",
        body_str
    ));

    // Check if it's POST (XML-RPC requires POST)
    if parts.method != hyper::Method::POST {
        let fault_xml = generate_fault(-32600, "Invalid request: XML-RPC requires POST method");
        debug!(
            "XML-RPC error: invalid method {} (expected POST)",
            parts.method
        );
        let _ = status_tx.send(format!(
            "[DEBUG] XML-RPC error: invalid method {} (expected POST)",
            parts.method
        ));
        return Ok(Response::builder()
            .status(200)
            .header("Content-Type", "text/xml")
            .body(Full::new(Bytes::from(fault_xml)))
            .unwrap());
    }

    // Parse XML-RPC methodCall
    let method_call = match parse_method_call(&body_str) {
        Ok(call) => call,
        Err(e) => {
            console_error!(status_tx, "XML-RPC parse error: {}", e);
            let fault_xml = generate_fault(-32700, &format!("Parse error: {}", e));
            return Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/xml")
                .body(Full::new(Bytes::from(fault_xml)))
                .unwrap());
        }
    };

    debug!(
        "XML-RPC method call: {} with {} parameters",
        method_call.method_name,
        method_call.params.len()
    );
    let _ = status_tx.send(format!(
        "[DEBUG] XML-RPC method: {} ({} params)",
        method_call.method_name,
        method_call.params.len()
    ));

    // Create event for LLM
    let event = actions::create_method_call_event(&method_call);

    // Call LLM to get response
    let execution_result = match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &event,
        protocol.as_ref(),
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            console_error!(status_tx, "LLM error: {}", e);
            let fault_xml = generate_fault(-32603, &format!("Internal error: {}", e));
            return Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/xml")
                .body(Full::new(Bytes::from(fault_xml)))
                .unwrap());
        }
    };

    // Display messages from LLM
    for msg in execution_result.messages {
        let _ = status_tx.send(msg);
    }

    // Parse action result (should be XML response)
    let mut response_xml = String::new();
    for protocol_result in execution_result.protocol_results {
        if let ActionResult::Output(bytes) = protocol_result {
            response_xml = String::from_utf8_lossy(&bytes).to_string();
            break;
        }
    }

    // If no XML response was generated, return a fault
    if response_xml.is_empty() {
        error!("LLM did not generate XML-RPC response");
        let _ = status_tx.send("[ERROR] LLM did not generate XML-RPC response".to_string());
        response_xml = generate_fault(-32603, "Internal error: no response generated");
    }

    trace!("XML-RPC response:\n{}", response_xml);
    let _ = status_tx.send(format!("[TRACE] XML-RPC response:\r\n{}", response_xml));

    debug!(
        "→ XML-RPC {} → response ({} bytes)",
        method_call.method_name,
        response_xml.len()
    );
    let _ = status_tx.send(format!(
        "→ XML-RPC {} → {} bytes",
        method_call.method_name,
        response_xml.len()
    ));

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "text/xml")
        .body(Full::new(Bytes::from(response_xml)))
        .unwrap())
}

/// XML-RPC method call structure
#[derive(Debug, Clone)]
pub struct MethodCall {
    pub method_name: String,
    pub params: Vec<XmlRpcValue>,
}

/// XML-RPC value types
#[derive(Debug, Clone, PartialEq)]
pub enum XmlRpcValue {
    Int(i32),
    I8(i64),         // Extension: 64-bit integer
    Boolean(bool),
    String(String),
    Double(f64),
    DateTime(String), // ISO 8601 format
    Base64(Vec<u8>),
    Array(Vec<XmlRpcValue>),
    Struct(Vec<(String, XmlRpcValue)>), // key-value pairs
    Nil,             // Extension: null value
}

/// Parse XML-RPC methodCall from XML string
#[cfg(feature = "xmlrpc")]
fn parse_method_call(xml: &str) -> Result<MethodCall> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut method_name = String::new();
    let mut params = Vec::new();
    let mut buf = Vec::new();

    let mut in_method_name = false;
    let mut _in_params = false;
    let mut _in_param = false;
    let mut in_value = false;

    let mut value_stack: Vec<XmlRpcValue> = Vec::new();
    let mut array_stack: Vec<Vec<XmlRpcValue>> = Vec::new();
    let mut struct_stack: Vec<Vec<(String, XmlRpcValue)>> = Vec::new();
    let mut member_name: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) => {
                match e.name().as_ref() {
                    b"methodName" => in_method_name = true,
                    b"params" => _in_params = true,
                    b"param" => _in_param = true,
                    b"value" => in_value = true,
                    b"array" => {
                        array_stack.push(Vec::new());
                    }
                    b"struct" => {
                        struct_stack.push(Vec::new());
                    }
                    b"member" => {
                        // Struct member
                    }
                    b"name" if !struct_stack.is_empty() => {
                        // Read member name for struct
                        if let Ok(XmlEvent::Text(t)) = reader.read_event_into(&mut buf) {
                            member_name = Some(t.unescape()?.to_string());
                        }
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                match e.name().as_ref() {
                    b"methodName" => in_method_name = false,
                    b"params" => _in_params = false,
                    b"param" => {
                        _in_param = false;
                        if let Some(val) = value_stack.pop() {
                            params.push(val);
                        }
                    }
                    b"value" => in_value = false,
                    b"array" => {
                        if let Some(arr) = array_stack.pop() {
                            value_stack.push(XmlRpcValue::Array(arr));
                        }
                    }
                    b"data" => {
                        // Array data end
                        if let Some(arr) = array_stack.last_mut() {
                            if let Some(val) = value_stack.pop() {
                                arr.push(val);
                            }
                        }
                    }
                    b"struct" => {
                        if let Some(s) = struct_stack.pop() {
                            value_stack.push(XmlRpcValue::Struct(s));
                        }
                    }
                    b"member" => {
                        if let Some(s) = struct_stack.last_mut() {
                            if let (Some(name), Some(val)) = (member_name.take(), value_stack.pop()) {
                                s.push((name, val));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::Text(e)) => {
                let text = e.unescape()?.to_string();
                if in_method_name {
                    method_name = text;
                } else if in_value {
                    // Default to string
                    value_stack.push(XmlRpcValue::String(text));
                }
            }
            Ok(XmlEvent::Empty(ref e)) => {
                if in_value {
                    match e.name().as_ref() {
                        b"i4" | b"int" => {
                            // Read next text
                        }
                        b"boolean" => {}
                        b"string" => {}
                        b"double" => {}
                        b"nil" => {
                            value_stack.push(XmlRpcValue::Nil);
                        }
                        _ => {}
                    }
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML parse error: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(MethodCall {
        method_name,
        params,
    })
}

/// Generate XML-RPC fault response
#[cfg(feature = "xmlrpc")]
pub fn generate_fault(code: i32, message: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<methodResponse>
  <fault>
    <value>
      <struct>
        <member>
          <name>faultCode</name>
          <value><int>{}</int></value>
        </member>
        <member>
          <name>faultString</name>
          <value><string>{}</string></value>
        </member>
      </struct>
    </value>
  </fault>
</methodResponse>"#,
        code, message
    )
}

/// Generate XML-RPC success response with a value
#[cfg(feature = "xmlrpc")]
pub fn generate_success_response(value: &XmlRpcValue) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // XML declaration
    writer
        .write_event(XmlEvent::Decl(quick_xml::events::BytesDecl::new(
            "1.0", None, None,
        )))
        .unwrap();

    // methodResponse
    writer
        .write_event(XmlEvent::Start(BytesStart::new("methodResponse")))
        .unwrap();
    writer
        .write_event(XmlEvent::Start(BytesStart::new("params")))
        .unwrap();
    writer
        .write_event(XmlEvent::Start(BytesStart::new("param")))
        .unwrap();

    write_value(&mut writer, value);

    writer
        .write_event(XmlEvent::End(BytesEnd::new("param")))
        .unwrap();
    writer
        .write_event(XmlEvent::End(BytesEnd::new("params")))
        .unwrap();
    writer
        .write_event(XmlEvent::End(BytesEnd::new("methodResponse")))
        .unwrap();

    String::from_utf8(writer.into_inner().into_inner()).unwrap()
}

/// Write XML-RPC value to XML writer
#[cfg(feature = "xmlrpc")]
fn write_value(writer: &mut Writer<Cursor<Vec<u8>>>, value: &XmlRpcValue) {
    writer
        .write_event(XmlEvent::Start(BytesStart::new("value")))
        .unwrap();

    match value {
        XmlRpcValue::Int(i) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("int")))
                .unwrap();
            writer
                .write_event(XmlEvent::Text(BytesText::new(&i.to_string())))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("int")))
                .unwrap();
        }
        XmlRpcValue::I8(i) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("i8")))
                .unwrap();
            writer
                .write_event(XmlEvent::Text(BytesText::new(&i.to_string())))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("i8")))
                .unwrap();
        }
        XmlRpcValue::Boolean(b) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("boolean")))
                .unwrap();
            writer
                .write_event(XmlEvent::Text(BytesText::new(if *b { "1" } else { "0" })))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("boolean")))
                .unwrap();
        }
        XmlRpcValue::String(s) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("string")))
                .unwrap();
            writer
                .write_event(XmlEvent::Text(BytesText::new(s)))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("string")))
                .unwrap();
        }
        XmlRpcValue::Double(d) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("double")))
                .unwrap();
            writer
                .write_event(XmlEvent::Text(BytesText::new(&d.to_string())))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("double")))
                .unwrap();
        }
        XmlRpcValue::DateTime(dt) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("dateTime.iso8601")))
                .unwrap();
            writer
                .write_event(XmlEvent::Text(BytesText::new(dt)))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("dateTime.iso8601")))
                .unwrap();
        }
        XmlRpcValue::Base64(bytes) => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            writer
                .write_event(XmlEvent::Start(BytesStart::new("base64")))
                .unwrap();
            writer
                .write_event(XmlEvent::Text(BytesText::new(&encoded)))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("base64")))
                .unwrap();
        }
        XmlRpcValue::Array(arr) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("array")))
                .unwrap();
            writer
                .write_event(XmlEvent::Start(BytesStart::new("data")))
                .unwrap();
            for item in arr {
                write_value(writer, item);
            }
            writer
                .write_event(XmlEvent::End(BytesEnd::new("data")))
                .unwrap();
            writer
                .write_event(XmlEvent::End(BytesEnd::new("array")))
                .unwrap();
        }
        XmlRpcValue::Struct(members) => {
            writer
                .write_event(XmlEvent::Start(BytesStart::new("struct")))
                .unwrap();
            for (name, val) in members {
                writer
                    .write_event(XmlEvent::Start(BytesStart::new("member")))
                    .unwrap();
                writer
                    .write_event(XmlEvent::Start(BytesStart::new("name")))
                    .unwrap();
                writer
                    .write_event(XmlEvent::Text(BytesText::new(name)))
                    .unwrap();
                writer
                    .write_event(XmlEvent::End(BytesEnd::new("name")))
                    .unwrap();
                write_value(writer, val);
                writer
                    .write_event(XmlEvent::End(BytesEnd::new("member")))
                    .unwrap();
            }
            writer
                .write_event(XmlEvent::End(BytesEnd::new("struct")))
                .unwrap();
        }
        XmlRpcValue::Nil => {
            writer
                .write_event(XmlEvent::Empty(BytesStart::new("nil")))
                .unwrap();
        }
    }

    writer
        .write_event(XmlEvent::End(BytesEnd::new("value")))
        .unwrap();
}
