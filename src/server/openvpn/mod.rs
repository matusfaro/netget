//! OpenVPN-shaped tunnel server - **INCOMPLETE AND CRYPTOGRAPHICALLY INSECURE**
//!
//! # ⚠️ Read this before deploying anything
//!
//! This is **not** a working OpenVPN server and **must not be used to carry real
//! traffic**. It speaks enough of the OpenVPN packet format to look like one, but
//! the security-critical half of the protocol is missing:
//!
//! - **The TLS control channel does not exist.** `create_tls_config()` builds a
//!   `rustls::ServerConfig` that is stored and never used. No TLS handshake is
//!   ever performed, no client certificate is ever verified, and no peer is ever
//!   authenticated. Any host that sends a HARD_RESET is accepted.
//! - **The data-channel keys are hardcoded constants.** `initialize_data_channel()`
//!   derives every peer's AES-256-GCM / ChaCha20-Poly1305 key with HKDF over the
//!   fixed literals `b"simplified_master_secret_for_mvp"` and fixed client/server
//!   "random" values. There is no per-session entropy, so **every peer on every
//!   run of every installation derives the identical key**. The keys are in this
//!   source file and in the public git history. The encryption provides no
//!   confidentiality whatsoever against anyone who can read this repository.
//! - **`handle_control_message()` and `handle_ack_packet()` are empty stubs**, so
//!   control-channel reliability, rekeying and configuration push do not work.
//!
//! What *is* genuinely implemented is the data plane mechanics: OpenVPN V1/V2
//! packet parsing and serialization, a TUN interface, an IP address pool, per-peer
//! packet-ID sequencing, and real AEAD encrypt/decrypt calls. Those are correct
//! code operating on worthless keys.
//!
//! Because of this the protocol is marked [`DevelopmentState::Incomplete`], which
//! hides it from the LLM entirely (see
//! `ProtocolMetadataV2::is_available_to_llm`). It is retained as a packet-format
//! testbed and as a honeypot that speaks plausible OpenVPN, not as a VPN.
//!
//! [`DevelopmentState::Incomplete`]: crate::protocol::metadata::DevelopmentState::Incomplete
//!
//! # Future work (large, not a patch)
//!
//! Making this real means implementing the full OpenVPN TLS control channel:
//! wrapping a genuine TLS 1.3 session over the reliability layer, exchanging key
//! material through it, deriving per-session keys from the negotiated master
//! secret via the OpenVPN PRF, verifying client certificates, and implementing
//! control-channel retransmission and rekeying. That is a project in its own
//! right and is deliberately out of scope here.

pub mod actions;
pub mod crypto;
pub mod packet;
pub mod peer;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use crate::{console_error, console_info};
use actions::{OpenvpnProtocol, OPENVPN_PEER_CONNECTED_EVENT};
use anyhow::{Context, Result};
use packet::{ControlPacket, DataPacket, Opcode, PacketHeader};
use peer::{Peer, PeerManager, PeerState};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, trace, warn};

/// Maximum number of peers to allow
const MAX_PEERS: usize = 100;

/// VPN subnet configuration
const VPN_NETWORK: &str = "10.8.0.0/24";
const VPN_SERVER_IP: &str = "10.8.0.1";

/// OpenVPN server state
pub struct OpenvpnServer {
    /// Write half of the TUN interface.
    ///
    /// The device is split so the read loop never holds a lock while parked in
    /// `read()`. Holding a single `RwLock<AsyncDevice>` across the read await
    /// deadlocked every client-to-TUN write for as long as the TUN was idle.
    tun_writer: Arc<Mutex<WriteHalf<tun::AsyncDevice>>>,
    /// Interface name
    interface_name: String,
    /// UDP socket for OpenVPN protocol
    socket: Arc<UdpSocket>,
    /// Local address the UDP socket is bound to
    local_addr: SocketAddr,
    /// Peer manager
    peer_manager: Arc<PeerManager>,
    /// Server session ID
    server_session_id: u64,
    /// IP address pool
    ip_pool: Arc<RwLock<IpPool>>,
    /// Server certificate config. Built at startup and **never used** - no TLS
    /// handshake is performed. Retained so the eventual real control channel has
    /// something to attach to. See the module docs.
    _tls_config: Arc<rustls::ServerConfig>,
    /// LLM client used to raise peer events
    llm_client: Arc<OllamaClient>,
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
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        info!("Starting OpenVPN-shaped tunnel server on {}", bind_addr);
        warn!(
            "OpenVPN server is INCOMPLETE: no TLS handshake is performed and all peers share \
             hardcoded data-channel keys. Do not carry real traffic over it."
        );
        let _ = status_tx.send(format!(
            "[INFO] Starting OpenVPN server on {} (INCOMPLETE - see below)",
            bind_addr
        ));
        let _ = status_tx.send(
            "[WARN] OpenVPN is INSECURE and INCOMPLETE: no TLS handshake, no peer \
             authentication, and every peer derives the same hardcoded encryption key. \
             Use WireGuard for a real VPN."
                .to_string(),
        );

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

        console_info!(status_tx, "Creating TUN interface: {}", interface_name);

        // Create TUN device
        let mut tun_config = tun::Configuration::default();
        tun_config
            .tun_name(&interface_name)
            .address(VPN_SERVER_IP.parse::<Ipv4Addr>().unwrap())
            .netmask("255.255.255.0".parse::<Ipv4Addr>().unwrap())
            .mtu(1500)
            .up();

        #[cfg(target_os = "linux")]
        let tun_device =
            tun::create_as_async(&tun_config).context("Failed to create TUN device")?;

        #[cfg(target_os = "macos")]
        let tun_device =
            tun::create_as_async(&tun_config).context("Failed to create TUN device")?;

        #[cfg(target_os = "windows")]
        let tun_device =
            tun::create_as_async(&tun_config).context("Failed to create TUN device")?;

        info!("TUN interface created successfully: {}", interface_name);
        let _ = status_tx.send(format!("[INFO] TUN interface created: {}", interface_name));

        // Bind UDP socket
        let socket = UdpSocket::bind(bind_addr)
            .await
            .context("Failed to bind UDP socket")?;
        let local_addr = socket.local_addr()?;

        info!("OpenVPN server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] OpenVPN listening on {}", local_addr));
        let _ = status_tx.send(format!("[INFO] VPN subnet: {}", VPN_NETWORK));

        // Split the TUN device so the read loop does not hold a lock across its
        // blocking read (which would starve every write to the tunnel).
        let (tun_reader, tun_writer) = tokio::io::split(tun_device);

        let server = Arc::new(OpenvpnServer {
            tun_writer: Arc::new(Mutex::new(tun_writer)),
            interface_name: interface_name.clone(),
            socket: Arc::new(socket),
            local_addr,
            peer_manager: Arc::new(PeerManager::new()),
            server_session_id,
            ip_pool: Arc::new(RwLock::new(IpPool::new())),
            _tls_config: Arc::new(tls_config),
            llm_client,
        });

        // Run the UDP listener and the TUN reader inside a single task.
        //
        // `register_server_task` stores exactly one handle per server, so
        // registering two tasks would silently drop the first and leak it past
        // stop_server. `select!` keeps both loops owned by the one handle that is
        // registered, so aborting it cancels both.
        let server_clone = server.clone();
        let status_udp = status_tx.clone();
        let status_tun = status_tx.clone();
        let app_state_clone = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            tokio::select! {
                res = server_clone.handle_udp_packets(app_state_clone, server_id, status_udp) => {
                    if let Err(e) = res {
                        error!("OpenVPN UDP packet handler error: {}", e);
                    }
                }
                res = server_clone.handle_tun_packets(tun_reader, status_tun) => {
                    if let Err(e) = res {
                        error!("OpenVPN TUN packet handler error: {}", e);
                    }
                }
            }
        });

        app_state
            .register_server_task(server_id, accept_handle)
            .await;

        info!("OpenVPN server ready on {}", local_addr);
        let _ = status_tx.send(format!("→ OpenVPN server ready on {}", local_addr));
        let _ = status_tx.send(format!(
            "[INFO] VPN subnet {} on {} (INSECURE: shared hardcoded keys, no authentication)",
            VPN_NETWORK, local_addr
        ));

        Ok(local_addr)
    }

    /// Build a self-signed server certificate config.
    ///
    /// **This config is never used.** It is stored in `_tls_config` and no TLS
    /// handshake is ever performed against it - see the module docs. It exists so
    /// that a future real control channel has certificate material ready.
    fn create_tls_config() -> Result<rustls::ServerConfig> {
        use rcgen::{CertificateParams, DistinguishedName, KeyPair};
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        // Generate self-signed certificate for server
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "NetGet OpenVPN Server");
        params.distinguished_name = dn;

        // Generate key pair and self-sign
        let key_pair = KeyPair::generate().context("Failed to generate key pair")?;

        let cert = params
            .self_signed(&key_pair)
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
                self.handle_control_packet(packet, peer_addr, &app_state, server_id, &status_tx)
                    .await;
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

        trace!(
            "Control packet from {}: {:?}",
            peer_addr,
            control_packet.header.opcode
        );

        // Handle based on opcode
        match control_packet.header.opcode {
            Opcode::ControlHardResetClientV2 | Opcode::ControlHardResetClientV1 => {
                self.handle_handshake_initiation(
                    control_packet,
                    peer_addr,
                    app_state,
                    server_id,
                    status_tx,
                )
                .await;
            }
            Opcode::ControlV1 => {
                self.handle_control_message(control_packet, peer_addr).await;
            }
            _ => {
                trace!(
                    "Unhandled control opcode: {:?}",
                    control_packet.header.opcode
                );
            }
        }
    }

    /// Handle handshake initiation from client.
    ///
    /// **This is not a real OpenVPN handshake.** No TLS is negotiated and the peer
    /// is never authenticated - any host that sends a HARD_RESET is accepted, given
    /// a VPN IP and a data-channel cipher keyed from module-level constants. See
    /// the module docs.
    async fn handle_handshake_initiation(
        &self,
        control_packet: ControlPacket,
        peer_addr: SocketAddr,
        app_state: &Arc<AppState>,
        server_id: crate::state::ServerId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        console_info!(status_tx, "OpenVPN handshake from {}", peer_addr);

        // Check peer limit
        if self.peer_manager.count().await >= MAX_PEERS {
            warn!("Maximum peers reached, rejecting {}", peer_addr);
            let _ = status_tx.send(format!("[WARN] Max peers reached, rejecting {}", peer_addr));
            return;
        }

        // Create new peer
        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
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

        console_info!(status_tx, "Allocated VPN IP {} to {}", vpn_ip, peer_addr);

        peer.mark_connected(vpn_ip);

        // Add peer to manager
        self.peer_manager.add_peer(peer.clone()).await;

        // Send handshake response
        self.send_handshake_response(&peer, &control_packet, status_tx)
            .await;

        // Initialize data channel keys (simplified for MVP)
        self.initialize_data_channel(&peer, status_tx).await;

        // Add connection to app state
        let now = std::time::Instant::now();
        let conn_state = ConnectionState {
            id: connection_id,
            remote_addr: peer_addr,
            local_addr: self.local_addr,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::empty(),
        };

        app_state
            .add_connection_to_server(server_id, conn_state)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        info!("OpenVPN peer connected: {} (VPN IP: {})", peer_addr, vpn_ip);

        // Raise the peer-connected event. This runs any configured script/static
        // handler in-process and only falls back to a model call when none is set.
        let event = Event::new(
            &OPENVPN_PEER_CONNECTED_EVENT,
            serde_json::json!({
                "peer_addr": peer_addr.to_string(),
                "vpn_ip": vpn_ip.to_string(),
                "session_id": format!("{:016x}", peer.session_id),
                "authenticated": false,
            }),
        );

        let protocol = OpenvpnProtocol::new();
        match call_llm(
            &self.llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &event,
            &protocol,
        )
        .await
        {
            Ok(result) => {
                for message in &result.messages {
                    info!("{}", message);
                    let _ = status_tx.send(format!("[INFO] {}", message));
                }
            }
            Err(e) => {
                error!("OpenVPN peer event handling failed: {}", e);
                let _ = status_tx.send(format!("[ERROR] OpenVPN peer event failed: {}", e));
            }
        }
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
            console_error!(status_tx, "Failed to send handshake response: {}", e);
        } else {
            debug!("Sent handshake response to {}", peer.addr);
        }
    }

    /// Initialize the data channel cipher for a peer.
    ///
    /// # ⚠️ The keys derived here are public constants
    ///
    /// A real OpenVPN server derives data-channel keys from the TLS master secret
    /// negotiated during the control-channel handshake, mixed with per-session
    /// client and server randoms. **This function has none of those inputs**,
    /// because no handshake takes place. It instead runs HKDF over the three fixed
    /// literals below.
    ///
    /// Consequences:
    /// - Every peer, on every run, on every machine derives the *same* key.
    /// - The inputs are in this file and in the public git history, so anyone can
    ///   reproduce the key and decrypt the tunnel.
    /// - Peers are mutually indistinguishable at the crypto layer.
    ///
    /// The AEAD calls themselves are correct; the key material is worthless. This
    /// is the single biggest reason the protocol is marked `Incomplete`.
    async fn initialize_data_channel(
        &self,
        peer: &Peer,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        use crypto::derive_data_keys;

        // SECURITY: fixed inputs - see this function's doc comment. Not a secret.
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
        self.peer_manager
            .update_peer(&peer.addr, |p| {
                if let Err(e) = p.init_data_cipher(&keys, true) {
                    error!("Failed to initialize cipher: {}", e);
                } else {
                    debug!(
                        "Data channel initialized for {} using hardcoded keys (insecure)",
                        peer.addr
                    );
                    let _ = status_tx.send(format!(
                        "[DEBUG] Data channel ready for {} (hardcoded key - insecure)",
                        peer.addr
                    ));
                }
            })
            .await;
    }

    /// Handle a CONTROL_V1 message (key exchange, config push).
    ///
    /// **NOT IMPLEMENTED.** A real implementation would feed the packet's TLS
    /// payload into the control-channel TLS session, drive key exchange and push
    /// client configuration. This drops the packet on the floor, which is why
    /// clients can never complete a genuine negotiation.
    async fn handle_control_message(&self, _control_packet: ControlPacket, peer_addr: SocketAddr) {
        trace!(
            "Dropping CONTROL_V1 from {}: control channel not implemented",
            peer_addr
        );
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
        {
            let mut tun = self.tun_writer.lock().await;
            if let Err(e) = tun.write_all(&plaintext).await {
                error!("Failed to write to TUN: {}", e);
                let _ = status_tx.send(format!("[ERROR] TUN write failed: {}", e));
            }
        }

        // Update stats
        self.peer_manager
            .update_peer(&peer_addr, |p| {
                p.update_stats(0, plaintext.len() as u64);
            })
            .await;
    }

    /// Handle an ACK_V1 packet.
    ///
    /// **NOT IMPLEMENTED.** Control-channel reliability (retransmission windows,
    /// ACK tracking) does not exist; incoming ACKs are ignored.
    async fn handle_ack_packet(&self, _packet: &[u8], peer_addr: SocketAddr) {
        trace!(
            "Ignoring ACK from {}: control-channel reliability not implemented",
            peer_addr
        );
    }

    /// Handle outgoing packets from TUN (to be sent to VPN clients).
    ///
    /// Takes the owned read half so it never blocks writers: the previous version
    /// held a write lock on the whole device while parked in `read()`, which meant
    /// no client packet could ever be written to the TUN while the TUN was idle.
    async fn handle_tun_packets(
        &self,
        mut tun_reader: ReadHalf<tun::AsyncDevice>,
        _status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut buf = vec![0u8; 2048];

        loop {
            let len = tun_reader.read(&mut buf).await?;

            // Parse IP header to get destination. Bounds-check before indexing:
            // `len` is attacker-influenced (it is whatever the kernel hands us).
            if len < 20 {
                continue; // Too short for an IPv4 header
            }

            let ip_packet = &buf[..len];
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
                self.peer_manager
                    .update_peer(&peer.addr, |p| {
                        packet_id = p.next_packet_id();
                    })
                    .await;

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
                self.peer_manager
                    .update_peer(&peer.addr, |p| {
                        p.update_stats(serialized.len() as u64, 0);
                    })
                    .await;
            }
        }
    }

    /// Get peer list
    pub async fn list_peers(&self) -> Vec<(SocketAddr, Ipv4Addr)> {
        let peers = self.peer_manager.get_all_peers().await;
        peers
            .iter()
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
