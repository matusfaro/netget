//! mDNS client implementation
pub mod actions;

pub use actions::MdnsClientProtocol;

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::mdns::actions::{MDNS_CLIENT_CONNECTED_EVENT, MDNS_CLIENT_SERVICE_FOUND_EVENT, MDNS_CLIENT_SERVICE_RESOLVED_EVENT};

/// mDNS client that performs service discovery on the local network
pub struct MdnsClient;

impl MdnsClient {
    /// Initialize mDNS client with integrated LLM actions
    pub async fn connect_with_llm_actions(
        _remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("mDNS client {} initializing", client_id);

        // Create mDNS service daemon
        let mdns = ServiceDaemon::new()
            .context("Failed to create mDNS service daemon")?;

        // Store daemon handle in protocol_data
        // Note: mdns daemon is not directly serializable, so we just mark it as initialized
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "mdns_initialized".to_string(),
                serde_json::json!(true),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] mDNS client {} initialized", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event to get initial instructions
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::mdns::actions::MdnsClientProtocol::new());
            let event = Event::new(
                &MDNS_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "status": "connected",
                    "message": "mDNS client ready for service discovery"
                }),
            );

            let llm_client_clone = llm_client.clone();
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();

            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client_clone,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    "",
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute initial actions
                        for action in actions {
                            if let Err(e) = Self::execute_mdns_action(
                                client_id,
                                action,
                                &mdns,
                                llm_client_clone.clone(),
                                app_state_clone.clone(),
                                status_tx_clone.clone(),
                            ).await {
                                error!("Failed to execute mDNS action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for mDNS client {}: {}", client_id, e);
                    }
                }
            });
        }

        // Spawn monitoring task to check for client disconnection
        let app_state_monitor = app_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state_monitor.get_client(client_id).await.is_none() {
                    info!("mDNS client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (mDNS is multicast UDP)
        Ok("224.0.0.251:5353".parse().unwrap())
    }

    /// Execute an mDNS action (browse, resolve, etc.)
    async fn execute_mdns_action(
        client_id: ClientId,
        action: serde_json::Value,
        mdns: &ServiceDaemon,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::Client;
        let protocol = Arc::new(crate::client::mdns::actions::MdnsClientProtocol::new());

        match protocol.as_ref().execute_action(action.clone()) {
            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                match name.as_str() {
                    "browse_service" => {
                        let service_type = data["service_type"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing service_type"))?;

                        info!("mDNS client {} browsing for service: {}", client_id, service_type);
                        let _ = status_tx.send(format!("[CLIENT] Browsing for mDNS service: {}", service_type));

                        // Start browsing
                        let receiver = mdns.browse(service_type)
                            .context("Failed to browse service")?;

                        // Spawn task to handle browse events
                        let llm_client_browse = llm_client.clone();
                        let app_state_browse = app_state.clone();
                        let status_tx_browse = status_tx.clone();
                        let protocol_browse = protocol.clone();

                        tokio::spawn(async move {
                            loop {
                                // Use recv_timeout to avoid blocking forever
                                match receiver.recv_timeout(Duration::from_secs(10)) {
                                    Ok(event) => {
                                        match event {
                                            ServiceEvent::ServiceFound(service_type, fullname) => {
                                                trace!("mDNS service found: {} ({})", fullname, service_type);

                                                // Call LLM with service found event
                                                if let Some(instruction) = app_state_browse.get_instruction_for_client(client_id).await {
                                                    let llm_event = Event::new(
                                                        &MDNS_CLIENT_SERVICE_FOUND_EVENT,
                                                        serde_json::json!({
                                                            "service_type": service_type,
                                                            "fullname": fullname,
                                                        }),
                                                    );

                                                    let memory = app_state_browse.get_memory_for_client(client_id).await.unwrap_or_default();

                                                    match call_llm_for_client(
                                                        &llm_client_browse,
                                                        &app_state_browse,
                                                        client_id.to_string(),
                                                        &instruction,
                                                        &memory,
                                                        Some(&llm_event),
                                                        protocol_browse.as_ref(),
                                                        &status_tx_browse,
                                                    ).await {
                                                        Ok(ClientLlmResult { actions: _, memory_updates }) => {
                                                            if let Some(mem) = memory_updates {
                                                                app_state_browse.set_memory_for_client(client_id, mem).await;
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!("LLM error processing service found: {}", e);
                                                        }
                                                    }
                                                }
                                            }
                                            ServiceEvent::ServiceResolved(info) => {
                                                let first_addr = info.get_addresses().iter().next()
                                                    .map(|scoped| scoped.to_string())
                                                    .unwrap_or_else(|| "0.0.0.0".to_string());

                                                info!("mDNS service resolved: {} at {}:{}",
                                                    info.get_fullname(),
                                                    first_addr,
                                                    info.get_port()
                                                );

                                                // Call LLM with service resolved event
                                                if let Some(instruction) = app_state_browse.get_instruction_for_client(client_id).await {
                                                    let llm_event = Event::new(
                                                        &MDNS_CLIENT_SERVICE_RESOLVED_EVENT,
                                                        serde_json::json!({
                                                            "fullname": info.get_fullname(),
                                                            "hostname": info.get_hostname(),
                                                            "addresses": info.get_addresses().iter().map(|a| a.to_string()).collect::<Vec<_>>(),
                                                            "port": info.get_port(),
                                                            "properties": info.get_properties().iter().map(|p| format!("{}={}", p.key(), p.val_str())).collect::<Vec<_>>(),
                                                        }),
                                                    );

                                                    let memory = app_state_browse.get_memory_for_client(client_id).await.unwrap_or_default();

                                                    match call_llm_for_client(
                                                        &llm_client_browse,
                                                        &app_state_browse,
                                                        client_id.to_string(),
                                                        &instruction,
                                                        &memory,
                                                        Some(&llm_event),
                                                        protocol_browse.as_ref(),
                                                        &status_tx_browse,
                                                    ).await {
                                                        Ok(ClientLlmResult { actions: _, memory_updates }) => {
                                                            if let Some(mem) = memory_updates {
                                                                app_state_browse.set_memory_for_client(client_id, mem).await;
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!("LLM error processing service resolved: {}", e);
                                                        }
                                                    }
                                                }
                                            }
                                            ServiceEvent::ServiceRemoved(service_type, fullname) => {
                                                info!("mDNS service removed: {} ({})", fullname, service_type);
                                            }
                                            ServiceEvent::SearchStarted(service_type) => {
                                                trace!("mDNS search started for: {}", service_type);
                                            }
                                            ServiceEvent::SearchStopped(service_type) => {
                                                trace!("mDNS search stopped for: {}", service_type);
                                                break;
                                            }
                                            _ => {
                                                // Handle any other event types
                                                trace!("mDNS unhandled event type");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Check if it's a timeout or disconnection
                                        if format!("{:?}", e).contains("Timeout") {
                                            // Timeout is expected, check if client still exists
                                            if app_state_browse.get_client(client_id).await.is_none() {
                                                info!("mDNS browse task stopping (client removed)");
                                                break;
                                            }
                                        } else {
                                            info!("mDNS browse channel error: {}", e);
                                            break;
                                        }
                                    }
                                }
                            }
                        });
                    }
                    "resolve_hostname" => {
                        let hostname = data["hostname"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing hostname"))?;

                        info!("mDNS client {} resolving hostname: {}", client_id, hostname);

                        // Use mdns to resolve hostname (timeout in milliseconds)
                        match mdns.resolve_hostname(hostname, Some(5000)) {
                            Ok(addrs) => {
                                info!("Resolved {} to {} addresses", hostname, addrs.len());
                                let _ = status_tx.send(format!("[CLIENT] Resolved {}: {:?}", hostname, addrs));
                            }
                            Err(e) => {
                                warn!("Failed to resolve {}: {}", hostname, e);
                                let _ = status_tx.send(format!("[CLIENT] Failed to resolve {}: {}", hostname, e));
                            }
                        }
                    }
                    _ => {
                        warn!("Unknown mDNS action: {}", name);
                    }
                }
            }
            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                info!("mDNS client {} disconnecting", client_id);
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                let _ = status_tx.send(format!("[CLIENT] mDNS client {} disconnected", client_id));
            }
            Ok(crate::llm::actions::client_trait::ClientActionResult::WaitForMore) => {
                trace!("mDNS client {} waiting for more data", client_id);
            }
            _ => {}
        }

        Ok(())
    }
}
