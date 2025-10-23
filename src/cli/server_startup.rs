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

/// Check if server needs to be started and start it
pub async fn check_and_start_server(
    state: &AppState,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<()> {
    use crate::state::app_state::Mode;

    // Check if we're in server mode and not yet listening
    if state.get_mode().await != Mode::Server {
        return Ok(());
    }

    if state.get_local_addr().await.is_some() {
        // Already listening
        return Ok(());
    }

    // Get port from state (set by OpenServer action)
    let port = state.get_port().await.unwrap_or(1234);
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;

    // Store the listen address
    state.set_local_addr(Some(listen_addr)).await;

    // Start server based on base stack
    let base_stack = state.get_base_stack().await;
    let msg = format!("Starting {} server on {}", base_stack, listen_addr);
    let _ = status_tx.send(msg.clone());

    match base_stack {
        BaseStack::TcpRaw => {
            #[cfg(feature = "tcp")]
            {
                use crate::network::tcp::TcpServer;
                let state_arc = Arc::new(state.clone());
                let send_first = state.get_send_first().await;
                TcpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    send_first,
                ).await?;
            }
            #[cfg(not(feature = "tcp"))]
            {
                let _ = status_tx.send("TCP support not compiled in. Enable 'tcp' feature.".to_string());
            }
        }
        BaseStack::Http => {
            #[cfg(feature = "http")]
            {
                use crate::network::http::HttpServer;
                let state_arc = Arc::new(state.clone());
                HttpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await?;
            }
            #[cfg(not(feature = "http"))]
            {
                let _ = status_tx.send("HTTP support not compiled in. Enable 'http' feature.".to_string());
            }
        }
        BaseStack::DataLink => {
            let _ = status_tx.send("DataLink server not yet implemented in TUI".to_string());
        }
        BaseStack::UdpRaw => {
            #[cfg(feature = "udp")]
            {
                use crate::network::UdpServer;
                let state_arc = Arc::new(state.clone());
                UdpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await?;
            }
            #[cfg(not(feature = "udp"))]
            {
                let _ = status_tx.send("UDP support not compiled in. Enable 'udp' feature.".to_string());
            }
        }
        BaseStack::Dns => {
            #[cfg(feature = "dns")]
            {
                use crate::network::DnsServer;
                let state_arc = Arc::new(state.clone());
                DnsServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await?;
            }
            #[cfg(not(feature = "dns"))]
            {
                let _ = status_tx.send("DNS support not compiled in. Enable 'dns' feature.".to_string());
            }
        }
        BaseStack::Dhcp => {
            #[cfg(feature = "dhcp")]
            {
                use crate::network::DhcpServer;
                let state_arc = Arc::new(state.clone());
                DhcpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await?;
            }
            #[cfg(not(feature = "dhcp"))]
            {
                let _ = status_tx.send("DHCP support not compiled in. Enable 'dhcp' feature.".to_string());
            }
        }
        BaseStack::Ntp => {
            #[cfg(feature = "ntp")]
            {
                use crate::network::NtpServer;
                let state_arc = Arc::new(state.clone());
                NtpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await?;
            }
            #[cfg(not(feature = "ntp"))]
            {
                let _ = status_tx.send("NTP support not compiled in. Enable 'ntp' feature.".to_string());
            }
        }
        BaseStack::Snmp => {
            #[cfg(feature = "snmp")]
            {
                use crate::network::SnmpServer;
                let state_arc = Arc::new(state.clone());
                SnmpServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await?;
            }
            #[cfg(not(feature = "snmp"))]
            {
                let _ = status_tx.send("SNMP support not compiled in. Enable 'snmp' feature.".to_string());
            }
        }
        BaseStack::Ssh => {
            #[cfg(feature = "ssh")]
            {
                use crate::network::SshServer;
                let state_arc = Arc::new(state.clone());
                let send_first = state.get_send_first().await;
                SshServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                    send_first,
                ).await?;
            }
            #[cfg(not(feature = "ssh"))]
            {
                let _ = status_tx.send("SSH support not compiled in. Enable 'ssh' feature.".to_string());
            }
        }
        BaseStack::Irc => {
            #[cfg(feature = "irc")]
            {
                use crate::network::IrcServer;
                let state_arc = Arc::new(state.clone());
                IrcServer::spawn_with_llm_actions(
                    listen_addr,
                    llm_client.clone(),
                    state_arc,
                    status_tx.clone(),
                ).await?;
            }
            #[cfg(not(feature = "irc"))]
            {
                let _ = status_tx.send("IRC support not compiled in. Enable 'irc' feature.".to_string());
            }
        }
    }

    Ok(())
}