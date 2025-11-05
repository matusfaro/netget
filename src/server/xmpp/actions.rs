//! XMPP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::server::connection::ConnectionId;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;
use tracing::debug;

/// XMPP client state for tracking JID and authentication
#[derive(Clone, Debug)]
pub struct XmppClientState {
    pub jid: Option<String>,        // Jabber ID (user@domain/resource)
    pub authenticated: bool,
    pub stream_id: Option<String>,
    pub resource: Option<String>,
}

impl XmppClientState {
    pub fn new() -> Self {
        Self {
            jid: None,
            authenticated: false,
            stream_id: None,
            resource: None,
        }
    }
}

/// XMPP protocol action handler
pub struct XmppProtocol {
    /// Map of active connections to their XMPP state
    clients: Arc<Mutex<HashMap<ConnectionId, XmppClientState>>>,
}

impl XmppProtocol {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a connection to the protocol handler
    pub async fn add_connection(&self, connection_id: ConnectionId) {
        self.clients
            .lock()
            .await
            .insert(connection_id, XmppClientState::new());
    }

    /// Remove a connection from the protocol handler
    pub async fn remove_connection(&self, connection_id: &ConnectionId) {
        self.clients.lock().await.remove(connection_id);
    }

    /// Set client JID
    pub async fn set_jid(&self, connection_id: ConnectionId, jid: String) {
        if let Some(client) = self.clients.lock().await.get_mut(&connection_id) {
            client.jid = Some(jid);
        }
    }

    /// Mark client as authenticated
    pub async fn set_authenticated(&self, connection_id: ConnectionId, authenticated: bool) {
        if let Some(client) = self.clients.lock().await.get_mut(&connection_id) {
            client.authenticated = authenticated;
        }
    }

    /// Get client state
    pub async fn get_client_state(&self, connection_id: &ConnectionId) -> Option<XmppClientState> {
        self.clients.lock().await.get(connection_id).cloned()
    }
}

impl Server for XmppProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::xmpp::XmppServer;
            XmppServer::spawn_with_llm_actions(
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
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "XMPP server domain name (e.g., 'localhost', 'example.com')".to_string(),
                required: false,
                example: serde_json::json!("localhost"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // XMPP could have async actions like broadcast_message in the future
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_stream_header_action(),
            send_stream_features_action(),
            send_message_action(),
            send_presence_action(),
            send_iq_result_action(),
            send_iq_error_action(),
            send_auth_success_action(),
            send_auth_failure_action(),
            send_raw_xml_action(),
            wait_for_more_action(),
            close_stream_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_stream_header" => self.execute_send_stream_header(action),
            "send_stream_features" => self.execute_send_stream_features(action),
            "send_message" => self.execute_send_message(action),
            "send_presence" => self.execute_send_presence(action),
            "send_iq_result" => self.execute_send_iq_result(action),
            "send_iq_error" => self.execute_send_iq_error(action),
            "send_auth_success" => self.execute_send_auth_success(action),
            "send_auth_failure" => self.execute_send_auth_failure(action),
            "send_raw_xml" => self.execute_send_raw_xml(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_stream" => self.execute_close_stream(action),
            _ => Err(anyhow::anyhow!("Unknown XMPP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "XMPP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_xmpp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>XMPP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["xmpp", "jabber", "messaging"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual XML stream parsing")
            .llm_control("All XMPP stanzas (message, presence, iq)")
            .e2e_testing("Manual XMPP client")
            .notes("No roster management, simplified authentication")
            .build()
    }

    fn description(&self) -> &'static str {
        "XMPP instant messaging server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an XMPP server for instant messaging"
    }

    fn group_name(&self) -> &'static str {
        "Application"
    }
}

// Action implementation methods
impl XmppProtocol {
    fn execute_send_stream_header(&self, action: serde_json::Value) -> Result<ActionResult> {
        let from = action
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost");

        let stream_id = action
            .get("stream_id")
            .and_then(|v| v.as_str())
            .unwrap_or("stream-id-123");

        let xml = format!(
            r#"<?xml version='1.0'?><stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' from='{}' id='{}' version='1.0'>"#,
            from, stream_id
        );

        debug!("XMPP sending stream header");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_stream_features(&self, action: serde_json::Value) -> Result<ActionResult> {
        let mechanisms = action
            .get("mechanisms")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| format!("<mechanism>{}</mechanism>", s))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_else(|| "<mechanism>PLAIN</mechanism>".to_string());

        let xml = format!(
            r#"<stream:features><mechanisms xmlns='urn:ietf:params:xml:ns:xmpp-sasl'>{}</mechanisms></stream:features>"#,
            mechanisms
        );

        debug!("XMPP sending stream features");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let from = action
            .get("from")
            .and_then(|v| v.as_str())
            .context("Missing 'from' parameter")?;

        let to = action
            .get("to")
            .and_then(|v| v.as_str())
            .context("Missing 'to' parameter")?;

        let body = action
            .get("body")
            .and_then(|v| v.as_str())
            .context("Missing 'body' parameter")?;

        let msg_type = action
            .get("message_type")
            .and_then(|v| v.as_str())
            .unwrap_or("chat");

        let xml = format!(
            r#"<message from='{}' to='{}' type='{}'><body>{}</body></message>"#,
            from, to, msg_type, body
        );

        debug!("XMPP sending message from {} to {}", from, to);
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_presence(&self, action: serde_json::Value) -> Result<ActionResult> {
        let from = action
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let presence_type = action
            .get("presence_type")
            .and_then(|v| v.as_str())
            .unwrap_or("available");

        let show = action
            .get("show")
            .and_then(|v| v.as_str());

        let status = action
            .get("status")
            .and_then(|v| v.as_str());

        let mut xml = if from.is_empty() {
            "<presence".to_string()
        } else {
            format!("<presence from='{}'", from)
        };

        if presence_type != "available" {
            xml.push_str(&format!(" type='{}'", presence_type));
        }
        xml.push('>');

        if let Some(s) = show {
            xml.push_str(&format!("<show>{}</show>", s));
        }
        if let Some(s) = status {
            xml.push_str(&format!("<status>{}</status>", s));
        }

        xml.push_str("</presence>");

        debug!("XMPP sending presence");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_iq_result(&self, action: serde_json::Value) -> Result<ActionResult> {
        let id = action
            .get("id")
            .and_then(|v| v.as_str())
            .context("Missing 'id' parameter")?;

        let to = action
            .get("to")
            .and_then(|v| v.as_str());

        let payload = action
            .get("payload")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let to_attr = to.map(|t| format!(" to='{}'", t)).unwrap_or_default();

        let xml = format!(
            r#"<iq type='result' id='{}'{}>{}</iq>"#,
            id, to_attr, payload
        );

        debug!("XMPP sending IQ result");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_iq_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let id = action
            .get("id")
            .and_then(|v| v.as_str())
            .context("Missing 'id' parameter")?;

        let error_type = action
            .get("error_type")
            .and_then(|v| v.as_str())
            .unwrap_or("cancel");

        let condition = action
            .get("condition")
            .and_then(|v| v.as_str())
            .unwrap_or("feature-not-implemented");

        let xml = format!(
            r#"<iq type='error' id='{}'><error type='{}'><{} xmlns='urn:ietf:params:xml:ns:xmpp-stanzas'/></error></iq>"#,
            id, error_type, condition
        );

        debug!("XMPP sending IQ error");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_auth_success(&self, _action: serde_json::Value) -> Result<ActionResult> {
        let xml = r#"<success xmlns='urn:ietf:params:xml:ns:xmpp-sasl'/>"#;
        debug!("XMPP sending auth success");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_auth_failure(&self, action: serde_json::Value) -> Result<ActionResult> {
        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("not-authorized");

        let xml = format!(
            r#"<failure xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><{}/></failure>"#,
            reason
        );

        debug!("XMPP sending auth failure");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_send_raw_xml(&self, action: serde_json::Value) -> Result<ActionResult> {
        let xml = action
            .get("xml")
            .and_then(|v| v.as_str())
            .context("Missing 'xml' parameter")?;

        debug!("XMPP sending raw XML");
        Ok(ActionResult::Output(xml.as_bytes().to_vec()))
    }

    fn execute_close_stream(&self, _action: serde_json::Value) -> Result<ActionResult> {
        let xml = r#"</stream:stream>"#;
        debug!("XMPP closing stream");
        Ok(ActionResult::Multiple(vec![
            ActionResult::Output(xml.as_bytes().to_vec()),
            ActionResult::CloseConnection,
        ]))
    }
}

// Action definitions
fn send_stream_header_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_stream_header".to_string(),
        description: "Send XMPP stream header to initiate XML stream".to_string(),
        parameters: vec![
            Parameter {
                name: "from".to_string(),
                type_hint: "string".to_string(),
                description: "Server domain name".to_string(),
                required: false,
            },
            Parameter {
                name: "stream_id".to_string(),
                type_hint: "string".to_string(),
                description: "Unique stream identifier".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_stream_header",
            "from": "localhost",
            "stream_id": "stream-123"
        }),
    }
}

fn send_stream_features_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_stream_features".to_string(),
        description: "Send stream features (authentication mechanisms, etc.)".to_string(),
        parameters: vec![
            Parameter {
                name: "mechanisms".to_string(),
                type_hint: "array".to_string(),
                description: "List of SASL mechanisms (e.g., ['PLAIN', 'SCRAM-SHA-1'])".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_stream_features",
            "mechanisms": ["PLAIN"]
        }),
    }
}

fn send_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_message".to_string(),
        description: "Send XMPP message stanza".to_string(),
        parameters: vec![
            Parameter {
                name: "from".to_string(),
                type_hint: "string".to_string(),
                description: "Sender JID".to_string(),
                required: true,
            },
            Parameter {
                name: "to".to_string(),
                type_hint: "string".to_string(),
                description: "Recipient JID".to_string(),
                required: true,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Message body text".to_string(),
                required: true,
            },
            Parameter {
                name: "message_type".to_string(),
                type_hint: "string".to_string(),
                description: "Message type: chat, groupchat, headline, normal".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_message",
            "from": "bot@localhost",
            "to": "user@localhost",
            "body": "Hello, world!",
            "message_type": "chat"
        }),
    }
}

fn send_presence_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_presence".to_string(),
        description: "Send XMPP presence stanza".to_string(),
        parameters: vec![
            Parameter {
                name: "from".to_string(),
                type_hint: "string".to_string(),
                description: "Sender JID".to_string(),
                required: false,
            },
            Parameter {
                name: "presence_type".to_string(),
                type_hint: "string".to_string(),
                description: "Presence type: available, unavailable, subscribe, etc.".to_string(),
                required: false,
            },
            Parameter {
                name: "show".to_string(),
                type_hint: "string".to_string(),
                description: "Availability: away, chat, dnd, xa".to_string(),
                required: false,
            },
            Parameter {
                name: "status".to_string(),
                type_hint: "string".to_string(),
                description: "Status message".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_presence",
            "from": "user@localhost/resource",
            "show": "chat",
            "status": "Available for chat"
        }),
    }
}

fn send_iq_result_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_iq_result".to_string(),
        description: "Send IQ result stanza".to_string(),
        parameters: vec![
            Parameter {
                name: "id".to_string(),
                type_hint: "string".to_string(),
                description: "IQ ID (must match request)".to_string(),
                required: true,
            },
            Parameter {
                name: "to".to_string(),
                type_hint: "string".to_string(),
                description: "Recipient JID".to_string(),
                required: false,
            },
            Parameter {
                name: "payload".to_string(),
                type_hint: "string".to_string(),
                description: "Optional XML payload".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_iq_result",
            "id": "iq-123",
            "to": "user@localhost",
            "payload": "<query xmlns='jabber:iq:roster'/>"
        }),
    }
}

fn send_iq_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_iq_error".to_string(),
        description: "Send IQ error stanza".to_string(),
        parameters: vec![
            Parameter {
                name: "id".to_string(),
                type_hint: "string".to_string(),
                description: "IQ ID (must match request)".to_string(),
                required: true,
            },
            Parameter {
                name: "error_type".to_string(),
                type_hint: "string".to_string(),
                description: "Error type: cancel, continue, modify, auth, wait".to_string(),
                required: false,
            },
            Parameter {
                name: "condition".to_string(),
                type_hint: "string".to_string(),
                description: "Error condition".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_iq_error",
            "id": "iq-123",
            "error_type": "cancel",
            "condition": "feature-not-implemented"
        }),
    }
}

fn send_auth_success_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_auth_success".to_string(),
        description: "Send SASL authentication success".to_string(),
        parameters: vec![],
        example: json!({
            "type": "send_auth_success"
        }),
    }
}

fn send_auth_failure_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_auth_failure".to_string(),
        description: "Send SASL authentication failure".to_string(),
        parameters: vec![
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Failure reason".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_auth_failure",
            "reason": "not-authorized"
        }),
    }
}

fn send_raw_xml_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_raw_xml".to_string(),
        description: "Send raw XML data (for custom stanzas)".to_string(),
        parameters: vec![
            Parameter {
                name: "xml".to_string(),
                type_hint: "string".to_string(),
                description: "Raw XML string to send".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_raw_xml",
            "xml": "<custom xmlns='example:custom'/>"
        }),
    }
}

fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data before responding (accumulate in buffer)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
    }
}

fn close_stream_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_stream".to_string(),
        description: "Close XMPP stream and connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_stream"
        }),
    }
}

// Event types
pub static XMPP_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "xmpp_data_received",
        "XML data received from XMPP client"
    )
    .with_parameters(vec![
        Parameter {
            name: "xml_data".to_string(),
            type_hint: "string".to_string(),
            description: "Raw XML data received".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        send_stream_header_action(),
        send_stream_features_action(),
        send_message_action(),
        send_presence_action(),
        send_iq_result_action(),
        send_iq_error_action(),
        send_auth_success_action(),
        send_auth_failure_action(),
        send_raw_xml_action(),
        wait_for_more_action(),
        close_stream_action(),
    ])
});

pub fn get_xmpp_event_types() -> Vec<EventType> {
    vec![
        XMPP_DATA_RECEIVED_EVENT.clone(),
    ]
}
