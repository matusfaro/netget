//! Server startup logic for TUI mode
//!
//! Handles spawning TCP and HTTP servers based on application state

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::llm::OllamaClient;
use crate::protocol::BaseStack;
use crate::state::app_state::AppState;
use crate::state::ServerId;

/// Start a specific server by ID
pub async fn start_server_by_id(
    state: &AppState,
    server_id: ServerId,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<()> {
    // Get server info
    let server = match state.get_server(server_id).await {
        Some(s) => s,
        None => {
            let _ = status_tx.send(format!("[ERROR] Server #{} not found", server_id.as_u32()));
            return Ok(());
        }
    };

    // Build listen address
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", server.port).parse()?;

    let base_stack = server.base_stack;
    let msg = format!("[SERVER] Starting server #{} ({}) on {}", server_id.as_u32(), base_stack, listen_addr);
    let _ = status_tx.send(msg.clone());

    // Actually spawn the server
    use crate::state::server::ServerStatus;

    match base_stack {
        BaseStack::Tcp => {
            #[cfg(feature = "tcp")]
            {
                use crate::network::tcp::TcpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn TCP server
                match TcpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - default to false for now
                    server_id,
                ).await {
                    Ok(actual_addr) => {
                        // Update server with actual listen address
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] TCP server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start TCP server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "tcp"))]
            {
                let _ = status_tx.send("TCP support not compiled in. Enable 'tcp' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("TCP not compiled".to_string())).await;
            }
        }
        BaseStack::Http => {
            #[cfg(feature = "http")]
            {
                use crate::network::http::HttpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn HTTP server
                match HttpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] HTTP server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start HTTP server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "http"))]
            {
                let _ = status_tx.send("HTTP support not compiled in. Enable 'http' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("HTTP not compiled".to_string())).await;
            }
        }
        BaseStack::DataLink => {
            let _ = status_tx.send("DataLink server not yet implemented in TUI".to_string());
        }
        BaseStack::Udp => {
            #[cfg(feature = "udp")]
            {
                use crate::network::UdpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn UDP server
                match UdpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] UDP server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start UDP server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "udp"))]
            {
                let _ = status_tx.send("UDP support not compiled in. Enable 'udp' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("UDP not compiled".to_string())).await;
            }
        }
        BaseStack::Dns => {
            #[cfg(feature = "dns")]
            {
                use crate::network::dns::DnsServer;
                let state_arc = Arc::new(state.clone());

                // Spawn DNS server
                match DnsServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] DNS server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start DNS server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "dns"))]
            {
                let _ = status_tx.send("DNS support not compiled in. Enable 'dns' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("DNS not compiled".to_string())).await;
            }
        }
        BaseStack::Dhcp => {
            #[cfg(feature = "dhcp")]
            {
                use crate::network::dhcp::DhcpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn DHCP server
                match DhcpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] DHCP server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start DHCP server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "dhcp"))]
            {
                let _ = status_tx.send("DHCP support not compiled in. Enable 'dhcp' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("DHCP not compiled".to_string())).await;
            }
        }
        BaseStack::Ntp => {
            #[cfg(feature = "ntp")]
            {
                use crate::network::ntp::NtpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn NTP server
                match NtpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] NTP server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start NTP server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "ntp"))]
            {
                let _ = status_tx.send("NTP support not compiled in. Enable 'ntp' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("NTP not compiled".to_string())).await;
            }
        }
        BaseStack::Snmp => {
            #[cfg(feature = "snmp")]
            {
                use crate::network::snmp::SnmpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn SNMP server
                match SnmpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] SNMP server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start SNMP server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "snmp"))]
            {
                let _ = status_tx.send("SNMP support not compiled in. Enable 'snmp' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("SNMP not compiled".to_string())).await;
            }
        }
        BaseStack::Ssh => {
            #[cfg(feature = "ssh")]
            {
                use crate::network::ssh::SshServer;
                let state_arc = Arc::new(state.clone());

                // Spawn SSH server
                match SshServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - SSH waits for client
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] SSH server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start SSH server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "ssh"))]
            {
                let _ = status_tx.send("SSH support not compiled in. Enable 'ssh' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("SSH not compiled".to_string())).await;
            }
        }
        BaseStack::Irc => {
            #[cfg(feature = "irc")]
            {
                use crate::network::irc::IrcServer;
                let state_arc = Arc::new(state.clone());

                // Spawn IRC server
                match IrcServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await {
                    Ok(actual_addr) => {
                        state.update_server_status(server_id, ServerStatus::Running).await;
                        let _ = status_tx.send(format!("[SERVER] IRC server #{} listening on {}", server_id.as_u32(), actual_addr));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state.update_server_status(server_id, ServerStatus::Error(e.to_string())).await;
                        let _ = status_tx.send(format!("[ERROR] Failed to start IRC server #{}: {}", server_id.as_u32(), e));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "irc"))]
            {
                let _ = status_tx.send("IRC support not compiled in. Enable 'irc' feature.".to_string());
                state.update_server_status(server_id, ServerStatus::Error("IRC not compiled".to_string())).await;
            }
        }
    }

    Ok(())
}