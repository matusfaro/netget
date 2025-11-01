//! VNC protocol actions for LLM integration

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::metadata::{ProtocolMetadata, DevelopmentState};
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::Value as JsonValue;
use tracing::debug;

/// VNC protocol event types
pub const VNC_AUTH_REQUEST_EVENT: &str = "vnc_auth_request";
pub const VNC_UPDATE_REQUEST_EVENT: &str = "vnc_framebuffer_update_request";
pub const VNC_KEY_EVENT: &str = "vnc_key_event";
pub const VNC_POINTER_EVENT: &str = "vnc_pointer_event";

/// VNC protocol implementation
#[derive(Clone)]
pub struct VncProtocol;

impl VncProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for VncProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Server for VncProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::vnc::VncServer;
            VncServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn protocol_name(&self) -> &'static str {
        "VNC"
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>VNC"
    }

    fn description(&self) -> &'static str {
        "VNC remote desktop server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a VNC server on port 5900 displaying a blue background"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["vnc", "rfb", "remote desktop", "framebuffer"]
    }

    fn metadata(&self) -> ProtocolMetadata {
        ProtocolMetadata::new(DevelopmentState::Alpha)
    }

    fn group_name(&self) -> &'static str {
        "Network Services"
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_framebuffer_update".to_string(),
                description: "Send a framebuffer update to a VNC client with display content".to_string(),
                parameters: vec![
                    Parameter {
                        name: "connection_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Connection ID of the VNC client".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "width".to_string(),
                        type_hint: "number".to_string(),
                        description: "Framebuffer width in pixels".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "height".to_string(),
                        type_hint: "number".to_string(),
                        description: "Framebuffer height in pixels".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "commands".to_string(),
                        type_hint: "array".to_string(),
                        description: "Display commands to render (DrawRectangle, DrawText, RenderAsciiArt, etc.)".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::from_str(r#"{"type": "send_framebuffer_update", "connection_id": "conn123", "width": 800, "height": 600, "commands": [{"SetBackground": {"color": {"r": 50, "g": 50, "b": 50, "a": 255}}}, {"DrawText": {"x": 100, "y": 100, "text": "Welcome to VNC", "font_size": 24, "color": {"r": 255, "g": 255, "b": 255, "a": 255}}}]}"#).unwrap(),
            },
            ActionDefinition {
                name: "disconnect_vnc_client".to_string(),
                description: "Disconnect a VNC client".to_string(),
                parameters: vec![
                    Parameter {
                        name: "connection_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Connection ID of the VNC client".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::from_str(r#"{"type": "disconnect_vnc_client", "connection_id": "conn123"}"#).unwrap(),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "vnc_auth_success".to_string(),
                description: "Allow VNC client to connect".to_string(),
                parameters: vec![
                    Parameter {
                        name: "username".to_string(),
                        type_hint: "string".to_string(),
                        description: "Optional username for this connection".to_string(),
                        required: false,
                    },
                ],
                example: serde_json::from_str(r#"{"type": "vnc_auth_success", "username": "guest"}"#).unwrap(),
            },
            ActionDefinition {
                name: "vnc_auth_deny".to_string(),
                description: "Deny VNC client connection".to_string(),
                parameters: vec![
                    Parameter {
                        name: "reason".to_string(),
                        type_hint: "string".to_string(),
                        description: "Reason for denying the connection".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::from_str(r#"{"type": "vnc_auth_deny", "reason": "Access denied"}"#).unwrap(),
            },
            ActionDefinition {
                name: "vnc_render_display".to_string(),
                description: "Render display content in response to update request".to_string(),
                parameters: vec![
                    Parameter {
                        name: "commands".to_string(),
                        type_hint: "array".to_string(),
                        description: "Display commands to render (DrawRectangle, DrawText, RenderAsciiArt, DrawWindow, DrawButton, etc.)".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::from_str(r#"{"type": "vnc_render_display", "commands": [{"RenderAsciiArt": {"text": "+----------+\n| Login:   |\n| User: __ |\n+----------+", "font_size": 16, "fg_color": {"r": 255, "g": 255, "b": 255, "a": 255}, "bg_color": {"r": 0, "g": 0, "b": 0, "a": 255}}}]}"#).unwrap(),
            },
        ]
    }

    fn execute_action(&self, action: JsonValue) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "vnc_auth_success" => {
                debug!("VNC auth success");
                // Return NoAction since authentication is handled by the protocol handler
                Ok(ActionResult::NoAction)
            }
            "vnc_auth_deny" => {
                let reason = action["reason"]
                    .as_str()
                    .unwrap_or("Access denied")
                    .to_string();
                debug!("VNC auth denied: {}", reason);
                // Return CloseConnection to deny the client
                Ok(ActionResult::CloseConnection)
            }
            "vnc_render_display" => {
                // VNC framebuffer updates are handled asynchronously by the server
                // This action just signals success to the LLM
                debug!("VNC render display command received");
                Ok(ActionResult::NoAction)
            }
            "send_framebuffer_update" => {
                // Async action - handled by spawning a task
                debug!("VNC send framebuffer update command received");
                Ok(ActionResult::NoAction)
            }
            "disconnect_vnc_client" => {
                debug!("VNC disconnect client");
                Ok(ActionResult::CloseConnection)
            }
            _ => Err(anyhow!("Unknown VNC action: {}", action_type)),
        }
    }
}
