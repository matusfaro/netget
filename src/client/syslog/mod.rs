//! Syslog client implementation
pub mod actions;

pub use actions::SyslogClientProtocol;

use crate::protocol::StartupParams;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

use crate::client::syslog::actions::SYSLOG_CLIENT_CONNECTED_EVENT;

/// Syslog facilities (RFC 5424)
fn facility_to_number(facility: &str) -> Result<u8> {
    match facility.to_lowercase().as_str() {
        "kern" | "kernel" => Ok(0),
        "user" => Ok(1),
        "mail" => Ok(2),
        "daemon" => Ok(3),
        "auth" => Ok(4),
        "syslog" => Ok(5),
        "lpr" => Ok(6),
        "news" => Ok(7),
        "uucp" => Ok(8),
        "cron" => Ok(9),
        "authpriv" => Ok(10),
        "ftp" => Ok(11),
        "ntp" => Ok(12),
        "security" => Ok(13),
        "console" => Ok(14),
        "solaris-cron" => Ok(15),
        "local0" => Ok(16),
        "local1" => Ok(17),
        "local2" => Ok(18),
        "local3" => Ok(19),
        "local4" => Ok(20),
        "local5" => Ok(21),
        "local6" => Ok(22),
        "local7" => Ok(23),
        _ => Err(anyhow!("Unknown syslog facility: {}", facility)),
    }
}

/// Syslog severities (RFC 5424)
fn severity_to_number(severity: &str) -> Result<u8> {
    match severity.to_lowercase().as_str() {
        "emerg" | "emergency" | "panic" => Ok(0),
        "alert" => Ok(1),
        "crit" | "critical" => Ok(2),
        "err" | "error" => Ok(3),
        "warn" | "warning" => Ok(4),
        "notice" => Ok(5),
        "info" | "informational" => Ok(6),
        "debug" => Ok(7),
        _ => Err(anyhow!("Unknown syslog severity: {}", severity)),
    }
}

/// Format a syslog message according to RFC 5424
fn format_syslog_message(
    facility: &str,
    severity: &str,
    message: &str,
    hostname: &str,
    app_name: &str,
    proc_id: &str,
    msg_id: &str,
) -> Result<String> {
    let facility_num = facility_to_number(facility)?;
    let severity_num = severity_to_number(severity)?;
    let priority = (facility_num * 8) + severity_num;

    // RFC 5424 format:
    // <PRI>VERSION TIMESTAMP HOSTNAME APP-NAME PROCID MSGID STRUCTURED-DATA MSG
    let timestamp = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let structured_data = "-"; // No structured data for now

    let formatted = format!(
        "<{}>1 {} {} {} {} {} {} {}",
        priority, timestamp, hostname, app_name, proc_id, msg_id, structured_data, message
    );

    Ok(formatted)
}

/// Transport protocol for syslog
enum SyslogTransport {
    Tcp(Arc<Mutex<TcpStream>>),
    Udp(Arc<UdpSocket>, SocketAddr),
}

/// Syslog client that connects to a remote syslog server
pub struct SyslogClient;

impl SyslogClient {
    /// Connect to a syslog server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse protocol from startup params (default to UDP)
        let protocol = startup_params
            .as_ref()
            .and_then(|params| params.get_optional_string("protocol"))
            .unwrap_or_else(|| "udp".to_string())
            .to_lowercase();

        let transport = match protocol.as_str() {
            "tcp" => {
                info!(
                    "Syslog client {} connecting via TCP to {}",
                    client_id, remote_addr
                );
                let stream = TcpStream::connect(&remote_addr).await.context(format!(
                    "Failed to connect to syslog server at {}",
                    remote_addr
                ))?;

                let local_addr = stream.local_addr()?;
                let remote_sock_addr = stream.peer_addr()?;

                info!(
                    "Syslog client {} connected via TCP to {} (local: {})",
                    client_id, remote_sock_addr, local_addr
                );

                SyslogTransport::Tcp(Arc::new(Mutex::new(stream)))
            }
            "udp" => {
                info!("Syslog client {} using UDP to {}", client_id, remote_addr);

                // Parse remote address
                let remote_sock_addr: SocketAddr = remote_addr
                    .parse()
                    .context(format!("Invalid address: {}", remote_addr))?;

                // Bind to local port (ephemeral)
                let local_bind = if remote_sock_addr.is_ipv6() {
                    "[::]:0"
                } else {
                    "0.0.0.0:0"
                };

                let socket = UdpSocket::bind(local_bind)
                    .await
                    .context("Failed to bind UDP socket")?;

                let local_addr = socket.local_addr()?;

                info!(
                    "Syslog client {} bound to UDP {} for remote {}",
                    client_id, local_addr, remote_sock_addr
                );

                SyslogTransport::Udp(Arc::new(socket), remote_sock_addr)
            }
            _ => {
                return Err(anyhow!(
                    "Invalid protocol: {}. Must be 'tcp' or 'udp'",
                    protocol
                ));
            }
        };

        let local_addr = match &transport {
            SyslogTransport::Tcp(stream) => stream.lock().await.local_addr()?,
            SyslogTransport::Udp(socket, _) => socket.local_addr()?,
        };

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] Syslog client {} connected via {}",
            client_id,
            protocol.to_uppercase()
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol_impl = Arc::new(SyslogClientProtocol::new());
            let event = Event::new(
                &SYSLOG_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                    "protocol": protocol,
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                "",
                Some(&event),
                protocol_impl.as_ref(),
                &status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates: _,
                }) => {
                    // Execute actions
                    for action in actions {
                        if let Err(e) = Self::execute_syslog_action(
                            action,
                            &transport,
                            client_id,
                            &status_tx,
                            protocol_impl.as_ref(),
                        )
                        .await
                        {
                            error!("Error executing syslog action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for syslog client {}: {}", client_id, e);
                }
            }
        }

        // Store transport for async actions
        // Note: In a full implementation, we'd store this in app_state for async action execution
        // For now, the client is fire-and-forget for UDP, or connection-based for TCP

        Ok(local_addr)
    }

    /// Execute a syslog action
    async fn execute_syslog_action(
        action: Value,
        transport: &SyslogTransport,
        client_id: ClientId,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &dyn Client,
    ) -> Result<()> {
        match protocol.execute_action(action)? {
            ClientActionResult::Custom { name, data } if name == "send_syslog_message" => {
                let facility = data["facility"].as_str().context("Missing facility")?;
                let severity = data["severity"].as_str().context("Missing severity")?;
                let message = data["message"].as_str().context("Missing message")?;
                let hostname = data["hostname"].as_str().unwrap_or("netget");
                let app_name = data["app_name"].as_str().unwrap_or("netget");
                let proc_id = data["proc_id"].as_str().unwrap_or("-");
                let msg_id = data["msg_id"].as_str().unwrap_or("-");

                let formatted_message = format_syslog_message(
                    facility, severity, message, hostname, app_name, proc_id, msg_id,
                )?;

                trace!("Syslog client {} sending: {}", client_id, formatted_message);

                match transport {
                    SyslogTransport::Tcp(stream) => {
                        // TCP: message terminated with newline
                        let mut stream = stream.lock().await;
                        stream.write_all(formatted_message.as_bytes()).await?;
                        stream.write_all(b"\n").await?;
                        stream.flush().await?;
                    }
                    SyslogTransport::Udp(socket, remote_addr) => {
                        // UDP: send as datagram
                        socket
                            .send_to(formatted_message.as_bytes(), remote_addr)
                            .await?;
                    }
                }

                info!(
                    "Syslog client {} sent message: facility={}, severity={}, msg={}",
                    client_id, facility, severity, message
                );
                let _ = status_tx.send(format!(
                    "[CLIENT] Syslog {} sent: [{}:{}] {}",
                    client_id, facility, severity, message
                ));

                Ok(())
            }
            ClientActionResult::Disconnect => {
                info!("Syslog client {} disconnecting", client_id);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
