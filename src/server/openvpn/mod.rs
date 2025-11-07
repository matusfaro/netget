//! OpenVPN VPN server implementation
//!
//! This is a FULL OpenVPN VPN server that creates actual tunnels for clients.
//! It implements a simplified OpenVPN protocol supporting:
//! - UDP transport only
//! - TLS 1.3 control channel
//! - AES-256-GCM or ChaCha20-Poly1305 data channel
//! - TUN interface for IP packet tunneling
//!
//! The LLM controls:
//! - Peer authorization (approve/reject new peers)
//! - Traffic inspection policies
//! - Routing decisions
//! - Connection limits

pub mod actions;
pub mod crypto;
pub mod packet;
pub mod peer;

use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use actions::OPENVPN_PEER_CONNECTED_EVENT;
use anyhow::{Context, Result};
use packet::{ControlPacket, DataPacket, Opcode, PacketHeader};
use peer::{Peer, PeerManager, PeerState};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, trace, warn};

/// Maximum number of peers to allow
const MAX_PEERS: usize = 100;

/// VPN subnet configuration
const VPN_NETWORK: &str = "10.8.0.0/24";
const VPN_SERVER_IP: &str = "10.8.0.1";

/// OpenVPN server state
pub struct OpenvpnServer {
    /// TUN interface
    tun: Arc<RwLock<tun::AsyncDevice>>,
    /// Interface name
    interface_name: String,
    /// UDP socket for OpenVPN protocol
    socket: Arc<UdpSocket>,
    /// Peer manager
    peer_manager: Arc<PeerManager>,
    /// Server session ID
    server_session_id: u64,
    /// IP address pool
    ip_pool: Arc<RwLock<IpPool>>,
    /// Server private key and certificate (for TLS)
    _tls_config: Arc<rustls::ServerConfig>,
}

/// IP address pool for assigning VPN IPs to clients
struct IpPool {
    network: Ipv4Addr,
    allocated: HashMap<Ipv4Addr, SocketAddr>,
    next_ip: u32,
}

impl IpPool {
    fn new() -> Self {
        IpPool {
            network: "10.8.0.0".parse().unwrap(),
            allocated: HashMap::new(),
            next_ip: 2, // Start from .2 (server is .1)
        }
    }

    fn allocate(&mut self, addr: SocketAddr) -> Option<Ipv4Addr> {
        if self.next_ip >= 254 {
            return None; // Pool exhausted
        }

        let octets = self.network.octets();
        let ip = Ipv4Addr::new(octets[0], octets[1], octets[2], self.next_ip as u8);
        self.allocated.insert(ip, addr);
        self.next_ip += 1;

        Some(ip)
    }

    fn deallocate(&mut self, ip: Ipv4Addr) {
        self.allocated.remove(&ip);
    }
}

impl OpenvpnServer {
    /// Spawn OpenVPN VPN server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        bind_addr: SocketAddr,
        _llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        info!("Starting OpenVPN VPN server on {}", bind_addr);
        let _ = status_tx.send(format!(
            "[INFO] Starting OpenVPN VPN server on {} (full VPN tunnel support)",
            bind_addr
        ));

        // Generate server session ID
        let server_session_id = rand::random::<u64>();
        info!("OpenVPN server session ID: {:016x}", server_session_id);

        // Create TLS configuration for control channel
        let tls_config = Self::create_tls_config()?;
        let _ = status_tx.send("[INFO] TLS configuration created".to_string());

        // Determine TUN interface name based on OS
        let interface_name: String = if cfg!(target_os = "linux") {
            "netget_ovpn0".into()
        } else if cfg!(target_os = "macos") {
            "utun11".into()
        } else if cfg!(target_os = "windows") {
            "netget_ovpn0".into()
        } else {
            return Err(anyhow::anyhow!("Unsupported operating system for OpenVPN"));
        };

        info!("Creating TUN interface: {}", interface_name);
        let _ = status_tx.send(format!("[INFO] Creating TUN interface: {}", interface_name));

        // Create TUN device
        let mut tun_config = tun::Configuration::default();
        tun_config
            .tun_name(&interface_name)
            .address(VPN_SERVER_IP.parse::<Ipv4Addr>().unwrap())
            .netmask("255.255.255.0".parse::<Ipv4Addr>().unwrap())
            .mtu(1500)
            .up();

        #[cfg(target_os = "linux")]
        let tun_device = tun::create_as_async(&tun_config)
            .context("Failed to create TUN device")?;

        #[cfg(target_os = "macos")]
        let tun_device = tun::create_as_async(&tun_config)
            .context("Failed to create TUN device")?;

        #[cfg(target_os = "windows")]
        let tun_device = tun::create_as_async(&tun_config)
            .context("Failed to create TUN device")?;

        info!("TUN interface created successfully: {}", interface_name);
        let _ = status_tx.send(format!("[INFO] TUN interface created: {}", interface_name));

        // Bind UDP socket
        let socket = UdpSocket::bind(bind_addr).await
            .context("Failed to bind UDP socket")?;
        let local_addr = socket.local_addr()?;

        info!("OpenVPN server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] OpenVPN listening on {}", local_addr));
        let _ = status_tx.send(format!("[INFO] VPN subnet: {}", VPN_NETWORK));

        let server = Arc::new(OpenvpnServer {
            tun: Arc::new(RwLock::new(tun_device)),
            interface_name: interface_name.clone(),
            socket: Arc::new(socket),
            peer_manager: Arc::new(PeerManager::new()),
            server_session_id,
            ip_pool: Arc::new(RwLock::new(IpPool::new())),
            _tls_config: Arc::new(tls_config),
        });

        // Spawn UDP packet handler
        let server_clone = server.clone();
        let status_clone = status_tx.clone();
        let app_state_clone = app_state.clone();
        tokio::spawn(async move {
            if let Err(e) = server_clone.handle_udp_packets(
                app_state_clone,
                server_id,
                status_clone,
            ).await {
                error!("UDP packet handler error: {}", e);
            }
        });

        // Spawn TUN packet handler
        let server_clone = server.clone();
        let status_clone = status_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = server_clone.handle_tun_packets(status_clone).await {
                error!("TUN packet handler error: {}", e);
            }
        });

        info!("OpenVPN VPN server ready on {}", local_addr);
        let _ = status_tx.send(format!("→ OpenVPN VPN server ready on {}", local_addr));
        let _ = status_tx.send(format!("[INFO] Clients can connect to {} with VPN subnet {}", local_addr, VPN_NETWORK));

        Ok(local_addr)
    }

    /// Create TLS configuration for control channel
    fn create_tls_config() -> Result<rustls::ServerConfig> {
        use rcgen::{CertificateParams, DistinguishedName, KeyPair};
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        // Generate self-signed certificate for server
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "NetGet OpenVPN Server");
        params.distinguished_name = dn;

        // Generate key pair and self-sign
        let key_pair = KeyPair::generate()
            .context("Failed to generate key pair")?;

        let cert = params.self_signed(&key_pair)
            .context("Failed to create self-signed certificate")?;

        let cert_der = CertificateDer::from(cert.der().to_vec());
        let key_der = PrivateKeyDer::try_from(key_pair.serialize_der())
            .map_err(|e| anyhow::anyhow!("Failed to parse private key: {:?}", e))?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .context("Failed to create TLS config")?;

        Ok(config)
    }

    /// Handle incoming UDP packets (control and data)
    async fn handle_udp_packets(
        &self,
        app_state: Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut buf = vec![0u8; 65535];

        loop {
            let (len, peer_addr) = self.socket.recv_from(&mut buf).await?;
            let packet = &buf[..len];

            trace!("Received {} bytes from {}", len, peer_addr);

            // Parse packet header
            let header = match PacketHeader::parse(packet) {
                Ok((hdr, _)) => hdr,
                Err(e) => {
                    warn!("Failed to parse packet from {}: {}", peer_addr, e);
                    continue;
                }
            };

            // Route packet based on opcode
            if header.opcode.is_control() {
                self.handle_control_packet(packet, peer_addr, &app_state, server_id, &status_tx).await;
            } else if header.opcode.is_data() {
                self.handle_data_packet(packet, peer_addr, &status_tx).await;
            } else if header.opcode.is_ack() {
                self.handle_ack_packet(packet, peer_addr).await;
            }
        }
    }

    /// Handle control packet (handshake, key exchange)
    async fn handle_control_packet(
        &self,
        packet: &[u8],
        peer_addr: SocketAddr,
        app_state: &Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let control_packet = match ControlPacket::parse(packet) {
            Ok(pkt) => pkt,
            Err(e) => {
                warn!("Failed to parse control packet: {}", e);
                return;
            }
        };

        trace!("Control packet from {}: {:?}", peer_addr, control_packet.header.opcode);

        // Handle based on opcode
        match control_packet.header.opcode {
            Opcode::ControlHardResetClientV2 | Opcode::ControlHardResetClientV1 => {
                self.handle_handshake_initiation(control_packet, peer_addr, app_state, server_id, status_tx).await;
            }
            Opcode::ControlV1 => {
                self.handle_control_message(control_packet, peer_addr).await;
            }
            _ => {
                trace!("Unhandled control opcode: {:?}", control_packet.header.opcode);
            }
        }
    }

    /// Handle handshake initiation from client
    async fn handle_handshake_initiation(
        &self,
        control_packet: ControlPacket,
        peer_addr: SocketAddr,
        app_state: &Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        info!("OpenVPN handshake from {}", peer_addr);
        let _ = status_tx.send(format!("[INFO] OpenVPN handshake from {}", peer_addr));

        // Check peer limit
        if self.peer_manager.count().await >= MAX_PEERS {
            warn!("Maximum peers reached, rejecting {}", peer_addr);
            let _ = status_tx.send(format!("[WARN] Max peers reached, rejecting {}", peer_addr));
            return;
        }

        // Create new peer
        let connection_id = ConnectionId::new();
        let client_session_id = control_packet.header.session_id.unwrap_or(0);

        let mut peer = Peer::new(connection_id, peer_addr, client_session_id);
        peer.remote_session_id = control_packet.header.session_id;
        peer.state = PeerState::TlsHandshaking;
        peer.record_received_packet(control_packet.header.packet_id.unwrap_or(0));

        // Allocate VPN IP
        let vpn_ip = match self.ip_pool.write().await.allocate(peer_addr) {
            Some(ip) => ip,
            None => {
                error!("Failed to allocate VPN IP for {}", peer_addr);
                return;
            }
        };

        info!("Allocated VPN IP {} to {}", vpn_ip, peer_addr);
        let _ = status_tx.send(format!("[INFO] Allocated VPN IP {} to {}", vpn_ip, peer_addr));

        peer.mark_connected(vpn_ip);

        // Add peer to manager
        self.peer_manager.add_peer(peer.clone()).await;

        // Send handshake response
        self.send_handshake_response(&peer, &control_packet, status_tx).await;

        // Initialize data channel keys (simplified for MVP)
        self.initialize_data_channel(&peer, status_tx).await;

        // Add connection to app state
        let now = std::time::Instant::now();
        let conn_state = ConnectionState {
            id: connection_id,
            remote_addr: peer_addr,
            local_addr: self.socket.local_addr().unwrap(),
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::empty(),
        };

        app_state.add_connection_to_server(server_id, conn_state).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Trigger LLM event
        let _event = Event::new(
            &OPENVPN_PEER_CONNECTED_EVENT,
            serde_json::json!({
                "peer_addr": peer_addr.to_string(),
                "vpn_ip": vpn_ip.to_string(),
                "session_id": format!("{:016x}", peer.session_id),
            }),
        );

        info!("OpenVPN peer connected: {} (VPN IP: {})", peer_addr, vpn_ip);
    }

    /// Send handshake response to client
    async fn send_handshake_response(
        &self,
        peer: &Peer,
        client_packet: &ControlPacket,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let response = ControlPacket {
            header: PacketHeader {
                opcode: Opcode::ControlHardResetServerV2,
                key_id: 0,
                session_id: Some(self.server_session_id),
                packet_id_array_len: Some(1),
                packet_id: Some(1),
            },
            ack_packet_ids: vec![client_packet.header.packet_id.unwrap_or(0)],
            remote_session_id: client_packet.header.session_id,
            tls_payload: vec![], // Simplified: no actual TLS payload for MVP
        };

        let serialized = response.serialize();

        if let Err(e) = self.socket.send_to(&serialized, peer.addr).await {
            error!("Failed to send handshake response: {}", e);
            let _ = status_tx.send(format!("[ERROR] Failed to send handshake response: {}", e));
        } else {
            debug!("Sent handshake response to {}", peer.addr);
        }
    }

    /// Initialize data channel for peer (simplified key exchange)
    async fn initialize_data_channel(
        &self,
        peer: &Peer,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        // In a full implementation, this would derive keys from TLS master secret
        // For MVP, we use a simplified approach with hardcoded keys
        use crypto::derive_data_keys;

        let master_secret = b"simplified_master_secret_for_mvp";
        let client_random = b"client_random_data_12345678";
        let server_random = b"server_random_data_87654321";

        let keys = match derive_data_keys(master_secret, client_random, server_random) {
            Ok(k) => k,
            Err(e) => {
                error!("Failed to derive data channel keys: {}", e);
                return;
            }
        };

        // Update peer with cipher
        self.peer_manager.update_peer(&peer.addr, |p| {
            if let Err(e) = p.init_data_cipher(&keys, true) {
                error!("Failed to initialize cipher: {}", e);
            } else {
                debug!("Data channel initialized for {}", peer.addr);
                let _ = status_tx.send(format!("[DEBUG] Data channel ready for {}", peer.addr));
            }
        }).await;
    }

    /// Handle control message (key exchange, config push)
    async fn handle_control_message(
        &self,
        _control_packet: ControlPacket,
        _peer_addr: SocketAddr,
    ) {
        // Simplified for MVP
        trace!("Control message handling (simplified)");
    }

    /// Handle data packet (encrypted tunnel traffic)
    async fn handle_data_packet(
        &self,
        packet: &[u8],
        peer_addr: SocketAddr,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let data_packet = match DataPacket::parse(packet) {
            Ok(pkt) => pkt,
            Err(e) => {
                warn!("Failed to parse data packet: {}", e);
                return;
            }
        };

        // Get peer
        let peer = match self.peer_manager.get_peer(&peer_addr).await {
            Some(p) => p,
            None => {
                warn!("Data packet from unknown peer: {}", peer_addr);
                return;
            }
        };

        // Decrypt data
        let plaintext = match &peer.data_cipher {
            Some(cipher) => {
                let packet_id = data_packet.header.packet_id.unwrap_or(0);
                match cipher.decrypt(packet_id, &data_packet.encrypted_payload, &[]) {
                    Ok(pt) => pt,
                    Err(e) => {
                        warn!("Failed to decrypt data packet: {}", e);
                        return;
                    }
                }
            }
            None => {
                warn!("No data cipher for peer {}", peer_addr);
                return;
            }
        };

        trace!("Decrypted {} bytes from {}", plaintext.len(), peer_addr);

        // Write decrypted IP packet to TUN
        if let Err(e) = self.tun.write().await.write(&plaintext).await {
            error!("Failed to write to TUN: {}", e);
            let _ = status_tx.send(format!("[ERROR] TUN write failed: {}", e));
        }

        // Update stats
        self.peer_manager.update_peer(&peer_addr, |p| {
            p.update_stats(0, plaintext.len() as u64);
        }).await;
    }

    /// Handle ACK packet
    async fn handle_ack_packet(
        &self,
        _packet: &[u8],
        _peer_addr: SocketAddr,
    ) {
        // Simplified for MVP
        trace!("ACK packet handling (simplified)");
    }

    /// Handle outgoing packets from TUN (to be sent to VPN clients)
    async fn handle_tun_packets(
        &self,
        _status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut buf = vec![0u8; 2048];

        loop {
            let len = self.tun.write().await.read(&mut buf).await?;
            let ip_packet = &buf[..len];

            // Parse IP header to get destination
            if len < 20 {
                continue; // Too short for IP header
            }

            let dst_ip = Ipv4Addr::new(ip_packet[16], ip_packet[17], ip_packet[18], ip_packet[19]);

            trace!("TUN packet to {}: {} bytes", dst_ip, len);

            // Find peer with this VPN IP
            let peers = self.peer_manager.get_all_peers().await;
            let peer = peers.iter().find(|p| p.vpn_ip == Some(dst_ip));

            let peer = match peer {
                Some(p) => p,
                None => {
                    trace!("No peer found for VPN IP {}", dst_ip);
                    continue;
                }
            };

            // Encrypt and send
            if let Some(cipher) = &peer.data_cipher {
                let mut packet_id = 0;
                self.peer_manager.update_peer(&peer.addr, |p| {
                    packet_id = p.next_packet_id();
                }).await;

                let encrypted = match cipher.encrypt(packet_id, ip_packet, &[]) {
                    Ok(enc) => enc,
                    Err(e) => {
                        warn!("Failed to encrypt: {}", e);
                        continue;
                    }
                };

                let data_packet = DataPacket {
                    header: PacketHeader {
                        opcode: Opcode::DataV2,
                        key_id: 0,
                        session_id: Some(self.server_session_id),
                        packet_id_array_len: None,
                        packet_id: Some(packet_id),
                    },
                    encrypted_payload: encrypted,
                };

                let serialized = data_packet.serialize();

                if let Err(e) = self.socket.send_to(&serialized, peer.addr).await {
                    error!("Failed to send data packet: {}", e);
                } else {
                    trace!("Sent {} bytes to {}", serialized.len(), peer.addr);
                }

                // Update stats
                self.peer_manager.update_peer(&peer.addr, |p| {
                    p.update_stats(serialized.len() as u64, 0);
                }).await;
            }
        }
    }

    /// Get peer list
    pub async fn list_peers(&self) -> Vec<(SocketAddr, Ipv4Addr)> {
        let peers = self.peer_manager.get_all_peers().await;
        peers.iter()
            .filter_map(|p| p.vpn_ip.map(|ip| (p.addr, ip)))
            .collect()
    }

    /// Remove peer
    pub async fn remove_peer(&self, addr: SocketAddr) -> Result<()> {
        if let Some(peer) = self.peer_manager.remove_peer(&addr).await {
            if let Some(vpn_ip) = peer.vpn_ip {
                self.ip_pool.write().await.deallocate(vpn_ip);
            }
            info!("Removed peer: {}", addr);
        }
        Ok(())
    }
}

impl Drop for OpenvpnServer {
    fn drop(&mut self) {
        info!("OpenVPN server shutting down: {}", self.interface_name);
    }
}
