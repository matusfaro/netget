//! Example external protocol plugin for NetGet
//!
//! This demonstrates how to create a protocol implementation in an external crate
//! without modifying NetGet's core code.

use anyhow::Result;
use netget::llm::actions::{ActionDefinition, Parameter, Server};
use netget::llm::actions::protocol_trait::ActionResult;
use netget::protocol::metadata::{ProtocolMetadata, ProtocolState};
use netget::protocol::SpawnContext;
use netget::state::app_state::AppState;
use serde_json::Value as JsonValue;
use std::net::SocketAddr;
use std::pin::Pin;
use std::future::Future;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, info};

/// Simple Echo protocol that echoes back all received data
#[derive(Clone)]
pub struct EchoProtocol;

impl EchoProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EchoProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Server for EchoProtocol {
    fn spawn(
        &self,
        ctx: SpawnContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            let listener = TcpListener::bind(ctx.listen_addr).await?;
            let local_addr = listener.local_addr()?;

            info!("[ECHO] Server listening on {}", local_addr);

            tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((mut stream, peer_addr)) => {
                            debug!("[ECHO] Connection from {}", peer_addr);

                            tokio::spawn(async move {
                                let mut buf = vec![0u8; 8192];
                                loop {
                                    match stream.read(&mut buf).await {
                                        Ok(0) => {
                                            debug!("[ECHO] Connection closed by {}", peer_addr);
                                            break;
                                        }
                                        Ok(n) => {
                                            debug!("[ECHO] Received {} bytes from {}", n, peer_addr);

                                            // Echo back the data
                                            if let Err(e) = stream.write_all(&buf[..n]).await {
                                                debug!("[ECHO] Write error: {}", e);
                                                break;
                                            }

                                            debug!("[ECHO] Echoed {} bytes to {}", n, peer_addr);
                                        }
                                        Err(e) => {
                                            debug!("[ECHO] Read error: {}", e);
                                            break;
                                        }
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            debug!("[ECHO] Accept error: {}", e);
                        }
                    }
                }
            });

            Ok(local_addr)
        })
    }

    fn protocol_name(&self) -> &'static str {
        "Echo"
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>ECHO"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["echo", "echo protocol"]
    }

    fn metadata(&self) -> ProtocolMetadata {
        ProtocolMetadata::with_notes(
            ProtocolState::Beta,
            "Simple echo server that returns all received data"
        )
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]  // No async actions for this simple protocol
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_echo_data".to_string(),
                description: "Send data back to the client".to_string(),
                parameters: vec![
                    Parameter {
                        name: "data".to_string(),
                        type_hint: "string".to_string(),
                        description: "Data to echo back".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({
                    "type": "send_echo_data",
                    "data": "Hello, World!"
                }),
            },
        ]
    }

    fn execute_action(&self, action: JsonValue) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing action type"))?;

        match action_type {
            "send_echo_data" => {
                let data = action["data"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing data parameter"))?;

                debug!("[ECHO] Sending data: {}", data);

                Ok(ActionResult::Output(data.as_bytes().to_vec()))
            }
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_name() {
        let protocol = EchoProtocol::new();
        assert_eq!(protocol.protocol_name(), "Echo");
    }

    #[test]
    fn test_stack_name() {
        let protocol = EchoProtocol::new();
        assert_eq!(protocol.stack_name(), "ETH>IP>TCP>ECHO");
    }

    #[test]
    fn test_keywords() {
        let protocol = EchoProtocol::new();
        assert!(protocol.keywords().contains(&"echo"));
    }
}
