//! Server startup logic for TUI mode
//!
//! Handles spawning TCP and HTTP servers based on application state

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::error;

use crate::events::{types::AppEvent, NetworkEvent};
use crate::network::ConnectionId;
use crate::protocol::BaseStack;
use crate::state::app_state::AppState;

type WriteHalfMap = Arc<Mutex<std::collections::HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>;
type CancellationTokenMap = Arc<Mutex<std::collections::HashMap<ConnectionId, oneshot::Sender<()>>>>;

/// Check if server needs to be started and start it
pub async fn check_and_start_server(
    state: &AppState,
    network_tx: &mpsc::UnboundedSender<NetworkEvent>,
    connections: &WriteHalfMap,
    cancellation_tokens: &CancellationTokenMap,
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
                TcpServer::spawn_tui(listen_addr, network_tx.clone(), connections.clone(), cancellation_tokens.clone()).await?;
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
                HttpServer::spawn_tui(listen_addr, network_tx.clone()).await?;
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
                spawn_udp_based_server(listen_addr, network_tx.clone(), "UDP").await?;
            }
            #[cfg(not(feature = "udp"))]
            {
                let _ = status_tx.send("UDP support not compiled in. Enable 'udp' feature.".to_string());
            }
        }
        BaseStack::Dns => {
            #[cfg(feature = "dns")]
            {
                spawn_dns_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "dns"))]
            {
                let _ = status_tx.send("DNS support not compiled in. Enable 'dns' feature.".to_string());
            }
        }
        BaseStack::Dhcp => {
            #[cfg(feature = "dhcp")]
            {
                spawn_dhcp_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "dhcp"))]
            {
                let _ = status_tx.send("DHCP support not compiled in. Enable 'dhcp' feature.".to_string());
            }
        }
        BaseStack::Ntp => {
            #[cfg(feature = "ntp")]
            {
                spawn_ntp_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "ntp"))]
            {
                let _ = status_tx.send("NTP support not compiled in. Enable 'ntp' feature.".to_string());
            }
        }
        BaseStack::Snmp => {
            #[cfg(feature = "snmp")]
            {
                spawn_snmp_agent(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "snmp"))]
            {
                let _ = status_tx.send("SNMP support not compiled in. Enable 'snmp' feature.".to_string());
            }
        }
        BaseStack::Ssh => {
            #[cfg(feature = "ssh")]
            {
                spawn_ssh_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "ssh"))]
            {
                let _ = status_tx.send("SSH support not compiled in. Enable 'ssh' feature.".to_string());
            }
        }
        BaseStack::Irc => {
            #[cfg(feature = "irc")]
            {
                spawn_irc_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "irc"))]
            {
                let _ = status_tx.send("IRC support not compiled in. Enable 'irc' feature.".to_string());
            }
        }
    }

    Ok(())
}

/// Helper function to spawn UDP-based servers with AppEvent adapter
#[cfg(feature = "udp")]
async fn spawn_udp_based_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
    server_name: &str,
) -> Result<()> {
    use crate::network::UdpServer;

    // Create adapter from NetworkEvent to AppEvent
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let network_tx_clone = network_tx.clone();

    // Spawn adapter task to forward AppEvents to NetworkEvents
    tokio::spawn(async move {
        while let Some(event) = app_rx.recv().await {
            if let AppEvent::Network(net_event) = event {
                let _ = network_tx_clone.send(net_event);
            }
        }
    });

    let udp_server = UdpServer::new(listen_addr, app_tx).await?;
    let name = server_name.to_string();

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = udp_server.start().await {
            error!("{} server error: {}", name, e);
        }
    });

    Ok(())
}

/// Helper function to spawn DNS server
#[cfg(feature = "dns")]
async fn spawn_dns_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::network::DnsServer;

    // Create adapter from NetworkEvent to AppEvent
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let network_tx_clone = network_tx.clone();

    // Spawn adapter task to forward AppEvents to NetworkEvents
    tokio::spawn(async move {
        while let Some(event) = app_rx.recv().await {
            if let AppEvent::Network(net_event) = event {
                let _ = network_tx_clone.send(net_event);
            }
        }
    });

    let dns_server = DnsServer::new(listen_addr, app_tx).await?;

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = dns_server.start().await {
            error!("DNS server error: {}", e);
        }
    });

    Ok(())
}

/// Helper function to spawn DHCP server
#[cfg(feature = "dhcp")]
async fn spawn_dhcp_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::network::DhcpServer;

    // Create adapter from NetworkEvent to AppEvent
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let network_tx_clone = network_tx.clone();

    // Spawn adapter task to forward AppEvents to NetworkEvents
    tokio::spawn(async move {
        while let Some(event) = app_rx.recv().await {
            if let AppEvent::Network(net_event) = event {
                let _ = network_tx_clone.send(net_event);
            }
        }
    });

    let dhcp_server = DhcpServer::new(listen_addr, app_tx).await?;

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = dhcp_server.start().await {
            error!("DHCP server error: {}", e);
        }
    });

    Ok(())
}

/// Helper function to spawn NTP server
#[cfg(feature = "ntp")]
async fn spawn_ntp_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::network::NtpServer;

    // Create adapter from NetworkEvent to AppEvent
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let network_tx_clone = network_tx.clone();

    // Spawn adapter task to forward AppEvents to NetworkEvents
    tokio::spawn(async move {
        while let Some(event) = app_rx.recv().await {
            if let AppEvent::Network(net_event) = event {
                let _ = network_tx_clone.send(net_event);
            }
        }
    });

    let ntp_server = NtpServer::new(listen_addr, app_tx).await?;

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = ntp_server.start().await {
            error!("NTP server error: {}", e);
        }
    });

    Ok(())
}

/// Helper function to spawn SNMP agent
#[cfg(feature = "snmp")]
async fn spawn_snmp_agent(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::network::SnmpServer;

    // Create adapter from NetworkEvent to AppEvent
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let network_tx_clone = network_tx.clone();

    // Spawn adapter task to forward AppEvents to NetworkEvents
    tokio::spawn(async move {
        while let Some(event) = app_rx.recv().await {
            if let AppEvent::Network(net_event) = event {
                let _ = network_tx_clone.send(net_event);
            }
        }
    });

    let snmp_server = SnmpServer::new(listen_addr, app_tx).await?;

    // Spawn agent loop
    tokio::spawn(async move {
        if let Err(e) = snmp_server.start().await {
            error!("SNMP agent error: {}", e);
        }
    });

    Ok(())
}

/// Helper function to spawn SSH server
#[cfg(feature = "ssh")]
async fn spawn_ssh_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::network::SshServer;

    // Create adapter from NetworkEvent to AppEvent
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let network_tx_clone = network_tx.clone();

    // Spawn adapter task to forward AppEvents to NetworkEvents
    tokio::spawn(async move {
        while let Some(event) = app_rx.recv().await {
            if let AppEvent::Network(net_event) = event {
                let _ = network_tx_clone.send(net_event);
            }
        }
    });

    let ssh_server = SshServer::new(listen_addr, app_tx).await?;

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = ssh_server.start().await {
            error!("SSH server error: {}", e);
        }
    });

    Ok(())
}

/// Helper function to spawn IRC server
#[cfg(feature = "irc")]
async fn spawn_irc_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::network::IrcServer;

    // Create adapter from NetworkEvent to AppEvent
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let network_tx_clone = network_tx.clone();

    // Spawn adapter task to forward AppEvents to NetworkEvents
    tokio::spawn(async move {
        while let Some(event) = app_rx.recv().await {
            if let AppEvent::Network(net_event) = event {
                let _ = network_tx_clone.send(net_event);
            }
        }
    });

    let irc_server = IrcServer::new(listen_addr, app_tx).await?;

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = irc_server.start().await {
            error!("IRC server error: {}", e);
        }
    });

    Ok(())
}