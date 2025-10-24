//! Network context for protocol actions
//!
//! This module defines the context objects passed to protocol-specific
//! actions during network events. Each context contains all the information
//! needed to execute sync actions (actions that require network context).

use crate::network::connection::ConnectionId;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};

/// Network event context for protocol-specific actions
///
/// This enum contains all the contextual information needed to execute
/// sync actions (actions that only make sense in response to network events).
#[derive(Clone)]
pub enum NetworkContext {
    /// TCP connection context - for connection-oriented protocols
    TcpConnection {
        connection_id: ConnectionId,
        write_half: Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// UDP datagram context - for connectionless protocols
    UdpDatagram {
        peer_addr: SocketAddr,
        socket: Arc<UdpSocket>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// HTTP request context - for HTTP protocol
    HttpRequest {
        connection_id: ConnectionId,
        method: String,
        uri: String,
        headers: HashMap<String, String>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// SNMP request context - for SNMP protocol
    SnmpRequest {
        peer_addr: SocketAddr,
        socket: Arc<UdpSocket>,
        version: u8,
        request_id: i32,
        community: Vec<u8>,
        requested_oids: Vec<String>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// DNS query context - for DNS protocol
    DnsQuery {
        peer_addr: SocketAddr,
        socket: Arc<UdpSocket>,
        query_data: Vec<u8>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// DHCP request context - for DHCP protocol
    DhcpRequest {
        peer_addr: SocketAddr,
        socket: Arc<UdpSocket>,
        request_data: Vec<u8>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// NTP request context - for NTP protocol
    NtpRequest {
        peer_addr: SocketAddr,
        socket: Arc<UdpSocket>,
        request_data: Vec<u8>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// SSH connection context - for SSH protocol
    SshConnection {
        connection_id: ConnectionId,
        write_half: Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// SFTP request context - for SFTP subsystem (runs over SSH)
    SftpRequest {
        connection_id: ConnectionId,
        request_id: u32,
        operation: String,  // e.g., "opendir", "read", "write", "stat"
        path: Option<String>,
        handle: Option<String>,
        status_tx: mpsc::UnboundedSender<String>,
    },

    /// IRC connection context - for IRC protocol
    IrcConnection {
        connection_id: ConnectionId,
        write_half: Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
        status_tx: mpsc::UnboundedSender<String>,
    },
}

impl NetworkContext {
    /// Get a human-readable description of this context
    pub fn description(&self) -> String {
        match self {
            NetworkContext::TcpConnection { connection_id, .. } =>
                format!("TCP connection {}", connection_id),
            NetworkContext::UdpDatagram { peer_addr, .. } =>
                format!("UDP datagram from {}", peer_addr),
            NetworkContext::HttpRequest { method, uri, .. } =>
                format!("HTTP {} {}", method, uri),
            NetworkContext::SnmpRequest { peer_addr, requested_oids, .. } =>
                format!("SNMP request from {} for OIDs: {}", peer_addr, requested_oids.join(", ")),
            NetworkContext::DnsQuery { peer_addr, .. } =>
                format!("DNS query from {}", peer_addr),
            NetworkContext::DhcpRequest { peer_addr, .. } =>
                format!("DHCP request from {}", peer_addr),
            NetworkContext::NtpRequest { peer_addr, .. } =>
                format!("NTP request from {}", peer_addr),
            NetworkContext::SshConnection { connection_id, .. } =>
                format!("SSH connection {}", connection_id),
            NetworkContext::SftpRequest { connection_id, operation, path, .. } =>
                format!("SFTP {} on connection {} (path: {})",
                    operation, connection_id, path.as_deref().unwrap_or("N/A")),
            NetworkContext::IrcConnection { connection_id, .. } =>
                format!("IRC connection {}", connection_id),
        }
    }
}
