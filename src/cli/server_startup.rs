//! Server startup logic for TUI mode
//!
//! Handles spawning TCP and HTTP servers based on application state

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::events::NetworkEvent;
use crate::network::ConnectionId;
#[cfg(feature = "http")]
use crate::network::HttpServer;
#[cfg(feature = "tcp")]
use crate::network::TcpServer;
#[cfg(feature = "udp")]
use crate::network::UdpServer;
#[cfg(feature = "dns")]
use crate::network::DnsServer;
#[cfg(feature = "dhcp")]
use crate::network::DhcpServer;
#[cfg(feature = "ntp")]
use crate::network::NtpServer;
#[cfg(feature = "snmp")]
use crate::network::SnmpAgent;
#[cfg(feature = "ssh")]
use crate::network::SshServer;
#[cfg(feature = "irc")]
use crate::network::IrcServer;
use crate::protocol::BaseStack;
use crate::state::app_state::AppState;

type WriteHalfMap = Arc<Mutex<std::collections::HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>;

/// Start a TCP server and spawn accept loop
type CancellationTokenMap = Arc<Mutex<std::collections::HashMap<crate::network::ConnectionId, tokio::sync::oneshot::Sender<()>>>>;

#[cfg(feature = "tcp")]
pub async fn start_tcp_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
    connections: WriteHalfMap,
    cancellation_tokens: &CancellationTokenMap,
) -> Result<()> {
    // Create and bind TCP server
    let mut tcp_server = TcpServer::new(network_tx.clone());
    tcp_server.listen(listen_addr).await?;

    // Send listening event
    let _ = network_tx.send(NetworkEvent::Listening { addr: listen_addr });

    // Clone for the spawned task
    let cancellation_tokens = cancellation_tokens.clone();

    // Spawn accept loop
    tokio::spawn(async move {
        loop {
            match tcp_server.accept().await {
                Ok(Some((stream, remote_addr))) => {
                    let connection_id = ConnectionId::new();

                    // Split stream
                    let (read_half, write_half) = tokio::io::split(stream);
                    let write_half_arc = Arc::new(Mutex::new(write_half));
                    connections.lock().await.insert(connection_id, write_half_arc);

                    // Create cancellation channel for this connection
                    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
                    cancellation_tokens.lock().await.insert(connection_id, cancel_tx);

                    // Send connected event
                    let _ = network_tx.send(NetworkEvent::Connected {
                        connection_id,
                        remote_addr,
                    });

                    // Spawn reader task with cancellation
                    let network_tx_inner = network_tx.clone();
                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut buffer = vec![0u8; 8192];
                        let mut read_half = read_half;

                        loop {
                            tokio::select! {
                                // Check for cancellation
                                _ = &mut cancel_rx => {
                                    // Connection was explicitly closed
                                    let _ = network_tx_inner.send(NetworkEvent::Disconnected { connection_id });
                                    break;
                                }
                                // Read data
                                result = read_half.read(&mut buffer) => {
                                    match result {
                                        Ok(0) => {
                                            let _ = network_tx_inner.send(NetworkEvent::Disconnected { connection_id });
                                            break;
                                        }
                                        Ok(n) => {
                                            let data = bytes::Bytes::copy_from_slice(&buffer[..n]);
                                            let _ = network_tx_inner.send(NetworkEvent::DataReceived {
                                                connection_id,
                                                data,
                                            });
                                        }
                                        Err(_) => break,
                                    }
                                }
                            }
                        }
                    });
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });

    Ok(())
}

/// Start an HTTP server
#[cfg(feature = "http")]
pub async fn start_http_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    let http_server = HttpServer::new(listen_addr, network_tx.clone()).await?;

    // Send listening event
    let _ = network_tx.send(NetworkEvent::Listening { addr: listen_addr });

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = http_server.accept_loop().await {
            eprintln!("HTTP server error: {}", e);
        }
    });

    Ok(())
}

/// Start a UDP server
#[cfg(feature = "udp")]
pub async fn start_udp_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::events::types::AppEvent;

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

    // Spawn server loop
    tokio::spawn(async move {
        if let Err(e) = udp_server.start().await {
            eprintln!("UDP server error: {}", e);
        }
    });

    Ok(())
}

/// Start a DNS server
#[cfg(feature = "dns")]
pub async fn start_dns_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::events::types::AppEvent;

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
            eprintln!("DNS server error: {}", e);
        }
    });

    Ok(())
}

/// Start a DHCP server
#[cfg(feature = "dhcp")]
pub async fn start_dhcp_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::events::types::AppEvent;

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
            eprintln!("DHCP server error: {}", e);
        }
    });

    Ok(())
}

/// Start an NTP server
#[cfg(feature = "ntp")]
pub async fn start_ntp_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::events::types::AppEvent;

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
            eprintln!("NTP server error: {}", e);
        }
    });

    Ok(())
}

/// Start an SNMP agent
#[cfg(feature = "snmp")]
pub async fn start_snmp_agent(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::events::types::AppEvent;

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

    let snmp_agent = SnmpAgent::new(listen_addr, app_tx).await?;

    // Spawn agent loop
    tokio::spawn(async move {
        if let Err(e) = snmp_agent.start().await {
            eprintln!("SNMP agent error: {}", e);
        }
    });

    Ok(())
}

/// Start an SSH server
#[cfg(feature = "ssh")]
pub async fn start_ssh_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::events::types::AppEvent;

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
            eprintln!("SSH server error: {}", e);
        }
    });

    Ok(())
}

/// Start an IRC server
#[cfg(feature = "irc")]
pub async fn start_irc_server(
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    use crate::events::types::AppEvent;

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
            eprintln!("IRC server error: {}", e);
        }
    });

    Ok(())
}

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
                start_tcp_server(listen_addr, network_tx.clone(), connections.clone(), cancellation_tokens).await?;
            }
            #[cfg(not(feature = "tcp"))]
            {
                let _ = status_tx.send("TCP support not compiled in. Enable 'tcp' feature.".to_string());
            }
        }
        BaseStack::Http => {
            #[cfg(feature = "http")]
            {
                start_http_server(listen_addr, network_tx.clone()).await?;
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
                start_udp_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "udp"))]
            {
                let _ = status_tx.send("UDP support not compiled in. Enable 'udp' feature.".to_string());
            }
        }
        BaseStack::Dns => {
            #[cfg(feature = "dns")]
            {
                start_dns_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "dns"))]
            {
                let _ = status_tx.send("DNS support not compiled in. Enable 'dns' feature.".to_string());
            }
        }
        BaseStack::Dhcp => {
            #[cfg(feature = "dhcp")]
            {
                start_dhcp_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "dhcp"))]
            {
                let _ = status_tx.send("DHCP support not compiled in. Enable 'dhcp' feature.".to_string());
            }
        }
        BaseStack::Ntp => {
            #[cfg(feature = "ntp")]
            {
                start_ntp_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "ntp"))]
            {
                let _ = status_tx.send("NTP support not compiled in. Enable 'ntp' feature.".to_string());
            }
        }
        BaseStack::Snmp => {
            #[cfg(feature = "snmp")]
            {
                start_snmp_agent(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "snmp"))]
            {
                let _ = status_tx.send("SNMP support not compiled in. Enable 'snmp' feature.".to_string());
            }
        }
        BaseStack::Ssh => {
            #[cfg(feature = "ssh")]
            {
                start_ssh_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "ssh"))]
            {
                let _ = status_tx.send("SSH support not compiled in. Enable 'ssh' feature.".to_string());
            }
        }
        BaseStack::Irc => {
            #[cfg(feature = "irc")]
            {
                start_irc_server(listen_addr, network_tx.clone()).await?;
            }
            #[cfg(not(feature = "irc"))]
            {
                let _ = status_tx.send("IRC support not compiled in. Enable 'irc' feature.".to_string());
            }
        }
    }

    Ok(())
}

