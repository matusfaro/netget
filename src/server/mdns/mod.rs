//! mDNS/DNS-SD server implementation
pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

#[cfg(feature = "mdns")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "mdns")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "mdns")]
use crate::protocol::Event;
#[cfg(feature = "mdns")]
use crate::server::MdnsProtocol;
#[cfg(feature = "mdns")]
use crate::state::app_state::AppState;
use crate::console_info;
#[cfg(feature = "mdns")]
use actions::MDNS_SERVER_STARTUP_EVENT;

/// mDNS server that advertises services based on LLM instructions
pub struct MdnsServer;

#[cfg(feature = "mdns")]
impl MdnsServer {
    /// Spawn mDNS server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        use mdns_sd::{ServiceDaemon, ServiceInfo};

        info!("mDNS server (action-based) starting");
        let _ = status_tx.send("[INFO] mDNS server starting".to_string());

        let protocol = Arc::new(MdnsProtocol::new());

        // Create mDNS daemon
        let mdns = ServiceDaemon::new()
            .map_err(|e| anyhow::anyhow!("Failed to create mDNS daemon: {}", e))?;
        info!("mDNS daemon created");

        // Track if we successfully processed startup_params
        let mut used_startup_params = false;

        // If startup_params are provided, register services directly
        if let Some(ref params) = startup_params {
            // Check for multiple services array
            if let Some(services) = params.get_optional_array("services") {
                info!("Registering {} services from startup_params", services.len());
                used_startup_params = true;
                for service in services {
                    if let Some(service_obj) = service.as_object() {
                        let service_type = service_obj
                            .get("service_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("_http._tcp.local.");
                        let service_name = service_obj
                            .get("service_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Service");
                        let properties = service_obj
                            .get("properties")
                            .and_then(|v| v.as_object())
                            .map(|obj| {
                                obj.iter()
                                    .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        // Don't fail server startup if registration fails
                        let _ = register_service(&mdns, service_type, service_name, 0, &properties, &status_tx);
                    }
                }
            }
            // Check for single service parameters
            else if let Some(service_type) = params.get_optional_string("service_type") {
                info!("Registering single service from startup_params");
                used_startup_params = true;
                let service_name = params.get_optional_string("service_name")
                    .unwrap_or_else(|| "Service".to_string());
                let properties = params.get_optional_object("properties")
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                // Don't fail server startup if registration fails
                let _ = register_service(&mdns, &service_type, &service_name, 0, &properties, &status_tx);
            }
        }

        // Only call LLM if we didn't use startup_params
        if !used_startup_params {
            // Create mDNS server startup event
            let event = Event::new(&MDNS_SERVER_STARTUP_EVENT, serde_json::json!({}));

            // Get LLM's service registration instructions
            // mDNS manually processes register_mdns_service actions using raw_actions
            if let Ok(execution_result) = call_llm(
            &llm_client,
            &app_state,
            server_id,
            None,
            &event,
            protocol.as_ref(),
        )
        .await
        {
            // Display messages from LLM
            for message in &execution_result.messages {
                console_info!(status_tx, "{}", message);
            }

            // Process raw actions for manual mDNS service registration
            for action in execution_result.raw_actions {
                if let Some(action_type) = action.get("type").and_then(|v| v.as_str()) {
                    if action_type == "register_mdns_service" {
                        // Extract service parameters
                        let service_type = action
                            .get("service_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("_http._tcp.local.");
                        let instance_name = action
                            .get("instance_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("MyService");
                        let port =
                            action.get("port").and_then(|v| v.as_u64()).unwrap_or(8080) as u16;

                        let properties = action
                            .get("properties")
                            .and_then(|v| v.as_object())
                            .map(|obj| {
                                obj.iter()
                                    .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        // Get local IP (simplified - use first non-loopback interface)
                        let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
                        let host_name = format!("{}.local.", instance_name.replace(" ", "-"));

                        // Create ServiceInfo
                        match ServiceInfo::new(
                            service_type,
                            instance_name,
                            &host_name,
                            &local_ip,
                            port,
                            &properties[..],
                        ) {
                            Ok(service_info) => {
                                // Register service
                                match mdns.register(service_info) {
                                    Ok(_) => {
                                        info!(
                                            "mDNS registered service: {} ({}:{})",
                                            instance_name, local_ip, port
                                        );
                                        let _ = status_tx.send(format!(
                                            "[INFO] → mDNS registered service: {} ({}:{})",
                                            instance_name, local_ip, port
                                        ));
                                    }
                                    Err(e) => {
                                        error!("Failed to register mDNS service: {}", e);
                                        let _ = status_tx.send(format!(
                                            "[ERROR] ✗ Failed to register mDNS service: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to create ServiceInfo: {}", e);
                                let _ = status_tx
                                    .send(format!("[ERROR] ✗ Failed to create ServiceInfo: {}", e));
                            }
                        }
                    }
                }
            }
        }
        } // Close if !used_startup_params

        // Keep daemon running
        tokio::spawn(async move {
            // Store daemon to keep it alive
            let _daemon = mdns;

            // Keep running indefinitely
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            }
        });

        // Return a dummy address since mDNS doesn't bind to a specific port
        Ok("224.0.0.251:5353".parse().unwrap())
    }
}

#[cfg(feature = "mdns")]
fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;

    // Try to get local IP by connecting to a public DNS server
    // This doesn't actually send any packets, just determines the local IP
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return Some(addr.ip().to_string());
            }
        }
    }
    None
}

#[cfg(feature = "mdns")]
fn register_service(
    mdns: &mdns_sd::ServiceDaemon,
    service_type: &str,
    instance_name: &str,
    port: u16,
    properties: &[(&str, &str)],
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<()> {
    use mdns_sd::ServiceInfo;

    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let host_name = format!("{}.local.", instance_name.replace(" ", "-"));

    // Create ServiceInfo
    match ServiceInfo::new(
        service_type,
        instance_name,
        &host_name,
        &local_ip,
        port,
        properties,
    ) {
        Ok(service_info) => {
            // Register service
            match mdns.register(service_info) {
                Ok(_) => {
                    info!(
                        "mDNS registered service: {} ({}:{})",
                        instance_name, local_ip, port
                    );
                    let _ = status_tx.send(format!(
                        "[INFO] → mDNS registered service: {} ({}:{})",
                        instance_name, local_ip, port
                    ));
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to register mDNS service: {}", e);
                    let _ = status_tx.send(format!(
                        "[ERROR] ✗ Failed to register mDNS service: {}",
                        e
                    ));
                    Err(anyhow::anyhow!("Failed to register mDNS service: {}", e))
                }
            }
        }
        Err(e) => {
            error!("Failed to create ServiceInfo: {}", e);
            let _ = status_tx.send(format!("[ERROR] ✗ Failed to create ServiceInfo: {}", e));
            Err(anyhow::anyhow!("Failed to create ServiceInfo: {}", e))
        }
    }
}

#[cfg(not(feature = "mdns"))]
impl MdnsServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        anyhow::bail!("mDNS feature not enabled")
    }
}
