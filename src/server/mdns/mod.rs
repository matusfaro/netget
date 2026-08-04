//! mDNS/DNS-SD server implementation
pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::console_info;
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
#[cfg(feature = "mdns")]
use actions::MDNS_SERVER_STARTUP_EVENT;

/// Port advertised in the SRV record when neither the service definition nor
/// the server's own listen address names one.
#[cfg(feature = "mdns")]
const DEFAULT_ADVERTISED_PORT: u16 = 8080;

/// mDNS server that advertises services based on LLM instructions
pub struct MdnsServer;

#[cfg(feature = "mdns")]
impl MdnsServer {
    /// Spawn mDNS server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        use mdns_sd::ServiceDaemon;

        // mDNS itself does not bind a listening socket, so the server's own
        // port is only meaningful as the default port to advertise for the
        // service being announced.
        let default_port = match listen_addr.port() {
            0 => DEFAULT_ADVERTISED_PORT,
            p => p,
        };

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
            if let Some(services) = params.get_optional_array("services")? {
                info!(
                    "Registering {} services from startup_params",
                    services.len()
                );
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

                        // The advertised port is what discovering clients will
                        // actually connect to; publishing 0 makes the SRV
                        // record useless.
                        let port = service_obj
                            .get("port")
                            .and_then(|v| v.as_u64())
                            .and_then(|p| u16::try_from(p).ok())
                            .unwrap_or(default_port);

                        // Don't fail server startup if registration fails
                        let _ = register_service(
                            &mdns,
                            service_type,
                            service_name,
                            port,
                            &properties,
                            &status_tx,
                        );
                    }
                }
            }
            // Check for single service parameters
            else if let Some(service_type) = params.get_optional_string("service_type")? {
                info!("Registering single service from startup_params");
                used_startup_params = true;
                let service_name = params
                    .get_optional_string("service_name")?
                    .unwrap_or_else(|| "Service".to_string());
                let properties = params
                    .get_optional_object("properties")?
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                let port = params
                    .get_optional_u64("port")?
                    .and_then(|p| u16::try_from(p).ok())
                    .unwrap_or(default_port);

                // Don't fail server startup if registration fails
                let _ = register_service(
                    &mdns,
                    &service_type,
                    &service_name,
                    port,
                    &properties,
                    &status_tx,
                );
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
                            // `as u16` would silently wrap an out-of-range port
                            // (e.g. 70000 -> 4464) into a wrong SRV record.
                            let port = action
                                .get("port")
                                .and_then(|v| v.as_u64())
                                .and_then(|p| u16::try_from(p).ok())
                                .unwrap_or(default_port);

                            let properties = action
                                .get("properties")
                                .and_then(|v| v.as_object())
                                .map(|obj| {
                                    obj.iter()
                                        .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();

                            // Failures are logged by register_service; one bad
                            // service must not abort the remaining registrations.
                            let _ = register_service(
                                &mdns,
                                service_type,
                                instance_name,
                                port,
                                &properties,
                                &status_tx,
                            );
                        }
                    }
                }
            }
        } // Close if !used_startup_params

        // Keep the daemon alive for the lifetime of the server task.
        //
        // `ServiceDaemon` has no `Drop` impl, so simply dropping it leaves the
        // background thread running and the services still being announced on
        // the multicast group. `DaemonGuard` calls `shutdown()` from its own
        // `Drop`, which runs when the task is aborted by `stop_server`.
        let handle = tokio::spawn(async move {
            let _guard = DaemonGuard(mdns);
            std::future::pending::<()>().await;
        });

        // Register the task so stop_server can abort it and stop advertising.
        app_state.register_server_task(server_id, handle).await;

        // mDNS does not bind a listening socket of its own; report the
        // well-known IPv4 multicast group it announces on.
        Ok(SocketAddr::from((
            std::net::Ipv4Addr::new(224, 0, 0, 251),
            5353,
        )))
    }
}

/// Shuts the mDNS daemon down when the owning task is dropped or aborted.
#[cfg(feature = "mdns")]
struct DaemonGuard(mdns_sd::ServiceDaemon);

#[cfg(feature = "mdns")]
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        match self.0.shutdown() {
            Ok(_) => info!("mDNS daemon shutdown requested"),
            Err(e) => error!("Failed to shut down mDNS daemon: {}", e),
        }
    }
}

#[cfg(feature = "mdns")]
fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;

    // Determine which local address the default route would use. `connect` on
    // a UDP socket only performs a routing-table lookup - no packets are sent
    // and the peer is never contacted - so the destination is a documentation
    // address (RFC 5737 TEST-NET-1) rather than a real third-party host.
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("192.0.2.1:80").is_ok() {
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
                    let _ =
                        status_tx.send(format!("[ERROR] ✗ Failed to register mDNS service: {}", e));
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
