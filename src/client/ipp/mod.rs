//! IPP (Internet Printing Protocol) client implementation

pub mod actions;

pub use actions::IppClientProtocol;

use anyhow::{Context, Result};
use ipp::prelude::*;
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
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// IPP client that connects to remote IPP print servers
pub struct IppClient;

impl IppClient {
    /// Connect to an IPP print server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("IPP client {} initialized for {}", client_id, remote_addr);

        // Parse the remote address to construct IPP URI
        // IPP typically uses http://host:631/printers/printer-name format
        let uri_str = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
            remote_addr.clone()
        } else if remote_addr.starts_with("ipp://") {
            // Convert ipp:// to http://
            remote_addr.replace("ipp://", "http://")
        } else {
            // Default to http:// with IPP default port 631
            format!("http://{}", remote_addr)
        };

        // Store URI in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "ipp_uri".to_string(),
                serde_json::json!(uri_str),
            );
            client.set_protocol_field(
                "ipp_client".to_string(),
                serde_json::json!("initialized"),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] IPP client {} ready for {}", client_id, uri_str);
        console_info!(status_tx, "__UPDATE_UI__");

        // Spawn background task to monitor for client disconnection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("IPP client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (IPP is HTTP-based, connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Send Get-Printer-Attributes operation
    pub async fn get_printer_attributes(
        client_id: ClientId,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let uri_str = Self::get_uri(&app_state, client_id).await?;
        let uri: Uri = uri_str.parse().context("Invalid IPP URI")?;

        info!("IPP client {} sending Get-Printer-Attributes to {}", client_id, uri);

        let operation = IppOperationBuilder::get_printer_attributes(uri.clone()).build();
        let client = AsyncIppClient::new(uri);

        match client.send(operation).await {
            Ok(response) => {
                let status_code = response.header().status_code();
                info!("IPP client {} received response: status={:?}", client_id, status_code);

                // Extract printer attributes
                let mut attributes = serde_json::Map::new();
                if status_code.is_success() {
                    if let Some(printer_attrs) = response.attributes().groups_of(DelimiterTag::PrinterAttributes).next() {
                        for (_, attr) in printer_attrs.attributes() {
                            attributes.insert(
                                attr.name().to_string(),
                                serde_json::json!(attr.value().to_string())
                            );
                        }
                    }
                }

                // Call LLM with response
                Self::call_llm_with_response(
                    client_id,
                    &app_state,
                    &llm_client,
                    &status_tx,
                    "get_printer_attributes",
                    status_code.is_success(),
                    serde_json::json!({
                        "status_code": format!("{:?}", status_code),
                        "attributes": attributes,
                    }),
                ).await?;

                Ok(())
            }
            Err(e) => {
                console_error!(status_tx, "[ERROR] IPP Get-Printer-Attributes failed: {}", e);
                Err(e.into())
            }
        }
    }

    /// Send Print-Job operation
    pub async fn print_job(
        client_id: ClientId,
        job_name: String,
        document_format: Option<String>,
        document_data: Vec<u8>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let uri_str = Self::get_uri(&app_state, client_id).await?;
        let uri: Uri = uri_str.parse().context("Invalid IPP URI")?;

        info!("IPP client {} sending Print-Job to {}: job={}, format={:?}, size={} bytes",
            client_id, uri, job_name, document_format, document_data.len());

        // Build Print-Job operation
        // IppPayload needs a Read type, so we convert Vec<u8> to Cursor
        let cursor = std::io::Cursor::new(document_data);
        let payload = IppPayload::new(cursor);

        let operation = IppOperationBuilder::print_job(uri.clone(), payload)
            .job_title(&job_name)
            .build();

        let client = AsyncIppClient::new(uri);

        match client.send(operation).await {
            Ok(response) => {
                let status_code = response.header().status_code();
                info!("IPP client {} Print-Job response: status={:?}", client_id, status_code);

                // Extract job attributes
                let mut job_attrs = serde_json::Map::new();
                if status_code.is_success() {
                    if let Some(attrs) = response.attributes().groups_of(DelimiterTag::JobAttributes).next() {
                        for (_, attr) in attrs.attributes() {
                            job_attrs.insert(
                                attr.name().to_string(),
                                serde_json::json!(attr.value().to_string())
                            );
                        }
                    }
                }

                // Call LLM with response
                Self::call_llm_with_response(
                    client_id,
                    &app_state,
                    &llm_client,
                    &status_tx,
                    "print_job",
                    status_code.is_success(),
                    serde_json::json!({
                        "status_code": format!("{:?}", status_code),
                        "job_name": job_name,
                        "job_attributes": job_attrs,
                    }),
                ).await?;

                Ok(())
            }
            Err(e) => {
                console_error!(status_tx, "[ERROR] IPP Print-Job failed: {}", e);
                Err(e.into())
            }
        }
    }

    /// Send Get-Job-Attributes operation
    pub async fn get_job_attributes(
        client_id: ClientId,
        job_id: i32,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let uri_str = Self::get_uri(&app_state, client_id).await?;
        let uri: Uri = uri_str.parse().context("Invalid IPP URI")?;

        info!("IPP client {} sending Get-Job-Attributes to {}: job_id={}", client_id, uri, job_id);

        let operation = IppOperationBuilder::get_job_attributes(uri.clone(), job_id).build();
        let client = AsyncIppClient::new(uri);

        match client.send(operation).await {
            Ok(response) => {
                let status_code = response.header().status_code();
                info!("IPP client {} Get-Job-Attributes response: status={:?}", client_id, status_code);

                // Extract job attributes
                let mut attributes = serde_json::Map::new();
                if status_code.is_success() {
                    if let Some(job_attrs) = response.attributes().groups_of(DelimiterTag::JobAttributes).next() {
                        for (_, attr) in job_attrs.attributes() {
                            attributes.insert(
                                attr.name().to_string(),
                                serde_json::json!(attr.value().to_string())
                            );
                        }
                    }
                }

                // Call LLM with response
                Self::call_llm_with_response(
                    client_id,
                    &app_state,
                    &llm_client,
                    &status_tx,
                    "get_job_attributes",
                    status_code.is_success(),
                    serde_json::json!({
                        "status_code": format!("{:?}", status_code),
                        "job_id": job_id,
                        "attributes": attributes,
                    }),
                ).await?;

                Ok(())
            }
            Err(e) => {
                console_error!(status_tx, "[ERROR] IPP Get-Job-Attributes failed: {}", e);
                Err(e.into())
            }
        }
    }

    /// Get IPP URI from client state
    async fn get_uri(app_state: &AppState, client_id: ClientId) -> Result<String> {
        app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("ipp_uri")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No IPP URI found in client state")
    }

    /// Call LLM with IPP operation response
    async fn call_llm_with_response(
        client_id: ClientId,
        app_state: &AppState,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
        operation: &str,
        success: bool,
        response_data: serde_json::Value,
    ) -> Result<()> {
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::ipp::actions::IppClientProtocol::new());

            // Use the appropriate event based on operation type
            let event = Event::new(
                &crate::client::ipp::actions::IPP_CLIENT_RESPONSE_RECEIVED_EVENT,
                serde_json::json!({
                    "operation": operation,
                    "success": success,
                    "response": response_data,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions: _, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for IPP client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }
}
