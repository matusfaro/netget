//! SMTP client implementation using lettre library
pub mod actions;

pub use actions::SmtpClientProtocol;

use anyhow::{Context, Result};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{SmtpTransport, Transport};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::smtp::actions::SMTP_CLIENT_CONNECTED_EVENT;

/// SMTP client that sends emails via SMTP servers
pub struct SmtpClient;

impl SmtpClient {
    /// Connect to an SMTP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("SMTP client {} initializing connection to {}", client_id, remote_addr);

        // Parse server address (format: hostname:port or just hostname)
        let smtp_server = if remote_addr.contains(':') {
            remote_addr.split(':').next().unwrap().to_string()
        } else {
            remote_addr.clone()
        };

        // Store connection info in protocol data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "smtp_server".to_string(),
                serde_json::json!(smtp_server),
            );
            client.set_protocol_field(
                "remote_addr".to_string(),
                serde_json::json!(remote_addr),
            );
        }).await;

        // Update status to connected
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] SMTP client {} ready for {}", client_id, remote_addr);
        console_info!(status_tx, "__UPDATE_UI__");

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::smtp::actions::SmtpClientProtocol::new());
            let event = Event::new(
                &SMTP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "smtp_server": smtp_server,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions: _, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for SMTP client {}: {}", client_id, e);
                }
            }
        }

        // Spawn monitoring task
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("SMTP client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (SMTP is request-based, not persistent)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Send an email via SMTP
    #[allow(clippy::too_many_arguments)]
    pub async fn send_email(
        client_id: ClientId,
        from: String,
        to: Vec<String>,
        subject: String,
        body: String,
        username: Option<String>,
        password: Option<String>,
        use_tls: bool,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get SMTP server from client
        let smtp_server = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("smtp_server")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No SMTP server found")?;

        info!("SMTP client {} sending email to {:?}", client_id, to);

        // Clone subject for later use in event
        let subject_clone = subject.clone();

        // Build email message
        use lettre::message::Message;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
        let mut message_builder = Message::builder()
            .from(from.parse().context("Invalid 'from' address")?)
            .subject(subject);

        // Add recipients
        for recipient in &to {
            message_builder = message_builder.to(recipient.parse().context("Invalid 'to' address")?);
        }

        let email = message_builder
            .body(body)
            .context("Failed to build email message")?;

        // Build SMTP transport
        let mut transport_builder = SmtpTransport::relay(&smtp_server)
            .context("Failed to create SMTP transport")?;

        // Add credentials if provided
        if let (Some(user), Some(pass)) = (username.clone(), password.clone()) {
            let credentials = Credentials::new(user, pass);
            transport_builder = transport_builder.credentials(credentials);
        }

        // Configure TLS
        if use_tls {
            // STARTTLS
            let tls_parameters = TlsParameters::builder(smtp_server.clone())
                .dangerous_accept_invalid_certs(false)
                .build()
                .context("Failed to build TLS parameters")?;
            transport_builder = transport_builder.tls(Tls::Required(tls_parameters));
        } else {
            transport_builder = transport_builder.tls(Tls::None);
        }

        let mailer = transport_builder.build();

        // Send email (blocking operation, spawn blocking task)
        let result = tokio::task::spawn_blocking(move || {
            mailer.send(&email)
        }).await.context("Task join error")??;

        console_info!(status_tx, "[CLIENT] SMTP email sent successfully");

        // Call LLM with sent event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::smtp::actions::SmtpClientProtocol::new());
            let event = Event::new(
                &crate::client::smtp::actions::SMTP_CLIENT_EMAIL_SENT_EVENT,
                serde_json::json!({
                    "to": to,
                    "subject": subject_clone,
                    "success": true,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions: _, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for SMTP client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }
}
