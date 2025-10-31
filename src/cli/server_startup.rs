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
    let msg = format!(
        "[SERVER] Starting server #{} ({}) on {}",
        server_id.as_u32(),
        base_stack,
        listen_addr
    );
    let _ = status_tx.send(msg.clone());

    // Actually spawn the server
    use crate::state::server::ServerStatus;

    match base_stack {
        BaseStack::Tcp => {
            #[cfg(feature = "tcp")]
            {
                use crate::server::tcp::TcpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn TCP server
                match TcpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - default to false for now
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        // Update server with actual listen address
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] TCP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start TCP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "tcp"))]
            {
                let _ = status_tx
                    .send("TCP support not compiled in. Enable 'tcp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("TCP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Http => {
            #[cfg(feature = "http")]
            {
                use crate::server::http::HttpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn HTTP server
                match HttpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] HTTP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start HTTP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "http"))]
            {
                let _ = status_tx
                    .send("HTTP support not compiled in. Enable 'http' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("HTTP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::DataLink => {
            #[cfg(feature = "datalink")]
            {
                let _ = status_tx.send("DataLink server not yet implemented in TUI".to_string());
            }
            #[cfg(not(feature = "datalink"))]
            {
                let _ = status_tx
                    .send("DataLink support not compiled in. Enable 'datalink' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("DataLink not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Udp => {
            #[cfg(feature = "udp")]
            {
                use crate::server::UdpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn UDP server
                match UdpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] UDP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start UDP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "udp"))]
            {
                let _ = status_tx
                    .send("UDP support not compiled in. Enable 'udp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("UDP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Dns => {
            #[cfg(feature = "dns")]
            {
                use crate::server::dns::DnsServer;
                let state_arc = Arc::new(state.clone());

                // Spawn DNS server
                match DnsServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] DNS server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start DNS server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "dns"))]
            {
                let _ = status_tx
                    .send("DNS support not compiled in. Enable 'dns' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("DNS not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Dot => {
            #[cfg(feature = "dot")]
            {
                use crate::server::dot::DotServer;
                let state_arc = Arc::new(state.clone());

                // Spawn DoT server
                match DotServer::spawn(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    server_id,
                    status_tx.clone(),
                )
                .await
                {
                    Ok(_handle) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] DoT server #{} listening on {}",
                            server_id.as_u32(),
                            listen_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start DoT server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "dot"))]
            {
                let _ = status_tx
                    .send("DoT support not compiled in. Enable 'dot' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("DoT not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Doh => {
            #[cfg(feature = "doh")]
            {
                use crate::server::doh::DohServer;
                let state_arc = Arc::new(state.clone());

                // Spawn DoH server
                match DohServer::spawn(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    server_id,
                    status_tx.clone(),
                )
                .await
                {
                    Ok(_handle) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] DoH server #{} listening on {}",
                            server_id.as_u32(),
                            listen_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start DoH server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "doh"))]
            {
                let _ = status_tx
                    .send("DoH support not compiled in. Enable 'doh' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("DoH not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Dhcp => {
            #[cfg(feature = "dhcp")]
            {
                use crate::server::dhcp::DhcpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn DHCP server
                match DhcpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] DHCP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start DHCP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "dhcp"))]
            {
                let _ = status_tx
                    .send("DHCP support not compiled in. Enable 'dhcp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("DHCP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Ntp => {
            #[cfg(feature = "ntp")]
            {
                use crate::server::ntp::NtpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn NTP server
                match NtpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] NTP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start NTP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "ntp"))]
            {
                let _ = status_tx
                    .send("NTP support not compiled in. Enable 'ntp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("NTP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Snmp => {
            #[cfg(feature = "snmp")]
            {
                use crate::server::snmp::SnmpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn SNMP server
                match SnmpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] SNMP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start SNMP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "snmp"))]
            {
                let _ = status_tx
                    .send("SNMP support not compiled in. Enable 'snmp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("SNMP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Ssh => {
            #[cfg(feature = "ssh")]
            {
                use crate::server::ssh::SshServer;
                let state_arc = Arc::new(state.clone());

                // Spawn SSH server
                match SshServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - SSH waits for client
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] SSH server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start SSH server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "ssh"))]
            {
                let _ = status_tx
                    .send("SSH support not compiled in. Enable 'ssh' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("SSH not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Irc => {
            #[cfg(feature = "irc")]
            {
                use crate::server::irc::IrcServer;
                let state_arc = Arc::new(state.clone());

                // Spawn IRC server
                match IrcServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] IRC server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start IRC server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "irc"))]
            {
                let _ = status_tx
                    .send("IRC support not compiled in. Enable 'irc' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("IRC not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Telnet => {
            #[cfg(feature = "telnet")]
            {
                use crate::server::telnet::TelnetServer;
                let state_arc = Arc::new(state.clone());

                // Spawn Telnet server
                match TelnetServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Telnet server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start Telnet server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "telnet"))]
            {
                let _ = status_tx
                    .send("Telnet support not compiled in. Enable 'telnet' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("Telnet not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Smtp => {
            #[cfg(feature = "smtp")]
            {
                use crate::server::smtp::SmtpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn SMTP server
                match SmtpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] SMTP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start SMTP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "smtp"))]
            {
                let _ = status_tx
                    .send("SMTP support not compiled in. Enable 'smtp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("SMTP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Mdns => {
            #[cfg(feature = "mdns")]
            {
                use crate::server::mdns::MdnsServer;
                let state_arc = Arc::new(state.clone());

                // Spawn mDNS server
                match MdnsServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] mDNS server #{} advertising services on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start mDNS server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "mdns"))]
            {
                let _ = status_tx
                    .send("mDNS support not compiled in. Enable 'mdns' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("mDNS not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Mysql => {
            #[cfg(feature = "mysql")]
            {
                use crate::server::mysql::MysqlServer;
                let state_arc = Arc::new(state.clone());

                // Spawn MySQL server
                match MysqlServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - MySQL waits for client
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] MySQL server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start MySQL server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "mysql"))]
            {
                let _ = status_tx
                    .send("MySQL support not compiled in. Enable 'mysql' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("MySQL not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Ipp => {
            #[cfg(feature = "ipp")]
            {
                use crate::server::ipp::IppServer;
                let state_arc = Arc::new(state.clone());

                // Spawn IPP server
                match IppServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - IPP waits for client
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] IPP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start IPP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "ipp"))]
            {
                let _ = status_tx
                    .send("IPP support not compiled in. Enable 'ipp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("IPP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Postgresql => {
            #[cfg(feature = "postgresql")]
            {
                use crate::server::postgresql::PostgresqlServer;
                let state_arc = Arc::new(state.clone());

                // Spawn PostgreSQL server
                match PostgresqlServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - PostgreSQL waits for client
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] PostgreSQL server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start PostgreSQL server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "postgresql"))]
            {
                let _ = status_tx.send(
                    "PostgreSQL support not compiled in. Enable 'postgresql' feature.".to_string(),
                );
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("PostgreSQL not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Redis => {
            #[cfg(feature = "redis")]
            {
                use crate::server::redis::RedisServer;
                let state_arc = Arc::new(state.clone());

                // Spawn Redis server
                match RedisServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - Redis waits for client
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Redis server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start Redis server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "redis"))]
            {
                let _ = status_tx
                    .send("Redis support not compiled in. Enable 'redis' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("Redis not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Cassandra => {
            #[cfg(feature = "cassandra")]
            {
                use crate::server::cassandra::CassandraServer;
                let state_arc = Arc::new(state.clone());

                // Spawn Cassandra server
                match CassandraServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first - Cassandra waits for client
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Cassandra server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start Cassandra server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "cassandra"))]
            {
                let _ = status_tx
                    .send("Cassandra support not compiled in. Enable 'cassandra' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("Cassandra not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Proxy => {
            #[cfg(feature = "proxy")]
            {
                use crate::server::proxy::ProxyServer;
                let state_arc = Arc::new(state.clone());

                // Get startup params from server
                let startup_params = server.startup_params.clone();

                // Spawn HTTP Proxy server
                match ProxyServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                    startup_params,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Proxy server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start Proxy server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "proxy"))]
            {
                let _ = status_tx
                    .send("Proxy support not compiled in. Enable 'proxy' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("Proxy not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::WebDav => {
            #[cfg(feature = "webdav")]
            {
                use crate::server::webdav::WebDavServer;
                let state_arc = Arc::new(state.clone());

                // Spawn WebDAV server
                match WebDavServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] WebDAV server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start WebDAV server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "webdav"))]
            {
                let _ = status_tx
                    .send("WebDAV support not compiled in. Enable 'webdav' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("WebDAV not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Nfs => {
            #[cfg(feature = "nfs")]
            {
                use crate::server::nfs::NfsServer;
                let state_arc = Arc::new(state.clone());

                // Spawn NFS server
                match NfsServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] NFS server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start NFS server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "nfs"))]
            {
                let _ = status_tx
                    .send("NFS support not compiled in. Enable 'nfs' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("NFS not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Smb => {
            #[cfg(feature = "smb")]
            {
                use crate::server::smb::SmbServer;
                let state_arc = Arc::new(state.clone());

                // Spawn SMB server
                match SmbServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] SMB server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start SMB server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "smb"))]
            {
                let _ = status_tx
                    .send("SMB support not compiled in. Enable 'smb' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("SMB not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Stun => {
            #[cfg(feature = "stun")]
            {
                use crate::server::stun::StunServer;
                let state_arc = Arc::new(state.clone());

                // Spawn STUN server
                match StunServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] STUN server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start STUN server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "stun"))]
            {
                let _ = status_tx
                    .send("STUN support not compiled in. Enable 'stun' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("STUN not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Turn => {
            #[cfg(feature = "turn")]
            {
                use crate::server::turn::TurnServer;
                let state_arc = Arc::new(state.clone());

                // Spawn TURN server
                match TurnServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] TURN server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start TURN server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "turn"))]
            {
                let _ = status_tx
                    .send("TURN support not compiled in. Enable 'turn' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("TURN not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Ldap => {
            #[cfg(feature = "ldap")]
            {
                use crate::server::ldap::LdapServer;
                let state_arc = Arc::new(state.clone());

                // Spawn LDAP server
                match LdapServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] LDAP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start LDAP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "ldap"))]
            {
                let _ = status_tx
                    .send("LDAP support not compiled in. Enable 'ldap' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("LDAP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Bgp => {
            #[cfg(feature = "bgp")]
            {
                use crate::server::bgp::BgpServer;
                let state_arc = Arc::new(state.clone());

                // Get startup params from server
                let startup_params = server.startup_params.clone();

                // Spawn BGP server
                match BgpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                    startup_params,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] BGP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start BGP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "bgp"))]
            {
                let _ = status_tx
                    .send("BGP support not compiled in. Enable 'bgp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("BGP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Imap => {
            #[cfg(feature = "imap")]
            {
                use crate::server::imap::ImapServer;
                let state_arc = Arc::new(state.clone());

                // Check if port 993 (IMAPS/TLS) or 143 (plain IMAP)
                let is_tls = listen_addr.port() == 993;

                if is_tls {
                    #[cfg(feature = "proxy")]
                    {
                        // Spawn IMAPS server with TLS
                        match ImapServer::spawn_with_tls(
                            listen_addr,
                            llm_client.clone(),
                            state_arc,
                            status_tx.clone(),
                            server_id,
                        )
                        .await
                        {
                            Ok(actual_addr) => {
                                state
                                    .update_server_status(server_id, ServerStatus::Running)
                                    .await;
                                let _ = status_tx.send(format!(
                                    "[SERVER] IMAPS server #{} listening on {} (TLS)",
                                    server_id.as_u32(),
                                    actual_addr
                                ));
                                let _ = status_tx.send("__UPDATE_UI__".to_string());
                            }
                            Err(e) => {
                                state
                                    .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                                    .await;
                                let _ = status_tx.send(format!(
                                    "[ERROR] Failed to start IMAPS server #{}: {}",
                                    server_id.as_u32(),
                                    e
                                ));
                                let _ = status_tx.send("__UPDATE_UI__".to_string());
                                return Err(e);
                            }
                        }
                    }
                    #[cfg(not(feature = "proxy"))]
                    {
                        let _ = status_tx.send(
                            "IMAPS (TLS) requires 'proxy' feature for TLS support. Use port 143 for plain IMAP."
                                .to_string(),
                        );
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error("TLS support not compiled".to_string()),
                            )
                            .await;
                    }
                } else {
                    // Spawn plain IMAP server
                    match ImapServer::spawn_with_llm_actions(
                        listen_addr,
                        llm_client.clone(),
                        state_arc,
                        status_tx.clone(),
                        server_id,
                    )
                    .await
                    {
                        Ok(actual_addr) => {
                            state
                                .update_server_status(server_id, ServerStatus::Running)
                                .await;
                            let _ = status_tx.send(format!(
                                "[SERVER] IMAP server #{} listening on {}",
                                server_id.as_u32(),
                                actual_addr
                            ));
                            let _ = status_tx.send("__UPDATE_UI__".to_string());
                        }
                        Err(e) => {
                            state
                                .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                                .await;
                            let _ = status_tx.send(format!(
                                "[ERROR] Failed to start IMAP server #{}: {}",
                                server_id.as_u32(),
                                e
                            ));
                            let _ = status_tx.send("__UPDATE_UI__".to_string());
                            return Err(e);
                        }
                    }
                }
            }
            #[cfg(not(feature = "imap"))]
            {
                let _ = status_tx
                    .send("IMAP support not compiled in. Enable 'imap' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("IMAP not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Elasticsearch => {
            #[cfg(feature = "elasticsearch")]
            {
                use crate::server::elasticsearch::ElasticsearchServer;
                let state_arc = Arc::new(state.clone());

                // Spawn Elasticsearch server
                match ElasticsearchServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Elasticsearch server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start Elasticsearch server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "elasticsearch"))]
            {
                let _ = status_tx
                    .send("Elasticsearch support not compiled in. Enable 'elasticsearch' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("Elasticsearch not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Dynamo => {
            #[cfg(feature = "dynamo")]
            {
                use crate::server::dynamo::DynamoServer;
                let state_arc = Arc::new(state.clone());

                // Spawn DynamoDB server
                match DynamoServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] DynamoDB server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start DynamoDB server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "dynamo"))]
            {
                let _ = status_tx
                    .send("DynamoDB support not compiled in. Enable 'dynamo' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("DynamoDB not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::OpenAi => {
            #[cfg(feature = "openai")]
            {
                use crate::server::openai::OpenAiServer;
                let state_arc = Arc::new(state.clone());

                // Spawn OpenAI API server
                match OpenAiServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] OpenAI API server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start OpenAI API server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "openai"))]
            {
                let _ = status_tx
                    .send("OpenAI support not compiled in. Enable 'openai' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("OpenAI not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::JsonRpc => {
            #[cfg(feature = "jsonrpc")]
            {
                use crate::server::jsonrpc::JsonRpcServer;
                let state_arc = Arc::new(state.clone());

                // Spawn JSON-RPC server
                match JsonRpcServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    false, // send_first
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] JSON-RPC server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start JSON-RPC server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "jsonrpc"))]
            {
                let _ = status_tx
                    .send("JSON-RPC support not compiled in. Enable 'jsonrpc' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("JSON-RPC not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Socks5 => {
            #[cfg(feature = "socks5")]
            {
                use crate::server::socks5::Socks5Server;
                let state_arc = Arc::new(state.clone());

                // Get startup params from server
                let startup_params = server.startup_params.clone();

                // Spawn SOCKS5 server
                match Socks5Server::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                    startup_params,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] SOCKS5 server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start SOCKS5 server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "socks5"))]
            {
                let _ = status_tx
                    .send("SOCKS5 support not compiled in. Enable 'socks5' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("SOCKS5 not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Wireguard => {
            #[cfg(feature = "wireguard")]
            {
                use crate::server::wireguard::WireguardServer;
                let state_arc = Arc::new(state.clone());

                // Spawn WireGuard server
                match WireguardServer::spawn_with_llm_actions(
                    listen_addr,
                    Arc::new(llm_client.clone()),
                    state_arc,
                    server_id,
                    status_tx.clone(),
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] WireGuard VPN server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start WireGuard server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "wireguard"))]
            {
                let _ = status_tx.send(
                    "WireGuard support not compiled in. Enable 'wireguard' feature and ensure boringtun/tokio-tun are available."
                        .to_string(),
                );
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("WireGuard not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Openvpn => {
            #[cfg(feature = "openvpn")]
            {
                use crate::server::openvpn::OpenvpnServer;
                let state_arc = Arc::new(state.clone());

                // Spawn OpenVPN honeypot
                match OpenvpnServer::spawn_with_llm_actions(
                    listen_addr,
                    Arc::new(llm_client.clone()),
                    state_arc,
                    server_id,
                    status_tx.clone(),
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] OpenVPN honeypot server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start OpenVPN server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "openvpn"))]
            {
                let _ = status_tx.send(
                    "OpenVPN support not compiled in. Enable 'openvpn' feature."
                        .to_string(),
                );
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("OpenVPN not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Ipsec => {
            #[cfg(feature = "ipsec")]
            {
                use crate::server::ipsec::IpsecServer;
                let state_arc = Arc::new(state.clone());

                // Spawn IPSec honeypot
                match IpsecServer::spawn_with_llm_actions(
                    listen_addr,
                    Arc::new(llm_client.clone()),
                    state_arc,
                    server_id,
                    status_tx.clone(),
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] IPSec/IKEv2 honeypot server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start IPSec server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "ipsec"))]
            {
                let _ = status_tx.send(
                    "IPSec support not compiled in. Enable 'ipsec' feature."
                        .to_string(),
                );
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("IPSec not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Mcp => {
            #[cfg(feature = "mcp")]
            {
                use crate::server::mcp::McpServer;
                let state_arc = Arc::new(state.clone());

                // Spawn MCP server
                match McpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] MCP server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start MCP server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "mcp"))]
            {
                let _ = status_tx
                    .send("MCP support not compiled in. Enable 'mcp' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("MCP feature not enabled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Grpc => {
            #[cfg(feature = "grpc")]
            {
                use crate::server::grpc::GrpcServer;
                let state_arc = Arc::new(state.clone());

                // Get startup params from server (must contain proto_schema)
                let startup_params = server.startup_params.clone();

                // Spawn gRPC server with LLM-provided schema
                match GrpcServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                    startup_params,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] gRPC server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start gRPC server: {}",
                            e
                        ));
                    }
                }
            }
            #[cfg(not(feature = "grpc"))]
            {
                let _ = status_tx
                    .send("gRPC support not compiled in. Enable 'grpc' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("gRPC not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::XmlRpc => {
            #[cfg(feature = "xmlrpc")]
            {
                use crate::server::xmlrpc::XmlRpcServer;
                let state_arc = Arc::new(state.clone());

                // Spawn XML-RPC server
                match XmlRpcServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] XML-RPC server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start XML-RPC server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "xmlrpc"))]
            {
                let _ = status_tx
                    .send("XML-RPC support not compiled in. Enable 'xmlrpc' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("XML-RPC not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::TorDirectory => {
            #[cfg(feature = "tor-directory")]
            {
                use crate::server::tor_directory::TorDirectoryServer;
                let state_arc = Arc::new(state.clone());

                // Spawn Tor Directory server
                match TorDirectoryServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Tor Directory server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start Tor Directory server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "tor-directory"))]
            {
                let _ = status_tx
                    .send("Tor Directory support not compiled in. Enable 'tor-directory' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("Tor Directory not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::TorRelay => {
            #[cfg(feature = "tor-relay")]
            {
                use crate::server::tor_relay::TorRelayServer;
                let state_arc = Arc::new(state.clone());

                // Spawn Tor Relay server (OR protocol with TLS)
                match TorRelayServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Tor Relay (OR protocol) server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start Tor Relay server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "tor-relay"))]
            {
                let _ = status_tx
                    .send("Tor Relay support not compiled in. Enable 'tor-relay' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("Tor Relay not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::Vnc => {
            #[cfg(feature = "vnc")]
            {
                use crate::server::vnc::VncServer;
                let state_arc = Arc::new(state.clone());

                // Spawn VNC server
                match VncServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] Starting server #{} (ETH>IP>TCP>VNC) on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start VNC server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "vnc"))]
            {
                let _ = status_tx
                    .send("VNC support not compiled in. Enable 'vnc' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("VNC not compiled".to_string()),
                    )
                    .await;
            }
        }
        BaseStack::OpenApi => {
            #[cfg(feature = "openapi")]
            {
                use crate::server::openapi::OpenApiServer;
                let state_arc = Arc::new(state.clone());

                // Get startup params from server (may contain spec or spec_file)
                let startup_params = server.startup_params.clone();

                // Spawn OpenAPI server
                match OpenApiServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    server_id,
                    startup_params,
                )
                .await
                {
                    Ok(actual_addr) => {
                        state
                            .update_server_status(server_id, ServerStatus::Running)
                            .await;
                        let _ = status_tx.send(format!(
                            "[SERVER] OpenAPI server #{} listening on {}",
                            server_id.as_u32(),
                            actual_addr
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                    }
                    Err(e) => {
                        state
                            .update_server_status(
                                server_id,
                                ServerStatus::Error(e.to_string()),
                            )
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start OpenAPI server #{}: {}",
                            server_id.as_u32(),
                            e
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "openapi"))]
            {
                let _ = status_tx
                    .send("OpenAPI support not compiled in. Enable 'openapi' feature.".to_string());
                state
                    .update_server_status(
                        server_id,
                        ServerStatus::Error("OpenAPI not compiled".to_string()),
                    )
                    .await;
            }
        }
    }

    Ok(())
}
